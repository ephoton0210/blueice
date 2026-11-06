// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named async generators on ES2015--ES2017 use queued native generators.
//! Original await/yield/return operands are rewritten from retained tokens;
//! nested functions keep their own suspension and completion boundaries.

use super::*;
use crate::emitter::relocations;

const HELPER: &str = include_str!("async_generator_helpers.v1.js");

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target < crate::EcmaTarget::Es2015 || options.target >= crate::EcmaTarget::Es2018 {
        return Ok(emitted);
    }
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
        let candidate = (0..tokens.len()).rev().find_map(|start| {
            if !tokens[start].is("async")
                || !tokens.get(start + 1)?.is("function")
                || !tokens.get(start + 2)?.is("*")
                || tokens.get(start + 3)?.kind != TokenKind::Identifier
                || !tokens.get(start + 4)?.is("(")
            {
                return None;
            }
            let function = async_functions::function_at_parameters(&tokens, start + 4)?;
            Some((start, function))
        });
        let Some((start, function)) = candidate else {
            return Ok(emitted);
        };
        let external = crate::emitter::helper_selection::external(options);
        let helper = if external {
            crate::emitter::helper_selection::reference(&emitted.javascript, "__asyncGenerator")
        } else {
            unused_name(&emitted.javascript, "async_generator", &mut sequence)
        };
        let marker = if external {
            crate::emitter::helper_selection::reference(&emitted.javascript, "__await")
        } else {
            unused_name(&emitted.javascript, "generator_suspension", &mut sequence)
        };
        let Some(body) = rewrite(
            &emitted.javascript,
            tokens[function.body].end,
            tokens[function.body_end].start,
            (&marker, external),
            options.limits.parser.max_type_depth.min(128),
            (module, &options.limits.parser),
        ) else {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                "async generator suspension scope was not retained",
            ));
        };
        let count = async_functions::parameter_length(
            &tokens[function.parameters + 1..function.parameters_end],
        );
        let parameters = (0..count)
            .map(|_| unused_name(&emitted.javascript, "generator_argument", &mut sequence))
            .collect::<Vec<_>>()
            .join(", ");
        let originals_end = if tokens[function.parameters_end - 1].is(",") {
            tokens[function.parameters_end - 1].start
        } else {
            tokens[function.parameters_end].start
        };
        let originals =
            relocations::relocated_verbatim(tokens[function.parameters].end, originals_end);
        let name = relocations::relocated_verbatim(tokens[start + 3].start, tokens[start + 3].end);
        let start_anchor = relocations::source_anchor(tokens[start].start);
        let signature = if function.parameters + 1 == function.parameters_end {
            tokens[function.parameters].start
        } else {
            tokens[function.parameters_end].start
        };
        let signature_anchor = relocations::source_anchor(signature);
        let generated = relocations::unmapped();
        let body_close = relocations::relocated_verbatim(
            tokens[function.body_end].start,
            tokens[function.body_end].end,
        );
        let completion = relocations::source_anchor(tokens[function.body_end].end);
        let replacement = format!(
            "{start_anchor}function {name}{signature_anchor}({parameters}) {signature_anchor}{{\n\
             {generated}if (new.target) throw new TypeError(\"Async generator is not a constructor\");\n\
             {signature_anchor}return {helper}(this, arguments, function* ({originals}) {{{body}{body_close}{completion});\n{completion}}}"
        );
        let source = if external {
            String::new()
        } else {
            HELPER
                .replace("__blueice_target_async_generator", &helper)
                .replace("__blueice_target_generator_suspension", &marker)
        };
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\n{source}\n"),
                },
                TextEdit {
                    start: tokens[start].start,
                    end: tokens[function.body_end].end,
                    replacement,
                },
            ],
        );
    }
}

