// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async arrows retain lexical captures; methods retain their home-object super.
use super::*;
mod capture;
mod methods;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target >= crate::EcmaTarget::Es2017 {
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
        .map_err(|mut d| d.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let candidate = (0..tokens.len()).rev().find_map(|start| {
            if !tokens[start].is("async") || tokens.get(start + 1)?.is("function") {
                return None;
            }
            let head = start + 1;
            let (open, close) = if tokens[head].is("(") {
                (head, exponentiation::matching_close(&tokens, head)?)
            } else {
                (head, head)
            };
            if tokens.get(close + 1)?.is("=>") {
                let body = close + 2;
                let end = if tokens[body].is("{") {
                    exponentiation::matching_close(&tokens, body)? + 1
                } else {
                    assignment_end(&tokens, body)
                };
                return Some((start, open, close, body, end, true));
            }
            let open = if tokens[head].is("[") {
                exponentiation::matching_close(&tokens, head)? + 1
            } else {
                head + 1
            };
            if !tokens.get(open)?.is("(") {
                return None;
            }
            let function = async_functions::function_at_parameters(&tokens, open)?;
            Some((
                start,
                open,
                function.parameters_end,
                function.body,
                function.body_end + 1,
                false,
            ))
        });
        let Some((start, open, close, body, end, arrow)) = candidate else {
            return Ok(emitted);
        };
        let source = &emitted.javascript;
        let name = unused_name(source, "async_callable", &mut sequence);
        if !arrow {
            emitted = methods::lower(
                emitted,
                &tokens,
                (start, open, close, body, end),
                module,
                options,
                &mut sequence,
                &name,
            )?;
            continue;
        }
        let replacement = {
            let receiver = unused_name(source, "async_this", &mut sequence);
            let arguments = unused_name(source, "async_arguments", &mut sequence);
            let captures = |start: usize, end: usize| {
                capture::range(
                    source,
                    start..end,
                    module,
                    (&receiver, &arguments),
                    options.limits.parser.max_type_depth,
                    &options.limits.parser,
                )
            };
            let parameters = if open == close {
                captures(tokens[open].start, tokens[close].end)?
            } else {
                captures(tokens[open].end, tokens[close].start)?
            };
            let captured = if tokens[body].is("{") {
                captures(tokens[body].end, tokens[end - 1].start)?
            } else {
                captures(tokens[body].start, tokens[end - 1].end)?
            };
            let body_text = if tokens[body].is("{") {
                captured
            } else {
                format!("return ({captured});")
            };
            let anchor = crate::emitter::relocations::source_anchor;
            let generated = crate::emitter::relocations::unmapped();
            let close_body = if tokens[body].is("{") {
                format!(
                    "{}{} ",
                    crate::emitter::relocations::relocated_verbatim(
                        tokens[end - 1].start,
                        tokens[end - 1].end,
                    ),
                    anchor(tokens[end - 1].end)
                )
            } else {
                format!(
                    "{}}}{} ",
                    anchor(tokens[end - 1].end),
                    anchor(tokens[end - 1].end)
                )
            };
            let operation = if options.target == crate::EcmaTarget::Es5 {
                if async_functions::plain_arrow_formals(&emitted, module, tokens[start].start) {
                    tokens[close].start
                } else {
                    tokens[start].start
                }
            } else {
                tokens[close + 1].start
            };
            format!("{generated}(function({receiver},{arguments}){{return {}async function {generated}{name}{}({parameters}{}) {}{{{} {body_text}{close_body}{generated};}})(this,typeof arguments==='undefined'?void 0:arguments)", anchor(tokens[start].start), anchor(tokens[open].start), anchor(tokens[close].start), anchor(operation), anchor(tokens[close + 1].end))
        };
        emitted = mapped_edits(
            emitted,
            vec![TextEdit {
                start: tokens[start].start,
                end: tokens[end - 1].end,
                replacement,
            }],
        );
    }
}

fn class_member(tokens: &[Token], position: usize) -> bool {
    for start in (0..position).rev().filter(|&i| tokens[i].is("class")) {
        let Some(open) = (start + 1..position).find(|&i| tokens[i].is("{")) else {
            continue;
        };
        let Some(end) = exponentiation::matching_close(tokens, open) else {
            continue;
        };
        if position >= end {
            continue;
        }
        let mut at = open + 1;
        while at < position {
            if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
                let Some(end) = exponentiation::matching_close(tokens, at) else {
                    return false;
                };
                if position <= end {
                    return false;
                }
                at = end;
            }
            at += 1;
        }
        return true;
    }
    false
}
