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
            Some((index, function))
        });
        let Some((start, function)) = candidate else {
            return Ok(emitted);
        };
        let body = resumed_body(
            &emitted.javascript,
            &tokens,
            &function,
            module,
            &options.limits.parser,
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
        let originals = &emitted.javascript[tokens[function.parameters].end..originals_end];
        let name = &tokens[start + 2].text;
        let replacement = format!(
            "function {name}({parameters}) {{\n\
             if (new.target) throw new TypeError(\"Async function is not a constructor\");\n\
             return {helper}(this, arguments, {}function* ({originals}) {{{body}}});\n}}",
            if external { "void 0, " } else { "" }
        );
        let insertion = directive_end(&tokens);
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

fn resumed_body(
    source: &str,
    tokens: &[Token],
    function: &Function,
    module: &Module,
    limits: &crate::parser::ParserLimits,
) -> Result<String, Diagnostic> {
    resumed_text(
        &source[tokens[function.body].end..tokens[function.body_end].start],
        module,
        limits,
        limits.max_type_depth.min(128),
    )
}
fn resumed_text(
    source: &str,
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
        source,
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
            let replacement = super::suspension_text::template(
                source,
                &tokens[at],
                module,
                depth - 1,
                |text, depth| resumed_text(text, module, limits, depth),
            )?;
            edits.push(TextEdit {
                start: tokens[at].start,
                end: tokens[at].end,
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
                &source[tokens[at + 1].start..tokens[end - 1].end],
                module,
                limits,
                depth - 1,
            )?;
            edits.push(TextEdit {
                start: tokens[at].start,
                end: tokens[end - 1].end,
                replacement: format!("(yield ({operand}))"),
            });
            at = end;
        } else {
            at += 1;
        }
    }
    Ok(apply_edits(source, edits).javascript)
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