fn rewrite(
    source: &str,
    begin: usize,
    finish: usize,
    marker: (&str, bool),
    depth: usize,
    context: (&Module, &crate::parser::ParserLimits),
) -> Option<String> {
    let (marker, external) = marker;
    if depth == 0 {
        return None;
    }
    let tokens = crate::syntax::lex_with_limits(
        &context.0.id,
        &source[begin..finish],
        context.1.max_source_bytes,
        context.1.max_tokens,
    )
    .ok()?;
    let end = tokens.len().checked_sub(1)?;
    let mut edits = Vec::new();
    let mut index = 0;
    while index < end {
        let token = &tokens[index];
        if token.kind == TokenKind::Template {
            let replacement = mapped_fragments::template(
                source,
                begin + token.start,
                begin + token.end,
                context.0,
                depth - 1,
                |start, end, depth| {
                    rewrite(source, start, end, (marker, external), depth, context).ok_or_else(
                        || {
                            Diagnostic::error(
                                DiagnosticCode::UnsupportedSyntax,
                                SourceSpan::new(&context.0.id, start, end),
                                "template generator suspension was not retained",
                            )
                        },
                    )
                },
            )
            .ok()?;
            edits.push(TextEdit {
                start: begin + token.start,
                end: begin + token.end,
                replacement,
            });
            index += 1;
            continue;
        }
        if token.is("class") || token.is("async") && !tokens.get(index + 1)?.is("function") {
            return None;
        }
        if token.is("=>") && tokens.get(index + 1)?.is("{") {
            index = exponentiation::matching_close(&tokens, index + 1)? + 1;
            continue;
        }
        if token.is("(") && index > 0 && tokens[index - 1].kind == TokenKind::Identifier {
            let close = exponentiation::matching_close(&tokens, index)?;
            if tokens.get(close + 1)?.is("{") {
                return None;
            }
        }
        if token.is("function") {
            let parameters = (index + 1..end).find(|&position| tokens[position].is("("))?;
            let nested = async_functions::function_at_parameters(&tokens, parameters)?;
            index = nested.body_end + 1;
            continue;
        }
        if !matches!(token.text.as_str(), "await" | "yield" | "return")
            || index > 0 && matches!(tokens[index - 1].text.as_str(), "." | "?.")
            || tokens.get(index + 1).is_some_and(|token| token.is(":"))
        {
            index += 1;
            continue;
        }
        if tokens.get(index + 1)?.is("*") {
            return None;
        }
        let following = &tokens[index + 1];
        let line_break = source[begin + token.end..begin + following.start]
            .contains(['\n', '\r', '\u{2028}', '\u{2029}']);
        let operand_end = if token.is("await") {
            exponentiation::operand_end(&tokens, index + 1)?
        } else if line_break || matches!(following.text.as_str(), ";" | ")" | "]" | "}" | ",") {
            index + 1
        } else {
            assignment_end(&tokens, index + 1).min(end)
        };
        if operand_end > end || tokens.get(operand_end).is_some_and(|token| token.is("{")) {
            return None;
        }
        if operand_end == index + 1 && token.is("return") {
            index += 1;
            continue;
        }
        let (operand, last) = if operand_end == index + 1 {
            ("void 0".to_string(), begin + token.end)
        } else {
            let last = begin + tokens[operand_end - 1].end;
            (
                rewrite(
                    source,
                    begin + following.start,
                    last,
                    (marker, external),
                    depth - 1,
                    context,
                )?,
                last,
            )
        };
        let start = relocations::source_anchor(begin + token.start);
        let close = relocations::source_anchor(last);
        let generated = relocations::unmapped();
        let prefix = if token.is("return") {
            format!("{start}return ")
        } else {
            String::new()
        };
        let awaiting = !token.is("yield");
        let replacement = if external {
            if awaiting {
                format!(
                    "{prefix}{generated}({start}yield new {marker}({operand}{close}){generated})"
                )
            } else {
                format!("{generated}({start}yield {generated}({start}yield new {marker}({operand}{close}){generated}){generated})")
            }
        } else {
            format!("{prefix}{generated}({start}yield {marker}({awaiting}, {generated}({operand}{close} {generated}){close}){generated})")
        };
        edits.push(TextEdit {
            start: begin + token.start,
            end: last,
            replacement,
        });
        index = operand_end;
    }
    Some(mapped_fragments::splice(begin, finish, edits))
}
