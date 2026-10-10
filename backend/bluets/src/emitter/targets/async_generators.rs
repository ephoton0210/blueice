// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named async generators on ES2015--ES2017 use queued native generators.
//! Original await/yield/return operands are rewritten from retained tokens;
//! nested functions keep their own suspension and completion boundaries.

use super::*;

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
        let helper = unused_name(&emitted.javascript, "async_generator", &mut sequence);
        let marker = unused_name(&emitted.javascript, "generator_suspension", &mut sequence);
        let Some(body) = rewrite(
            &emitted.javascript,
            &tokens,
            function.body + 1..function.body_end,
            tokens[function.body].end..tokens[function.body_end].start,
            &marker,
            options.limits.parser.max_type_depth.min(128),
        ) else {
            // Unmeasured delegation, nested template suspensions and method
            // scopes retain their existing target boundary.
            return Ok(emitted);
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
        let originals = &emitted.javascript[tokens[function.parameters].end..originals_end];
        let name = &tokens[start + 3].text;
        let replacement = format!(
            "function {name}({parameters}) {{\n\
             if (new.target) throw new TypeError(\"Async generator is not a constructor\");\n\
             return {helper}(this, arguments, function* ({originals}) {{{body}}});\n}}"
        );
        let source = HELPER
            .replace("__blueice_target_async_generator", &helper)
            .replace("__blueice_target_generator_suspension", &marker);
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
    tokens: &[Token],
    indices: std::ops::Range<usize>,
    characters: std::ops::Range<usize>,
    marker: &str,
    depth: usize,
) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let std::ops::Range { start, end } = indices;
    let std::ops::Range {
        start: begin,
        end: finish,
    } = characters;
    let mut edits = Vec::new();
    let mut index = start;
    while index < end {
        let token = &tokens[index];
        if token.kind == TokenKind::Template && token.text.contains("${") {
            return None;
        }
        if token.is("class") || token.is("async") && !tokens.get(index + 1)?.is("function") {
            return None;
        }
        if token.is("=>") && tokens.get(index + 1)?.is("{") {
            index = exponentiation::matching_close(tokens, index + 1)? + 1;
            continue;
        }
        if token.is("(") && index > 0 && tokens[index - 1].kind == TokenKind::Identifier {
            let close = exponentiation::matching_close(tokens, index)?;
            if tokens.get(close + 1)?.is("{") {
                return None;
            }
        }
        if token.is("function") {
            let parameters = (index + 1..end).find(|&position| tokens[position].is("("))?;
            let nested = async_functions::function_at_parameters(tokens, parameters)?;
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
        let line_break =
            source[token.end..following.start].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
        let operand_end = if token.is("await") {
            exponentiation::operand_end(tokens, index + 1)?
        } else if line_break || matches!(following.text.as_str(), ";" | ")" | "]" | "}" | ",") {
            index + 1
        } else {
            assignment_end(tokens, index + 1).min(end)
        };
        if operand_end > end {
            return None;
        }
        if tokens.get(operand_end).is_some_and(|token| token.is("{")) {
            return None;
        }
        if operand_end == index + 1 && token.is("return") {
            index += 1;
            continue;
        }
        let (operand, last) = if operand_end == index + 1 {
            ("void 0".to_string(), token.end)
        } else {
            let last = tokens[operand_end - 1].end;
            (
                rewrite(
                    source,
                    tokens,
                    index + 1..operand_end,
                    following.start..last,
                    marker,
                    depth - 1,
                )?,
                last,
            )
        };
        let prefix = if token.is("return") { "return " } else { "" };
        let awaiting = !token.is("yield");
        edits.push(TextEdit {
            start: token.start - begin,
            end: last - begin,
            replacement: format!("{prefix}(yield {marker}({awaiting}, ({operand})))"),
        });
        index = operand_end;
    }
    Some(apply_edits(&source[begin..finish], edits).javascript)
}
