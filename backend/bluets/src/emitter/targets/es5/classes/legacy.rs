// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AST-owned ES5 class initialization completes before the outer binding is assigned.
use super::*;
use crate::parser::{ClassDeclaration, ClassMemberKind};

pub(super) struct Tail {
    pub end: usize,
    pub inside: String,
    pub after: String,
}

enum Phase {
    Exact(Vec<String>),
    Assignment(String),
    PrivateFunction(String),
    Field {
        prefix: Vec<String>,
        define: bool,
    },
    StaticBlock,
    Member {
        name: String,
        numeric_key: Option<String>,
        is_static: bool,
        field: bool,
    },
    Class,
}

fn location(source: &str, offset: usize) -> (usize, usize) {
    let mut characters = source[..offset].chars().peekable();
    let (mut line, mut column) = (0, 0);
    while let Some(character) = characters.next() {
        if crate::emitter::is_line_break(character, characters.peek().copied()) {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16();
        }
    }
    (line, column)
}

fn original<'a>(
    previous: &EmittedJavaScript,
    tokens: &[Token],
    start: usize,
    module: &'a Module,
) -> Option<&'a ClassDeclaration> {
    let origins = [tokens[start].start, tokens[start + 1].start]
        .into_iter()
        .filter_map(|offset| {
            let key = location(&previous.javascript, offset);
            let index = previous
                .provenance
                .partition_point(|point| (point.generated_line, point.generated_column) <= key);
            index
                .checked_sub(1)
                .and_then(|index| previous.provenance.get(index))
                .filter(|point| point.mapped && point.generated_line == key.0)
                .map(|point| (point.source_line, point.source_column))
        })
        .collect::<Vec<_>>();
    let mut candidates = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| {
            let Declaration::Class(class) = declaration else {
                return None;
            };
            if class.name != tokens[start + 1].text {
                return None;
            }
            let owned = [
                class.span.start,
                class.name_span.start,
                crate::emitter::decorators::class_keyword_start(class, &module.source),
            ];
            owned
                .into_iter()
                .any(|offset| origins.contains(&location(&module.source, offset)))
                .then_some(class)
        });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn words(module: &Module, text: &str) -> Result<Vec<String>, Diagnostic> {
    crate::lex(&module.id, text)
        .map(|tokens| {
            tokens
                .into_iter()
                .filter(|token| token.kind != TokenKind::Eof)
                .map(|token| token.text)
                .collect()
        })
        .map_err(|mut errors| errors.remove(0))
}

