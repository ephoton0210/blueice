// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named async functions on ES2015/ES2016 use the native generator protocol.
//! Generator parameters execute inside the promise boundary, including defaults.

use super::*;
use crate::emitter::relocations;

const HELPER: &str = include_str!("async_helpers.v1.js");

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target < crate::EcmaTarget::Es2015 || options.target >= crate::EcmaTarget::Es2017 {
        return Ok(emitted);
    }
    let source_tokens = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
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
        let candidate = (0..tokens.len()).rev().find_map(|index| {
            if !tokens[index].is("async") || !tokens.get(index + 1)?.is("function") {
                return None;
            }
            let function = named_function(&tokens, index + 1)?;
            Some((index, function))
        });
        let Some((start, function)) = candidate else {
            return Ok(emitted);
        };
        let arrow = arrow_at(&emitted, module, tokens[start].start);
        let method = method_end_at(&emitted, module, tokens[function.body_end].start).is_some();
        let body = resumed_body(
            &emitted.javascript,
            &tokens,
            &function,
            module,
            &options.limits.parser,
            arrow,
        )?;
        let external = super::super::helper_selection::external(options);
        let helper = if external {
            super::super::helper_selection::reference(&emitted.javascript, "__awaiter")
        } else {
            unused_name(&emitted.javascript, "async", &mut sequence)
        };
        let count = parameter_length(&tokens[function.parameters + 1..function.parameters_end]);
        let parameters = (0..count)
            .map(|_| unused_name(&emitted.javascript, "async_argument", &mut sequence))
            .collect::<Vec<_>>()
            .join(", ");
        let originals_end = if tokens[function.parameters_end - 1].is(",") {
            tokens[function.parameters_end - 1].start
        } else {
            tokens[function.parameters_end].start
        };
        let originals =
            relocations::relocated_verbatim(tokens[function.parameters].end, originals_end);
        let name = relocations::relocated_verbatim(tokens[start + 2].start, tokens[start + 2].end);
        let name_completion =
            named_completion(&emitted, module, &source_tokens, &tokens[start + 2]);
        let start_anchor = relocations::source_anchor(tokens[start].start);
        let signature_anchor = relocations::source_anchor(tokens[function.parameters_end].start);
        let body_anchor = relocations::source_anchor(tokens[function.body].start);
        let plain_arrow = arrow && plain_arrow_formals(&emitted, module, tokens[start].start);
        let driver_anchor = if arrow {
            relocations::source_anchor(tokens[function.body].end)
        } else {
            relocations::unmapped().to_string()
        };
        let parameters_anchor = if arrow {
            driver_anchor.clone()
        } else {
            relocations::source_anchor(tokens[function.parameters].start)
        };
        let body_close = relocations::relocated_verbatim(
            tokens[function.body_end].start,
            tokens[function.body_end].end,
        );
        let generated = relocations::unmapped();
        let callback_anchor = if arrow {
            driver_anchor.as_str()
        } else {
            generated
        };
        let callback_body_anchor = if plain_arrow {
            driver_anchor.as_str()
        } else {
            ""
        };
        let replacement = format!(
            "{start_anchor}function {name}{name_completion} {generated}({parameters}) {body_anchor}{{{generated}\n\
             if (new.target) throw new TypeError(\"Async function is not a constructor\");\n\
             return {driver_anchor}{helper}(this, arguments, {}{callback_anchor}function* {parameters_anchor}({originals}{signature_anchor}) {callback_body_anchor}{{{body}{body_close});\n{generated}}}",
            if external { "void 0, " } else { "" }
        );
        let insertion = directive_end(&tokens);
        let declaration_end = if method {
            tokens
                .get(function.body_end + 1)
                .filter(|next| {
                    next.is(")")
                        && emitted.javascript[tokens[function.body_end].end..next.start]
                            .chars()
                            .all(char::is_whitespace)
                })
                .map_or(tokens[function.body_end].end, |next| next.start)
        } else {
            async_callable_end(&emitted.javascript, &tokens, &function)
        };
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: if external {
                        String::new()
                    } else {
                        format!("\n{}\n", HELPER.replace("__blueice_target_async", &helper))
                    },
                },
                TextEdit {
                    start: tokens[start].start,
                    end: declaration_end,
                    replacement,
                },
            ],
        );
    }
}

