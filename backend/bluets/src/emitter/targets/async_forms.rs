// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async arrows retain lexical captures; methods retain their home-object super.
use super::*;

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
        let parameters = if open == close {
            &source[tokens[open].start..tokens[close].end]
        } else {
            &source[tokens[open].end..tokens[close].start]
        };
        let contents = if tokens[body].is("{") {
            &source[tokens[body].end..tokens[end - 1].start]
        } else {
            &source[tokens[body].start..tokens[end - 1].end]
        };
        let replacement = if arrow {
            let receiver = unused_name(source, "async_this", &mut sequence);
            let arguments = unused_name(source, "async_arguments", &mut sequence);
            let captures = |text: &str| {
                capture(
                    text,
                    module,
                    &receiver,
                    &arguments,
                    options.limits.parser.max_type_depth,
                    &options.limits.parser,
                )
            };
            let parameters = captures(parameters)?;
            let captured = captures(contents)?;
            let body_text = if tokens[body].is("{") {
                captured
            } else {
                format!("return ({captured});")
            };
            format!("(function({receiver},{arguments}){{return async function {name}({parameters}){{{body_text}}};}})(this,typeof arguments==='undefined'?void 0:arguments)")
        } else {
            let count = async_functions::parameter_length(&tokens[open + 1..close]);
            let external = (0..count)
                .map(|_| unused_name(source, "async_parameter", &mut sequence))
                .collect::<Vec<_>>()
                .join(", ");
            let (body, bridge) = if options.target == crate::EcmaTarget::Es5 {
                (contents.to_string(), String::new())
            } else {
                super_body(
                    contents,
                    module,
                    source,
                    &mut sequence,
                    options.limits.parser.max_type_depth,
                )?
            };
            let property = &source[tokens[start + 1].start..tokens[open].start];
            let is_class = class_member(&tokens, start);
            let head = if options.target == crate::EcmaTarget::Es5 && !is_class {
                format!("{property}: function({external})")
            } else {
                format!("{property}({external})")
            };
            format!("{head}{{{bridge}return (async function {name}({parameters}){{{body}}}).apply(this,arguments);}}")
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

fn capture(
    source: &str,
    module: &Module,
    receiver: &str,
    arguments: &str,
    depth: usize,
    limits: &crate::parser::ParserLimits,
) -> Result<String, Diagnostic> {
    if depth == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(&module.id, 0, 0),
            "async lexical capture exceeds depth limit",
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
                if let Some(function) = async_functions::function_at_parameters(&tokens, open) {
                    at = function.body_end + 1;
                    continue;
                }
            }
        }
        if tokens[at].is("(")
            && at > 0
            && matches!(
                tokens[at - 1].kind,
                TokenKind::Identifier | TokenKind::Keyword
            )
            && !matches!(
                tokens[at - 1].text.as_str(),
                "if" | "for" | "while" | "switch" | "catch" | "with"
            )
        {
            if let Some(function) = async_functions::function_at_parameters(&tokens, at) {
                at = function.body_end + 1;
                continue;
            }
        }
        if tokens[at].kind == TokenKind::Template {
            let text = super::suspension_text::template(
                source,
                &tokens[at],
                module,
                depth - 1,
                |text, depth| capture(text, module, receiver, arguments, depth, limits),
            )?;
            edits.push(TextEdit {
                start: tokens[at].start,
                end: tokens[at].end,
                replacement: text,
            });
        }
        if matches!(tokens[at].text.as_str(), "this" | "arguments")
            && !at.checked_sub(1).is_some_and(|p| tokens[p].is("."))
            && !tokens.get(at + 1).is_some_and(|p| p.is(":"))
        {
            edits.push(TextEdit {
                start: tokens[at].start,
                end: tokens[at].end,
                replacement: if tokens[at].is("this") {
                    receiver.into()
                } else if at
                    .checked_sub(1)
                    .is_some_and(|p| matches!(tokens[p].text.as_str(), "{" | ","))
                    && tokens
                        .get(at + 1)
                        .is_some_and(|t| matches!(t.text.as_str(), "}" | ","))
                {
                    format!("arguments: {arguments}")
                } else {
                    arguments.into()
                },
            });
        }
        at += 1;
    }
    Ok(apply_edits(source, edits).javascript)
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
fn super_body(
    body: &str,
    module: &Module,
    source: &str,
    sequence: &mut usize,
    _depth: usize,
) -> Result<(String, String), Diagnostic> {
    let tokens = crate::syntax::lex(&module.id, body).map_err(|mut d| d.remove(0))?;
    let getter = unused_name(source, "async_super", sequence);
    let mut edits = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if !tokens[at].is("super") {
            at += 1;
            continue;
        }
        let start = at;
        let (key, end) = if tokens.get(at + 1).is_some_and(|t| t.is(".")) {
            (serde_json::to_string(&tokens[at + 2].text).unwrap(), at + 3)
        } else if tokens.get(at + 1).is_some_and(|t| t.is("[")) {
            let end = exponentiation::matching_close(&tokens, at + 1).ok_or_else(|| {
                Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    SourceSpan::new(&module.id, 0, 0),
                    "async super key was not retained",
                )
            })?;
            (
                body[tokens[at + 1].end..tokens[end].start].to_string(),
                end + 1,
            )
        } else {
            at += 1;
            continue;
        };
        if tokens
            .get(end)
            .is_some_and(|t| matches!(t.text.as_str(), "=" | "+=" | "-=" | "++" | "--"))
        {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                "async super writes require a retained setter completion",
            ));
        }
        let mut replacement = format!("{getter}({key})");
        let mut finish = tokens[end - 1].end;
        if tokens.get(end).is_some_and(|t| t.is("(")) {
            replacement.push_str(".call(this");
            if !tokens.get(end + 1).is_some_and(|t| t.is(")")) {
                replacement.push_str(", ");
            }
            finish = tokens[end].end;
        }
        edits.push(TextEdit {
            start: tokens[start].start,
            end: finish,
            replacement,
        });
        at = end;
    }
    let bridge = if edits.is_empty() {
        String::new()
    } else {
        format!("var {getter}=(key)=>super[key];")
    };
    Ok((apply_edits(body, edits).javascript, bridge))
}