fn phases(
    module: &Module,
    class: &ClassDeclaration,
    options: &CompilerOptions,
) -> Result<Vec<Phase>, Diagnostic> {
    let original = crate::lex(&module.id, &module.source).map_err(|mut errors| errors.remove(0))?;
    let mut result = Vec::new();
    let mut pending = Vec::new();
    for member in &class.members {
        if member.abstract_modifier.is_some()
            || member.field.as_ref().is_some_and(|field| field.declared)
        {
            continue;
        }
        if member.key.first().is_some_and(|token| token.is("[")) {
            if member.field.is_some() {
                pending.push(crate::emitter::computed_fields::captured_key_name(
                    &original,
                    member.key[0].start,
                ));
            } else {
                pending.clear(); // The preceding keys are evaluated inside this retained method key.
            }
        }
    }
    result.extend(pending.into_iter().map(Phase::Assignment));
    if let Some(private) = crate::emitter::private_lowering::PrivateNames::collect(module, class)? {
        for setup in private
            .instance_setup()
            .split(';')
            .filter(|setup| !setup.trim().is_empty())
        {
            result.push(Phase::Exact(words(module, &format!("{setup};"))?));
        }
        for member in &class.members {
            let private_callable = member
                .method
                .as_ref()
                .is_some_and(|method| method.name.starts_with('#'))
                || member
                    .accessor
                    .as_ref()
                    .is_some_and(|accessor| accessor.name.starts_with('#'));
            if private_callable {
                let text = private.function_definition(
                    member,
                    &module.source[member.span.start..member.span.end],
                )?;
                result.push(Phase::PrivateFunction(
                    text.split_whitespace().next().unwrap().to_string(),
                ));
            }
        }
    }
    for member in &class.members {
        if member.abstract_modifier.is_some()
            || member.field.as_ref().is_some_and(|field| field.declared)
        {
            continue;
        }
        if let Some(field) = member.field.as_ref().filter(|field| field.is_static) {
            if field.initializer.is_none()
                && !options.defines_class_fields()
                && !field.name.starts_with('#')
            {
                continue;
            }
            if field.name.starts_with('#') {
                // Native rejects private identifiers at ES5 before this pass;
                // recovery emission still keeps its exact generated holder.
                let private =
                    crate::emitter::private_lowering::PrivateNames::collect(module, class)?
                        .unwrap();
                let store = private.static_field_store(&field.name, None);
                result.push(Phase::Assignment(
                    store.split_whitespace().next().unwrap().to_string(),
                ));
                continue;
            }
            let captured = member
                .key
                .first()
                .filter(|token| token.is("["))
                .map(|token| {
                    crate::emitter::computed_fields::captured_key_name(&original, token.start)
                });
            let key = captured
                .clone()
                .unwrap_or_else(|| format!("{:?}", field.name));
            let define = options.defines_class_fields();
            let prefix = if define {
                format!("Object.defineProperty({}, {key}, {{ enumerable: true, configurable: true, writable: true, value:", class.name)
            } else {
                let access = captured.map_or_else(
                    || {
                        if field.name.chars().all(|character| {
                            character.is_alphanumeric() || matches!(character, '_' | '$')
                        }) && !field
                            .name
                            .chars()
                            .next()
                            .is_some_and(|character| character.is_ascii_digit())
                        {
                            format!(".{}", field.name)
                        } else {
                            format!("[{key}]")
                        }
                    },
                    |key| format!("[{key}]"),
                );
                format!("{}{access} =", class.name)
            };
            result.push(Phase::Field {
                prefix: words(module, &prefix)?,
                define,
            });
        } else if member.static_block.is_some() {
            result.push(Phase::StaticBlock);
        }
    }
    for is_static in [false, true] {
        let mut accessors = std::collections::BTreeSet::new();
        for member in &class.members {
            let decorated = !member.decorators.is_empty()
                || member.method.as_ref().is_some_and(|method| {
                    method
                        .parameters
                        .iter()
                        .any(|parameter| !parameter.decorators.is_empty())
                });
            if !decorated {
                continue;
            }
            let (name, static_member, field) = match member.kind {
                ClassMemberKind::Method => {
                    let Some(method) = member
                        .method
                        .as_ref()
                        .filter(|method| method.body.is_some())
                    else {
                        continue;
                    };
                    (&method.name, method.is_static, false)
                }
                ClassMemberKind::Accessor => {
                    let Some(accessor) = &member.accessor else {
                        continue;
                    };
                    if !accessors.insert((accessor.name.clone(), accessor.is_static)) {
                        continue;
                    }
                    (&accessor.name, accessor.is_static, false)
                }
                ClassMemberKind::Field => {
                    let Some(field) = &member.field else {
                        continue;
                    };
                    (&field.name, field.is_static, !field.accessor)
                }
                _ => continue,
            };
            if static_member == is_static {
                result.push(Phase::Member {
                    name: name.clone(),
                    numeric_key: match member.key.as_slice() {
                        [key] if key.kind == TokenKind::Number => Some(key.text.clone()),
                        _ => None,
                    },
                    is_static,
                    field,
                });
            }
        }
    }
    if crate::emitter::legacy_decorators::lowers_class(class) {
        result.push(Phase::Class);
    }
    Ok(result)
}

fn statement_end(tokens: &[Token], begin: usize) -> Option<usize> {
    let mut index = begin;
    while index < tokens.len() {
        if tokens[index].is(";") {
            return Some(index + 1);
        }
        if matches!(tokens[index].text.as_str(), "(" | "[" | "{") {
            index = exponentiation::matching_close(tokens, index)?;
        } else if matches!(tokens[index].text.as_str(), ")" | "]" | "}") {
            return None;
        }
        index += 1;
    }
    None
}