/// Preserve an original declaration's name ending before its generated arity.
/// Synthesized arrow/method names have no original declaration identity.
pub(super) fn named_completion(
    emitted: &EmittedJavaScript,
    module: &Module,
    source_tokens: &[Token],
    name: &Token,
) -> String {
    let Some(original) = source_offset(emitted, module, name.start) else {
        return relocations::unmapped().to_string();
    };
    let index = source_tokens.partition_point(|token| token.start < original);
    let Some(source_name) = source_tokens.get(index).filter(|token| {
        token.start == original && token.kind == TokenKind::Identifier && token.text == name.text
    }) else {
        return relocations::unmapped().to_string();
    };
    let Some(keyword) = index
        .checked_sub(1)
        .and_then(|index| source_tokens.get(index))
    else {
        return relocations::unmapped().to_string();
    };
    if !keyword.is("function") {
        return relocations::unmapped().to_string();
    }
    let original_declaration =
        runtime_declarations(&module.declarations)
            .into_iter()
            .any(|(declaration, _)| match declaration {
                Declaration::Function(function)
                    if function.async_function
                        && !function.generator
                        && !function.anonymous
                        && !function.declared
                        && !function.overload
                        && function.name == source_name.text =>
                {
                    function.body_open.is_some_and(|body| {
                        source_tokens
                            .iter()
                            .find(|token| {
                                token.start >= function.span.start
                                    && token.start < body
                                    && token.is("function")
                            })
                            .is_some_and(|token| token.start == keyword.start)
                    })
                }
                _ => false,
            });
    if original_declaration {
        relocations::source_anchor(name.end)
    } else {
        relocations::unmapped().to_string()
    }
}

/// The original async-arrow node owns its synthesized callable's entry point.
pub(super) fn arrow_at(emitted: &EmittedJavaScript, module: &Module, offset: usize) -> bool {
    source_offset(emitted, module, offset)
        .and_then(|offset| module.nested_functions.get(&offset))
        .is_some_and(|function| {
            function.async_function && function.kind == crate::parser::NestedFunctionKind::Arrow
        })
}

pub(super) fn method_end_at(
    emitted: &EmittedJavaScript,
    module: &Module,
    offset: usize,
) -> Option<usize> {
    let closing = source_offset(emitted, module, offset)?;
    let end = closing.checked_add(1)?;
    if module.source.get(closing..end) != Some("}") {
        return None;
    }
    let object = module.nested_functions.values().any(|node| {
        node.async_function
            && !node.generator
            && node.kind == crate::parser::NestedFunctionKind::Method
            && node.span.end == end
    });
    let classes = crate::emitter::runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class),
            _ => None,
        })
        .chain(module.class_expressions.values().map(|node| &node.class));
    let class = classes.flat_map(|class| &class.members).any(|member| {
        member.method.as_ref().is_some_and(|node| {
            node.async_function && !node.generator && node.body.is_some() && node.span.end == end
        })
    });
    (object || class).then_some(end)
}

pub(super) fn class_method_at<'m>(
    emitted: &EmittedJavaScript,
    module: &'m Module,
    offset: usize,
) -> Option<&'m crate::parser::ClassMethod> {
    let property = source_offset(emitted, module, offset)?;
    let mut methods = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class),
            _ => None,
        })
        .chain(module.class_expressions.values().map(|node| &node.class))
        .flat_map(|class| &class.members)
        .filter_map(|member| member.method.as_ref())
        .filter(|method| {
            method.async_function
                && !method.generator
                && method.body.is_some()
                && method.name_span.start == property
        });
    let method = methods.next()?;
    methods.next().is_none().then_some(method)
}

pub(super) fn plain_arrow_formals(
    emitted: &EmittedJavaScript,
    module: &Module,
    offset: usize,
) -> bool {
    source_offset(emitted, module, offset)
        .and_then(|offset| module.nested_functions.get(&offset))
        .is_some_and(|function| {
            function.async_function
                && function.kind == crate::parser::NestedFunctionKind::Arrow
                && !function.parameters.is_empty()
                && function.parameters.iter().all(|parameter| {
                    parameter.default.is_none() && parameter.pattern.is_none() && !parameter.rest
                })
        })
}

pub(super) fn source_offset(
    emitted: &EmittedJavaScript,
    module: &Module,
    offset: usize,
) -> Option<usize> {
    let prefix = &emitted.javascript[..offset];
    let key = (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    );
    let index = emitted
        .provenance
        .partition_point(|point| (point.generated_line, point.generated_column) <= key);
    let point = index
        .checked_sub(1)
        .and_then(|index| emitted.provenance.get(index))
        .filter(|point| point.generated_line == key.0 && point.mapped)?;
    let mut lines = module.source.split_inclusive('\n');
    let start: usize = lines.by_ref().take(point.source_line).map(str::len).sum();
    let line = lines.next()?;
    let mut column = 0;
    for (byte, character) in line.char_indices() {
        if column == point.source_column {
            return Some(start + byte);
        }
        column += character.len_utf16();
        if column > point.source_column {
            return None;
        }
    }
    None
}

