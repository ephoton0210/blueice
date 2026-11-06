// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named ES5 classes after the existing AST-owned member and field lowering.

use super::*;
use crate::emitter::relocations;

mod accessors;
mod async_methods;
mod legacy;
mod origins;
mod super_members;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    origin_module: &Module,
    options: &CompilerOptions,
    wrapper: crate::ModuleKind,
) -> Result<EmittedJavaScript, Diagnostic> {
    let mut origin_options = options.clone();
    origin_options.module_kind = wrapper;
    let names = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| {
            if let Declaration::Class(class) = declaration {
                Some(class.name.clone())
            } else {
                None
            }
        })
        .chain(
            module
                .class_expressions
                .values()
                .filter_map(|expression| expression.name.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let mut remaining = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let Some(start) = (0..tokens.len()).rev().find(|index| {
            tokens[*index].is("class")
                && tokens
                    .get(*index + 1)
                    .is_some_and(|token| names.contains(&token.text))
        }) else {
            return Ok(emitted);
        };
        let name = &tokens[start + 1].text;
        let extends = tokens
            .get(start + 2)
            .is_some_and(|token| token.is("extends"));
        let open = start + if extends { 4 } else { 2 };
        if !tokens.get(open).is_some_and(|token| token.is("{")) {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "class heritage requires one retained base binding",
            ));
        }
        let close = exponentiation::matching_close(&tokens, open).ok_or_else(|| {
            unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "class body was not retained",
            )
        })?;
        let tail = legacy::extract(
            &emitted,
            &tokens,
            start,
            close,
            origin_module,
            options,
            wrapper,
        )?;
        let original_span =
            origins::original_span(&emitted, origin_module, &tokens, start, &origin_options);
        origins::retain_class_start(&mut emitted, origin_module, &tokens, start, original_span);
        origins::retain_expression_heritage(
            &mut emitted,
            origin_module,
            &tokens,
            start,
            original_span,
        );
        origins::retain_super_calls(
            &mut emitted,
            origin_module,
            &tokens,
            start,
            close,
            original_span,
        );
        let base = unused_name(&emitted.javascript, "base_class", &mut sequence);
        let inherit = if options.no_emit_helpers && !options.import_helpers {
            "__extends".to_string()
        } else {
            unused_name(&emitted.javascript, "inherit_class", &mut sequence)
        };
        let construct = if options.import_helpers || options.no_emit_helpers {
            "(function(base, receiver, values) { var result = Function.prototype.apply.call(base, receiver, values); return result !== null && (typeof result === 'object' || typeof result === 'function') ? result : receiver; })".to_string()
        } else {
            unused_name(&emitted.javascript, "call_base", &mut sequence)
        };
        let receiver = unused_name(&emitted.javascript, "class_this", &mut sequence);
        let accessor_pairs =
            accessors::read(&emitted, &tokens, (start, close), module, options, &base)?;
        let mut constructor = None;
        let mut members = String::new();
        let mut method_signatures = async_methods::Signatures::default();
        let mut cursor = open + 1;
        while cursor < close {
            if let Some(member) = accessor_pairs.get(&tokens[cursor].start) {
                members.push_str(&member.text);
                cursor = tokens.partition_point(|token| token.start < member.end);
                continue;
            }
            if tokens[cursor].is(";") {
                cursor += 1;
                continue;
            }
            let is_static = tokens[cursor].is("static")
                && !tokens.get(cursor + 1).is_some_and(|token| token.is("("));
            let first = cursor + usize::from(is_static);
            let accessor = matches!(tokens[first].text.as_str(), "get" | "set")
                && !tokens.get(first + 1).is_some_and(|token| token.is("("));
            let property = first + usize::from(accessor);
            let property_end = if tokens[property].is("[") {
                exponentiation::matching_close(&tokens, property).ok_or_else(|| {
                    unsupported(
                        module,
                        &SourceSpan::new(&module.id, 0, 0),
                        "computed member key was not retained",
                    )
                })? + 1
            } else {
                property + 1
            };
            let parameters = property_end;
            if !tokens.get(parameters).is_some_and(|token| token.is("(")) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "class member requires a retained named method",
                ));
            }
            let parameters_end = exponentiation::matching_close(&tokens, parameters).unwrap();
            let body = parameters_end + 1;
            if !tokens.get(body).is_some_and(|token| token.is("{")) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "class method body was not retained",
                ));
            }
            let end = exponentiation::matching_close(&tokens, body).unwrap();
            let arguments = relocations::relocated_verbatim(
                tokens[parameters].end,
                tokens[parameters_end].start,
            );
            let signature = relocations::source_anchor(tokens[cursor].start);
            let parameter_origin = if parameters_end == parameters + 1 {
                tokens[cursor].start
            } else {
                tokens[parameters_end].start
            };
            let parameter_end = relocations::source_anchor(parameter_origin);
            let first_body = tokens[body + 1].start;
            let text = format!(
                "{parameter_end}{{{}{}",
                &emitted.javascript[tokens[body].end..first_body],
                relocations::relocated_verbatim(first_body, tokens[end].end)
            );
            if tokens[property].is("constructor") && !is_static && !accessor {
                let text = if extends {
                    derived_body(
                        &emitted.javascript,
                        &tokens,
                        (body, end, parameter_origin),
                        &base,
                        &construct,
                        &receiver,
                        (module, options.downlevel_iteration),
                    )?
                } else {
                    text
                };
                constructor = Some(format!(
                    "{signature}function {name}({arguments}{parameter_end}) {text}\n"
                ));
            } else {
                let owner = if is_static {
                    name.clone()
                } else {
                    format!("{name}.prototype")
                };
                let text = if extends {
                    super_members::body(
                        &emitted.javascript,
                        &tokens,
                        (body, end, parameter_origin),
                        &base,
                        is_static,
                        module,
                    )?
                } else {
                    text
                };
                let define = options.defines_class_fields();
                let member_origin = if define {
                    relocations::unmapped().to_string()
                } else {
                    relocations::source_anchor(tokens[property].start)
                };
                let operation_origin = if define {
                    relocations::unmapped().to_string()
                } else {
                    relocations::source_anchor(tokens[property_end - 1].end)
                };
                let completed = if define {
                    relocations::unmapped().to_string()
                } else {
                    relocations::source_anchor(tokens[end].end)
                };
                let generated = relocations::unmapped();
                let key = if tokens[property].is("[") {
                    format!(
                        "({})",
                        relocations::relocated_verbatim(
                            tokens[property].end,
                            tokens[property_end - 1].start
                        )
                    )
                } else if matches!(tokens[property].kind, TokenKind::String | TokenKind::Number) {
                    tokens[property].text.clone()
                } else {
                    serde_json::to_string(&tokens[property].text).unwrap()
                };
                let literal_method = define
                    && !accessor
                    && matches!(tokens[property].kind, TokenKind::String | TokenKind::Number);
                let key_origin = if literal_method {
                    relocations::source_anchor(tokens[property].start)
                } else {
                    member_origin.clone()
                };
                let key_end = if literal_method {
                    relocations::source_anchor(tokens[property].end)
                } else {
                    generated.to_string()
                };
                let descriptor_origin = if literal_method { generated } else { "" };
                let body_completion = format!("{} ", relocations::source_anchor(tokens[end].end));
                let member_start = members.len();
                if accessor {
                    // Redefining only get or set retains the previously defined mate.
                    members.push_str(&format!("{generated}Object.defineProperty({member_origin}{owner}{generated}, {member_origin}{key}{generated}, {{ {}{operation_origin}: {signature}function ({arguments}{parameter_end}) {text}{generated}, enumerable: false, configurable: true }}){completed};\n",tokens[first].text));
                } else if !define {
                    let access = if tokens[property].is("[") {
                        format!("[{}]", &key[1..key.len() - 1])
                    } else if matches!(tokens[property].kind, TokenKind::String | TokenKind::Number)
                    {
                        format!("[{key}]")
                    } else {
                        format!(".{key_origin}{}{operation_origin}", tokens[property].text)
                    };
                    members.push_str(&format!("{member_origin}{owner}{access}{operation_origin} = {signature}function ({arguments}{parameter_end}) {text}{completed};\n"));
                } else {
                    members.push_str(&format!("{generated}Object.defineProperty({member_origin}{owner}{generated}, {key_origin}{key}{key_end}, {{{generated} enumerable: false, configurable: true, writable: true, {descriptor_origin}value{operation_origin}: {signature}function ({arguments}{parameter_end}) {text}{body_completion}{generated}}}){completed};\n"));
                }
                method_signatures.retain(
                    &emitted,
                    origin_module,
                    tokens[property].start,
                    &signature,
                    member_start,
                    &members,
                );
            }
            cursor = end + 1;
        }
        let exported = start > 0 && tokens[start - 1].is("export");
        let class_origin =
            relocations::source_anchor(tokens[if exported { start - 1 } else { start }].start);
        let binding_origin = if origins::generated_binding(original_span) {
            relocations::unmapped().to_string()
        } else {
            class_origin.clone()
        };
        let factory_origin = if original_span.is_some_and(|original| original.generated_factory) {
            relocations::unmapped().to_string()
        } else {
            class_origin.clone()
        };
        let class_close = relocations::source_anchor(tokens[close].start);
        let class_end = relocations::source_anchor(tokens[close].end);
        let invocation_origin = if original_span.is_some_and(|original| original.standard_factory) {
            class_end.clone()
        } else {
            class_origin.clone()
        };
        let generated = relocations::unmapped();
        let implicit_constructor = constructor.is_none();
        let implicit_origin = if origins::implicit_header_is_generated(original_span) {
            relocations::unmapped().to_string()
        } else {
            class_origin.clone()
        };
        let constructor = constructor.unwrap_or_else(|| {
            if extends {
                format!("{implicit_origin}function {name}() {{{generated} return {construct}({base}, this, arguments); {class_close}}}{class_end}\n")
            } else {
                format!("{implicit_origin}function {name}() {{{class_close}}}{class_end}\n")
            }
        });
        let inheritance = if extends {
            format!(
                "{}{inherit}({name}, {base});{}\n",
                relocations::source_anchor(tokens[start + 3].start),
                relocations::source_anchor(tokens[start + 3].end)
            )
        } else {
            String::new()
        };
        let argument = if extends {
            relocations::relocated_verbatim(tokens[start + 3].start, tokens[start + 3].end)
        } else {
            String::new()
        };
        let inside = &tail.inside;
        let factory_head = format!(
            "{factory_origin}(function ({}) {{\n{constructor}{inheritance}",
            if extends { base.as_str() } else { "" },
        );
        let expression = format!(
            "{factory_head}{members}{inside}{class_close}return {name}{class_end};\n{class_close}}}{invocation_origin})({argument}{})",
            if extends { relocations::source_anchor(tokens[start + 3].end) } else { invocation_origin.clone() }
        );
        let default_export =
            start >= 2 && tokens[start - 1].is("default") && tokens[start - 2].is("export");
        let mut replacement = if default_export {
            format!("{binding_origin}var {name} = {expression};{class_end} {generated}export default {name};")
        } else if exported {
            if original_span.is_some_and(|original| original.legacy_decorated) {
                format!(
                    "{binding_origin}var {name} = {expression};{class_end}{generated} {generated}export {{ {}{name}{} }};{}",
                    relocations::source_anchor(tokens[start + 1].start),
                    relocations::source_anchor(tokens[start + 1].end),
                    relocations::source_anchor(tokens[start + 1].end)
                )
            } else {
                format!(
                    "{binding_origin}var {name} = {expression};{class_end} {}export {{ {name} }};{}",
                    relocations::unmapped(),
                    relocations::unmapped()
                )
            }
        } else if class_is_expression(&tokens, start) {
            format!("{factory_origin}({expression})")
        } else {
            format!("{binding_origin}var {name} = {expression};")
        };
        replacement.push_str(&tail.after);
        let insertion = directive_end(&tokens);
        let helper = if options.import_helpers {
            if !extends {
                String::new()
            } else {
                format!(
                    "var {inherit} = {};",
                    super::super::super::helper_selection::reference(
                        &emitted.javascript,
                        "__extends"
                    )
                )
            }
        } else if options.no_emit_helpers {
            String::new()
        } else {
            include_str!("../class_helpers.v1.js")
                .replace("__blueice_target_inherit_class", &inherit)
                .replace("__blueice_target_call_base", &construct)
        };
        let replacement_start = tokens[if default_export {
            start - 2
        } else if exported {
            start - 1
        } else {
            start
        }]
        .start;
        let helper = format!("\n{helper}\n");
        let edits = vec![
            TextEdit {
                start: insertion,
                end: insertion,
                replacement: helper.clone(),
            },
            TextEdit {
                start: replacement_start,
                end: tail.end,
                replacement: replacement.clone(),
            },
        ];
        let rendered_expression = relocations::expand(&expression, &emitted.javascript, &edits)
            .iter()
            .map(|part| part.text().len())
            .sum::<usize>();
        let rendered = relocations::expand(&replacement, &emitted.javascript, &edits)
            .iter()
            .map(relocations::Part::text)
            .collect::<String>();
        // This is the generated wrapper's final return, after every moved
        // constructor and member. Its original endpoint can otherwise be
        // overwritten at the same position by the following export insertion.
        let completion =
            rendered.rfind(&format!("return {name};")).unwrap() + format!("return {name}").len();
        let completion = replacement_start
            + usize::from(insertion <= replacement_start) * helper.len()
            + completion;
        let publication_end = (exported || default_export).then_some(
            replacement_start
                + usize::from(insertion <= replacement_start) * helper.len()
                + rendered.len(),
        );
        let destination =
            replacement_start + usize::from(insertion <= replacement_start) * helper.len();
        let header = format!(
            "(function ({}) {{\n",
            if extends { base.as_str() } else { "" }
        );
        let header_end = destination + rendered.find(&header).unwrap() + header.len();
        let constructor_close = implicit_constructor.then(|| {
            let text = relocations::expand(&constructor, &emitted.javascript, &edits)
                .iter()
                .map(relocations::Part::text)
                .collect::<String>();
            header_end + text.rfind('}').expect("an implicit constructor has a body")
        });
        let expression_start =
            if !default_export && !exported && class_is_expression(&tokens, start) {
                factory_origin.len() + 1
            } else {
                format!("{binding_origin}var {name} = ").len()
            };
        let construction_end = destination
            + relocations::expand(
                &replacement[..expression_start
                    + expression.len()
                    + usize::from(
                        !default_export && !exported && class_is_expression(&tokens, start),
                    )],
                &emitted.javascript,
                &edits,
            )
            .iter()
            .map(|part| part.text().len())
            .sum::<usize>()
            + 1;
        let signature_points = method_signatures.positions(
            &replacement,
            expression_start + factory_head.len(),
            destination,
            &emitted.javascript,
            &edits,
        );
        emitted = mapped_edits(emitted, edits);
        if let Some(close) = constructor_close {
            origins::implicit_constructor_close(&mut emitted, origin_module, close, original_span);
        }
        for (generated, original) in signature_points {
            origins::point(&mut emitted, origin_module, generated, Some(original));
        }
        origins::expression_factory_prefix(
            &mut emitted,
            origin_module,
            destination,
            header_end,
            original_span,
        );
        origins::expression_factory_completion(
            &mut emitted,
            origin_module,
            destination + rendered.len(),
            original_span,
        );
        origins::completion(&mut emitted, origin_module, name, completion, original_span);
        if !class_is_expression(&tokens, start) {
            origins::legacy_binding_completion(
                &mut emitted,
                origin_module,
                destination + format!("var {name} = ").len() + rendered_expression + 1,
                original_span,
            );
        }
        origins::standard_factory_invocation(
            &mut emitted,
            origin_module,
            completion + 3,
            destination + rendered.len(),
            original_span,
        );
        origins::expression_factory_invocation(
            &mut emitted,
            origin_module,
            completion + 3,
            destination + rendered.len(),
            original_span,
        );
        origins::legacy_factory_invocation(
            &mut emitted,
            origin_module,
            completion + 3,
            construction_end - 1,
            original_span,
        );
        origins::declaration_completion(
            &mut emitted,
            origin_module,
            construction_end,
            original_span,
        );
        if let Some(end) = publication_end
            .filter(|_| !original_span.is_some_and(|original| original.legacy_decorated))
        {
            origins::publication_completion(&mut emitted, module, end);
        }
    }
}