fn matches_phase(tokens: &[Token], phase: &Phase, class: &ClassDeclaration) -> bool {
    let texts = tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>();
    match phase {
        Phase::Exact(expected) => texts == expected.iter().map(String::as_str).collect::<Vec<_>>(),
        Phase::Assignment(name) => {
            texts.first() == Some(&name.as_str()) && texts.get(1) == Some(&"=")
        }
        Phase::PrivateFunction(name) => {
            texts.starts_with(&[name.as_str(), "=", "function", name.as_str(), "("])
        }
        Phase::Field { prefix, define } => {
            texts.starts_with(&prefix.iter().map(String::as_str).collect::<Vec<_>>())
                && (!define || texts.ends_with(&["}", ")", ";"]))
        }
        Phase::StaticBlock => {
            texts.starts_with(&["(", "function", "(", ")", "{"])
                && texts.ends_with(&["}", ")", ".", "call", "(", &class.name, ")", ";"])
        }
        Phase::Class | Phase::Member { .. } => {
            let begin = if matches!(phase, Phase::Class) {
                if !texts.starts_with(&[&class.name, "=", "__bluetsDecorate", "(", "["]) {
                    return false;
                }
                4
            } else {
                if !texts.starts_with(&["__bluetsDecorate", "(", "["]) {
                    return false;
                }
                2
            };
            let Some(close) = exponentiation::matching_close(tokens, begin) else {
                return false;
            };
            let tail = &tokens[close + 1..];
            if matches!(phase, Phase::Class) {
                return tail.iter().map(|token| token.text.as_str()).eq([
                    ",",
                    &class.name,
                    ")",
                    ";",
                ]);
            }
            let Phase::Member {
                name,
                numeric_key,
                is_static,
                field,
            } = phase
            else {
                unreachable!()
            };
            let target = if *is_static {
                vec![",", class.name.as_str(), ","]
            } else {
                vec![",", class.name.as_str(), ".", "prototype", ","]
            };
            let key = target.len();
            if !tail
                .iter()
                .map(|token| token.text.as_str())
                .take(key)
                .eq(target)
            {
                return false;
            }
            let key_matches = tail.get(key).is_some_and(|token| {
                numeric_key.as_ref().map_or_else(
                    || crate::syntax::string_contents(token).as_deref() == Some(name.as_str()),
                    |expected| token.kind == TokenKind::Number && &token.text == expected,
                )
            });
            if !key_matches {
                return false;
            }
            tail[key + 1..]
                .iter()
                .map(|token| token.text.as_str())
                .eq(if *field {
                    vec![",", "void", "0", ")", ";"]
                } else {
                    vec![",", "null", ")", ";"]
                })
        }
    }
}

