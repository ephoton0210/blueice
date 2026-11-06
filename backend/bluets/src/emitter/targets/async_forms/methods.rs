// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Method wrappers copy their parameter and body subtrees before target lowering.
use super::*;
use crate::emitter::relocations;
mod origins;

pub(super) fn lower(
    emitted: EmittedJavaScript,
    tokens: &[Token],
    (start, open, close, body, end): (usize, usize, usize, usize, usize),
    module: &Module,
    options: &CompilerOptions,
    sequence: &mut usize,
    name: &str,
) -> Result<EmittedJavaScript, Diagnostic> {
    let source = &emitted.javascript;
    let count = async_functions::parameter_length(&tokens[open + 1..close]);
    let external = (0..count)
        .map(|_| unused_name(source, "async_parameter", sequence))
        .collect::<Vec<_>>()
        .join(", ");
    let parameters = relocations::relocated_verbatim(tokens[open].end, tokens[close].start);
    let body_start = tokens[body].end;
    let body_end = tokens[end - 1].start;
    let (body, bridge) = if options.target == crate::EcmaTarget::Es5 {
        (
            relocations::relocated_verbatim(body_start, body_end),
            String::new(),
        )
    } else {
        super_body(source, body_start, body_end, module, sequence)?
    };
    let property = relocations::relocated_verbatim(tokens[start + 1].start, tokens[open].start);
    let anchor = relocations::source_anchor;
    let generated = relocations::unmapped();
    let is_class = super::class_member(tokens, start);
    let head = if options.target == crate::EcmaTarget::Es5 && !is_class {
        format!(
            "{property}{}: function{}({generated}{external}{})",
            anchor(tokens[open].start),
            anchor(tokens[open].start),
            anchor(tokens[close].start),
        )
    } else {
        format!(
            "{property}{}({generated}{external}{})",
            anchor(tokens[open].start),
            anchor(tokens[close].start),
        )
    };
    let body_close = relocations::relocated_verbatim(tokens[end - 1].start, tokens[end - 1].end);
    let completion = format!("{} ", anchor(tokens[end - 1].end));
    let callback_head = format!(
        "{head}{}{{{generated}{bridge}return (async function {name}",
        anchor(tokens[close].start),
    );
    let prefix = format!(
        "{callback_head}{}({parameters}{}){generated}{{{body}{body_close}",
        anchor(tokens[open].start),
        anchor(tokens[close].start),
    );
    let signature_origin = (is_class && options.target == crate::EcmaTarget::Es5)
        .then(|| {
            async_functions::class_method_at(&emitted, module, tokens[start + 1].start).map(
                |method| {
                    origins::Point::new(
                        source,
                        &callback_head,
                        tokens[start].start,
                        method.span.start,
                    )
                },
            )
        })
        .flatten();
    let empty_signature_origin =
        (is_class && options.target == crate::EcmaTarget::Es5 && open + 1 == close)
            .then(|| {
                async_functions::class_method_at(&emitted, module, tokens[start + 1].start).map(
                    |method| {
                        origins::Point::new(
                            source,
                            &format!("{callback_head}("),
                            tokens[start].start,
                            method.span.start,
                        )
                    },
                )
            })
            .flatten();
    let origin = async_functions::method_end_at(&emitted, module, tokens[end - 1].start)
        .map(|end| origins::Point::new(source, &prefix, tokens[start].start, end));
    let mut next = mapped_edits(
        emitted,
        vec![TextEdit {
            start: tokens[start].start,
            end: tokens[end - 1].end,
            replacement: format!("{prefix}{completion}{generated}).apply(this,arguments);}}"),
        }],
    );
    if let Some(origin) = origin {
        origin.apply(&mut next, &module.source);
    }
    if let Some(origin) = signature_origin {
        origin.apply(&mut next, &module.source);
    }
    if let Some(origin) = empty_signature_origin {
        origin.apply(&mut next, &module.source);
    }
    Ok(next)
}

fn super_body(
    source: &str,
    body_start: usize,
    body_end: usize,
    module: &Module,
    sequence: &mut usize,
) -> Result<(String, String), Diagnostic> {
    let body = &source[body_start..body_end];
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
                relocations::relocated_verbatim(
                    body_start + tokens[at + 1].end,
                    body_start + tokens[end].start,
                ),
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
        let named = tokens[start + 1].is(".");
        let anchor = |offset| relocations::source_anchor(body_start + offset);
        let generated = relocations::unmapped();
        let mut replacement = if named {
            format!(
                "{getter}({}{key}{generated}){}",
                anchor(tokens[start + 2].start),
                anchor(tokens[end - 1].end),
            )
        } else {
            format!("{getter}({key})")
        };
        let mut finish = tokens[end - 1].end;
        if tokens.get(end).is_some_and(|t| t.is("(")) {
            replacement.push_str(".call(this");
            if !tokens.get(end + 1).is_some_and(|t| t.is(")")) {
                replacement.push_str(", ");
            }
            finish = tokens[end].end;
            if named {
                let close = exponentiation::matching_close(&tokens, end).ok_or_else(|| {
                    Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        SourceSpan::new(&module.id, body_start, body_end),
                        "async super invocation closing was not retained",
                    )
                })?;
                // The generated .call closes on its last argument phase. A
                // carrier prevents the following copied token from replacing
                // that logical ending at the same generated position.
                edits.push(TextEdit {
                    start: body_start + tokens[close].start,
                    end: body_start + tokens[close].end,
                    replacement: format!(
                        "{}{}{} ",
                        anchor(tokens[close].start),
                        relocations::relocated_verbatim(
                            body_start + tokens[close].start,
                            body_start + tokens[close].end,
                        ),
                        anchor(tokens[close].start),
                    ),
                });
                if let Some(operator) = tokens.get(close + 1).filter(|token| {
                    matches!(token.text.as_str(), "+" | "-" | "*" | "/" | "%" | "**")
                }) {
                    edits.push(TextEdit {
                        start: body_start + operator.start,
                        end: body_start + operator.end,
                        replacement: format!("{}{}", anchor(tokens[close].start), operator.text),
                    });
                }
            }
        } else if named {
            replacement.push(' ');
        }
        edits.push(TextEdit {
            start: body_start + tokens[start].start,
            end: body_start + finish,
            replacement: format!(
                "{}{replacement}",
                relocations::source_anchor(body_start + tokens[start].start)
            ),
        });
        at = end;
    }
    let bridge = if edits.is_empty() {
        String::new()
    } else {
        format!("{}var {getter}=(key)=>super[key];", relocations::unmapped())
    };
    Ok((
        mapped_fragments::splice(body_start, body_end, edits),
        bridge,
    ))
}
