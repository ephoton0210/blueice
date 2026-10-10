// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named async functions on ES2015/ES2016 use the native generator protocol.
//! Generator parameters execute inside the promise boundary, including defaults.

use super::*;

const HELPER: &str = include_str!("async_helpers.v1.js");

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target < crate::EcmaTarget::Es2015 || options.target >= crate::EcmaTarget::Es2017 {
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
        let candidate = (0..tokens.len()).rev().find_map(|index| {
            if !tokens[index].is("async") || !tokens.get(index + 1)?.is("function") {
                return None;
            }
            let function = named_function(&tokens, index + 1)?;
            let body = resumed_body(&emitted.javascript, &tokens, &function)?;
            Some((index, function, body))
        });
        let Some((start, function, body)) = candidate else {
            return Ok(emitted);
        };
        let helper = unused_name(&emitted.javascript, "async", &mut sequence);
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
        let originals = &emitted.javascript[tokens[function.parameters].end..originals_end];
        let name = &tokens[start + 2].text;
        let replacement = format!(
            "function {name}({parameters}) {{\n\
             if (new.target) throw new TypeError(\"Async function is not a constructor\");\n\
             return {helper}(this, arguments, function* ({originals}) {{{body}}});\n}}"
        );
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!(
                        "\n{}\n",
                        HELPER.replace("__blueice_target_async", &helper)
                    ),
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

pub(super) struct Function {
    pub(super) parameters: usize,
    pub(super) parameters_end: usize,
    pub(super) body: usize,
    pub(super) body_end: usize,
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

fn resumed_body(source: &str, tokens: &[Token], function: &Function) -> Option<String> {
    let mut edits = Vec::new();
    let begin = tokens[function.body].end;
    let mut index = function.body + 1;
    while index < function.body_end {
        if tokens[index].is("function") {
            // Nested functions own their awaits/arguments. Innermost named
            // async declarations have already been considered by the driver.
            let parameters =
                (index + 1..function.body_end).find(|&position| tokens[position].is("("))?;
            let nested = function_at_parameters(tokens, parameters)?;
            index = nested.body_end + 1;
            continue;
        }
        if tokens[index].is("async") && !tokens.get(index + 1)?.is("function") {
            return None;
        }
        if tokens[index].is("await") {
            if index > 0 && matches!(tokens[index - 1].text.as_str(), "." | "?.")
                || tokens.get(index + 1).is_some_and(|token| token.is(":"))
            {
                index += 1;
                continue;
            }
            if index > 0 && tokens[index - 1].is("for") {
                return None;
            }
            let end = exponentiation::operand_end(tokens, index + 1)?;
            if end > function.body_end {
                return None;
            }
            let operand = &source[tokens[index + 1].start..tokens[end - 1].end];
            // Nested awaits retain their current target boundary.
            if tokens[index + 1..end].iter().any(|token| token.is("await")) {
                return None;
            }
            edits.push(TextEdit {
                start: tokens[index].start - begin,
                end: tokens[end - 1].end - begin,
                replacement: format!("(yield {operand})"),
            });
            index = end;
        } else {
            index += 1;
        }
    }
    Some(apply_edits(&source[begin..tokens[function.body_end].start], edits).javascript)
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
