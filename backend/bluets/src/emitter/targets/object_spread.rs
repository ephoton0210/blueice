// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Object spread in assigned literals below ES2018. Nested calls preserve the
//! order of source reads and subsequent literal property evaluation.

use super::*;

pub(super) const HELPER: &str = include_str!("object_spread_helpers.v1.js");

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target >= crate::EcmaTarget::Es2018 {
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
        let Some((open, close, properties)) = assigned_literal(&tokens) else {
            return Ok(emitted);
        };
        let helper = unused_name(&emitted.javascript, "object_properties", &mut sequence);
        let anchor_offset = open
            .checked_sub(2)
            .and_then(|index| tokens.get(index))
            .map_or(tokens[open - 1].start, |token| token.end);
        let source_anchor = super::super::relocations::source_anchor(anchor_offset);
        let unmapped = super::super::relocations::unmapped();
        let mut replacement = format!("{source_anchor}{{{source_anchor}}}");
        let mut previous_end = anchor_offset;
        for (start, end) in properties {
            let spread = tokens[start].is("...");
            let first = start + usize::from(spread);
            if first == end {
                return Err(unsupported(
                    module,
                    "target object spread requires an operand",
                ));
            }
            if !spread && unsupported_property(&tokens[first..end]) {
                return Err(unsupported(
                    module,
                    "target object spread does not yet lower prototype setters or super properties",
                ));
            }
            let source = super::super::relocations::relocated_verbatim(
                tokens[first].start,
                tokens[end - 1].end,
            );
            let previous_end_anchor = super::super::relocations::source_anchor(previous_end);
            let end_offset = tokens[end - 1].end;
            let end_anchor = super::super::relocations::source_anchor(end_offset);
            let argument = if spread {
                if first + 1 == end && tokens[first].kind == TokenKind::Identifier {
                    source
                } else {
                    format!("{unmapped}({source}{unmapped})")
                }
            } else {
                format!("{previous_end_anchor}{{{source}{end_anchor}}}")
            };
            replacement = format!(
                "{source_anchor}{helper}{source_anchor}({replacement}{previous_end_anchor}, {argument}{unmapped}, {spread}{end_anchor})"
            );
            previous_end = end_offset;
        }
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: directive_end(&tokens),
                    end: directive_end(&tokens),
                    replacement: format!(
                        "{unmapped}\n{}\n",
                        HELPER.replace("__blueice_target_object_properties", &helper)
                    ),
                },
                TextEdit {
                    start: tokens[open].start,
                    end: tokens[close].end,
                    replacement,
                },
            ],
        );
    }
}

type Literal = (usize, usize, Vec<(usize, usize)>);

fn assigned_literal(tokens: &[Token]) -> Option<Literal> {
    // An assignment/default initializer distinguishes a literal from object
    // binding patterns and statement/function bodies. Other contexts retain
    // their current target boundary until separately measured.
    for open in (1..tokens.len()).rev() {
        if !tokens[open].is("{") || !tokens[open - 1].is("=") {
            continue;
        }
        let mut depth = 0usize;
        let mut start = open + 1;
        let mut properties = Vec::new();
        for index in open + 1..tokens.len() {
            match tokens[index].text.as_str() {
                "}" if depth == 0 => {
                    if start < index {
                        properties.push((start, index));
                    }
                    if properties.iter().any(|&(first, _)| tokens[first].is("...")) {
                        return Some((open, index, group_properties(tokens, properties)));
                    }
                    break;
                }
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.checked_sub(1)?,
                "," if depth == 0 => {
                    if start < index {
                        properties.push((start, index));
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    None
}

fn group_properties(tokens: &[Token], properties: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (start, end) in properties {
        if !tokens[start].is("...") {
            if let Some(previous) = groups.last_mut().filter(|group| !tokens[group.0].is("...")) {
                previous.1 = end;
                continue;
            }
        }
        groups.push((start, end));
    }
    groups
}

fn unsupported_property(tokens: &[Token]) -> bool {
    tokens.iter().any(|token| token.is("super"))
        || tokens.windows(2).any(|pair| {
            pair[1].is(":")
                && (pair[0].is("__proto__")
                    || pair[0].kind == TokenKind::String
                        && crate::enum_eval::decode_plain_string(&pair[0].text).as_deref()
                            == Some("__proto__"))
        })
}

fn unsupported(module: &Module, message: &str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        SourceSpan::new(&module.id, 0, 0),
        message,
    )
}