/// A class expression produces a constructor value at its existing expression
/// site. Decorator lowering can introduce such a site for an original class
/// declaration, so classify the retained emitted syntax rather than its source
/// declaration alone.
pub(in crate::emitter) fn class_is_expression(tokens: &[crate::Token], start: usize) -> bool {
    start.checked_sub(1).is_some_and(|before| {
        matches!(
            tokens[before].text.as_str(),
            "=" | "("
                | "["
                | ","
                | ":"
                | "?"
                | "return"
                | "new"
                | "=>"
                | "&&"
                | "||"
                | "??"
                | "+"
                | "-"
                | "!"
                | "~"
                | "void"
                | "typeof"
                | "yield"
                | "await"
        )
    })
}

fn derived_body(
    source: &str,
    tokens: &[Token],
    body: (usize, usize, usize),
    base: &str,
    construct: &str,
    receiver: &str,
    context: (&Module, bool),
) -> Result<String, Diagnostic> {
    let (module, downlevel_iteration) = context;
    let (open, close, parameter_origin) = body;
    let mut cursor = tokens[open + 1].start;
    let mut output = format!(
        "{}{{{}\nvar {receiver};\n{}",
        relocations::source_anchor(parameter_origin),
        relocations::unmapped(),
        &source[tokens[open].end..cursor]
    );
    let mut index = open + 1;
    while index < close {
        if tokens[index].is("function") {
            if let Some(body) = (index + 1..close).find(|index| tokens[*index].is("{")) {
                if let Some(end) = exponentiation::matching_close(tokens, body) {
                    index = end + 1;
                    continue;
                }
            }
        }
        if tokens[index].is("super") {
            if !tokens.get(index + 1).is_some_and(|token| token.is("(")) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "derived constructor super properties require a retained receiver",
                ));
            }
            let end = exponentiation::matching_close(tokens, index + 1).unwrap();
            let arguments = &tokens[index + 2..end];
            let fresh_array = !downlevel_iteration
                && arguments.first().is_some_and(|token| token.is("..."))
                && arguments.get(1).is_some_and(|token| token.is("["))
                && exponentiation::matching_close(tokens, index + 3) == Some(end - 1);
            let values = if fresh_array {
                relocations::relocated_verbatim(tokens[index + 3].start, tokens[end - 1].end)
            } else if let [spread, arguments] = arguments {
                if spread.is("...") && arguments.is("arguments") {
                    "arguments".to_string()
                } else {
                    format!(
                        "{}[{}{} {}]",
                        relocations::unmapped(),
                        relocations::relocated_verbatim(tokens[index + 1].end, tokens[end].start),
                        relocations::source_anchor(tokens[end - 1].end),
                        relocations::unmapped()
                    )
                }
            } else if arguments.is_empty() {
                format!("{}[]", relocations::unmapped())
            } else {
                format!(
                    "{}[{}{} {}]",
                    relocations::unmapped(),
                    relocations::relocated_verbatim(tokens[index + 1].end, tokens[end].start),
                    relocations::source_anchor(tokens[end - 1].end),
                    relocations::unmapped()
                )
            };
            output.push_str(&relocations::relocated_verbatim(
                cursor,
                tokens[index].start,
            ));
            output.push_str(&format!(
                "{}{receiver} = {}{construct}{}({}{base}{}, this, {values}{})",
                relocations::unmapped(),
                if construct.starts_with("(function") {
                    relocations::unmapped().to_string()
                } else {
                    relocations::source_anchor(tokens[index].start)
                },
                relocations::source_anchor(tokens[index].end),
                relocations::source_anchor(tokens[index].start),
                relocations::source_anchor(tokens[index].end),
                relocations::source_anchor(tokens[end].start)
            ));
            let completed = if tokens[index + 2].is("...") {
                tokens[end - 1].end
            } else {
                tokens[end].end
            };
            cursor = tokens[end].end;
            index = end + 1;
            if tokens.get(index).is_some_and(|token| token.is(";")) {
                output.push_str(&format!(
                    "{};{}",
                    relocations::source_anchor(completed),
                    relocations::source_anchor(tokens[index].end)
                ));
                cursor = tokens[index].end;
                index += 1;
            }
            continue;
        }
        if tokens[index].is("this") {
            output.push_str(&relocations::relocated_verbatim(
                cursor,
                tokens[index].start,
            ));
            output.push_str(&format!(
                "{}{receiver}",
                relocations::source_anchor(tokens[index].start)
            ));
            cursor = tokens[index].end;
        }
        if tokens[index].is("return") {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "derived constructor returns require a retained constructor completion",
            ));
        }
        index += 1;
    }
    output.push_str(&relocations::relocated_verbatim(
        cursor,
        tokens[close].start,
    ));
    output.push_str(&format!(
        "{}\nreturn {receiver};\n",
        relocations::unmapped()
    ));
    output.push_str(&relocations::relocated_verbatim(
        tokens[close].start,
        tokens[close].end,
    ));
    Ok(output)
}