/// Move only the exact generated inventory belonging to this original class.
pub(super) fn extract(
    previous: &EmittedJavaScript,
    tokens: &[Token],
    start: usize,
    close: usize,
    module: &Module,
    options: &CompilerOptions,
    wrapper: crate::ModuleKind,
) -> Result<Tail, Diagnostic> {
    let empty = || Tail {
        end: tokens[close].end,
        inside: String::new(),
        after: String::new(),
    };
    if !options.experimental_decorators {
        return Ok(empty());
    }
    let Some(class) = original(previous, tokens, start, module) else {
        return Ok(empty());
    };
    let phases = phases(module, class, options)?;
    if phases.is_empty() {
        return Ok(empty());
    }
    let publication = if class.exported {
        Some(
            if let Some((namespace, _)) = crate::emitter::decorators::namespace_owner(module, class)
            {
                format!("{namespace}.{0} = {0};", class.name)
            } else if options.module_kind == crate::ModuleKind::CommonJs {
                format!("exports.{} = {};", class.export_name(), class.name)
            } else if crate::emitter::legacy_decorators::lowers_class(class) {
                if class.default_export {
                    format!("export default {};", class.name)
                } else {
                    format!("export {{ {} }};", class.name)
                }
            } else {
                String::new()
            },
        )
    } else {
        None
    };
    let publication = publication
        .as_deref()
        .map(|text| words(module, text))
        .transpose()?
        .filter(|words| !words.is_empty());
    let expression = super::class_is_expression(tokens, start);
    let mut cursor = close + 1;
    let mut after = String::new();
    if expression && tokens.get(cursor).is_some_and(|token| token.is(";")) {
        after.push(';');
        cursor += 1;
    }
    let mut inside = String::new();
    let mut published = false;
    let consume_publication = |cursor: &mut usize, after: &mut String, published: &mut bool| {
        if *published {
            return;
        }
        let Some(expected) = &publication else {
            return;
        };
        let Some(end) = statement_end(tokens, *cursor) else {
            return;
        };
        if tokens[*cursor..end]
            .iter()
            .map(|token| &token.text)
            .eq(expected.iter())
        {
            if tokens[*cursor].is("export") && tokens[*cursor + 1].is("{") && end == *cursor + 5 {
                // The ES5 class's separately emitted ESM specifier retains
                // the original class-name range. This publication is owned
                // by its exact original class, independently of its IIFE.
                let binding = &tokens[*cursor + 2];
                after
                    .push_str(&previous.javascript[tokens[*cursor - 1].end..tokens[*cursor].start]);
                after.push_str(relocations::unmapped());
                after.push_str(&previous.javascript[tokens[*cursor].start..binding.start]);
                after.push_str(&relocations::source_anchor(tokens[start + 1].start));
                after.push_str(&binding.text);
                after.push_str(&relocations::source_anchor(tokens[start + 1].end));
                after.push_str(&previous.javascript[binding.end..tokens[end - 1].end]);
                after.push_str(relocations::unmapped());
            } else {
                if wrapper == crate::ModuleKind::System {
                    after.push_str(relocations::unmapped());
                } else {
                    after.push_str(&relocations::source_anchor(tokens[close].end));
                }
                after
                    .push_str(&previous.javascript[tokens[*cursor - 1].end..tokens[*cursor].start]);
                after.push_str(&relocations::relocated_verbatim(
                    tokens[*cursor].start,
                    tokens[end - 1].end,
                ));
            }
            *cursor = end;
            *published = true;
        }
    };
    for phase in &phases {
        consume_publication(&mut cursor, &mut after, &mut published);
        let end = statement_end(tokens, cursor).ok_or_else(|| {
            unsupported(
                module,
                &class.span,
                "an owned legacy initialization statement lost its balanced boundary",
            )
        })?;
        if !matches_phase(&tokens[cursor..end], phase, class) {
            return Err(unsupported(
                module,
                &class.span,
                "an owned legacy initialization phase did not match its original class inventory",
            ));
        }
        inside.push('\n');
        inside.push_str(&relocations::relocated_verbatim(
            tokens[cursor].start,
            tokens[end - 1].end,
        ));
        cursor = end;
    }
    consume_publication(&mut cursor, &mut after, &mut published);
    if publication.is_some() && !published {
        return Err(unsupported(
            module,
            &class.span,
            "an owned legacy class lost its final publication",
        ));
    }
    inside.push('\n');
    let mut end = tokens[cursor - 1].end;
    let trivia_start = end;
    while previous
        .javascript
        .as_bytes()
        .get(end)
        .is_some_and(u8::is_ascii_whitespace)
    {
        end += 1;
    }
    if published {
        if wrapper == crate::ModuleKind::System {
            after.push_str(relocations::unmapped());
        } else {
            after.push_str(&relocations::source_anchor(tokens[start + 1].end));
        }
    }
    if trivia_start == end {
        after.push(' ');
    } else {
        after.push_str(&previous.javascript[trivia_start..end]);
    }
    after.push_str(relocations::unmapped());
    Ok(Tail { end, inside, after })
}