pub(super) struct Function {
    pub(super) parameters: usize,
    pub(super) parameters_end: usize,
    pub(super) body: usize,
    pub(super) body_end: usize,
}

/// Consume whitespace before an async callable's following semicolon. The
/// renderer uses the same boundary with or without source-origin tracking;
/// an async_forms completion carrier belongs to the callable, not its factory.
pub(super) fn async_callable_end(source: &str, tokens: &[Token], function: &Function) -> usize {
    let end = tokens[function.body_end].end;
    tokens
        .get(function.body_end + 1)
        .filter(|next| next.is(";") && source[end..next.start].chars().all(char::is_whitespace))
        .map_or(end, |next| next.start)
}

fn named_function(tokens: &[Token], start: usize) -> Option<Function> {
    if tokens.get(start + 1)?.kind != TokenKind::Identifier || !tokens.get(start + 2)?.is("(") {
        return None;
    }
    function_at_parameters(tokens, start + 2)
}

pub(super) fn function_at_parameters(tokens: &[Token], parameters: usize) -> Option<Function> {
    let parameters_end = exponentiation::matching_close(tokens, parameters)?;
    let body = parameters_end + 1;
    if !tokens.get(body)?.is("{") {
        return None;
    }
    Some(Function {
        parameters,
        parameters_end,
        body,
        body_end: exponentiation::matching_close(tokens, body)?,
    })
}

fn resumed_body(
    source: &str,
    tokens: &[Token],
    function: &Function,
    module: &Module,
    limits: &crate::parser::ParserLimits,
    arrow: bool,
) -> Result<String, Diagnostic> {
    let start = tokens[function.body].end;
    let end = tokens[function.body_end].start;
    let leading = source[start..end].len() - source[start..end].trim_start().len();
    let body = resumed_text(
        source,
        start + leading,
        end,
        module,
        limits,
        limits.max_type_depth.min(128),
    )?;
    Ok(format!(
        "{}{}{body}",
        if arrow { "" } else { relocations::unmapped() },
        &source[start..start + leading]
    ))
}
fn resumed_text(
    source: &str,
    start: usize,
    end: usize,
    module: &Module,
    limits: &crate::parser::ParserLimits,
    depth: usize,
) -> Result<String, Diagnostic> {
    if depth == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(&module.id, 0, 0),
            "await lowering exceeds the depth limit",
        ));
    }
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &source[start..end],
        limits.max_source_bytes,
        limits.max_tokens,
    )
    .map_err(|mut d| d.remove(0))?;
    let mut edits = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].is("function") {
            if let Some(open) = (at + 1..tokens.len()).find(|&p| tokens[p].is("(")) {
                if let Some(function) = function_at_parameters(&tokens, open) {
                    at = function.body_end + 1;
                    continue;
                }
            }
        }
        if tokens[at].kind == TokenKind::Template {
            let replacement = mapped_fragments::template(
                source,
                start + tokens[at].start,
                start + tokens[at].end,
                module,
                depth - 1,
                |start, end, depth| resumed_text(source, start, end, module, limits, depth),
            )?;
            edits.push(TextEdit {
                start: start + tokens[at].start,
                end: start + tokens[at].end,
                replacement,
            });
            at += 1;
            continue;
        }
        if tokens[at].is("await")
            && !at
                .checked_sub(1)
                .is_some_and(|p| matches!(tokens[p].text.as_str(), "." | "?."))
            && !tokens.get(at + 1).is_some_and(|t| t.is(":"))
        {
            let end = exponentiation::operand_end(&tokens, at + 1).ok_or_else(|| {
                Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    SourceSpan::new(&module.id, 0, 0),
                    "await operand was not retained",
                )
            })?;
            let operand = resumed_text(
                source,
                start + tokens[at + 1].start,
                start + tokens[end - 1].end,
                module,
                limits,
                depth - 1,
            )?;
            edits.push(TextEdit {
                start: start + tokens[at].start,
                end: start + tokens[end - 1].end,
                replacement: format!(
                    "{}({}yield {}({operand}{} {}))",
                    relocations::unmapped(),
                    relocations::source_anchor(start + tokens[at].start),
                    relocations::unmapped(),
                    relocations::source_anchor(start + tokens[end - 1].end),
                    relocations::unmapped()
                ),
            });
            at = end;
        } else {
            at += 1;
        }
    }
    Ok(mapped_fragments::splice(start, end, edits))
}

pub(super) fn parameter_length(tokens: &[Token]) -> usize {
    if tokens.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut depth = 0usize;
    for token in tokens {
        match token.text.as_str() {
            "=" | "..." if depth == 0 => return count,
            "," if depth == 0 => count += 1,
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    count + usize::from(!tokens.last().is_some_and(|token| token.is(",")))
}
