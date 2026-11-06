// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array/call spread preserves receiver evaluation and sparse array entries.

use super::*;
use crate::emitter::relocations;
mod origins;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
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
        let Some((open, close, parts, call)) = candidate(&tokens) else {
            return Ok(emitted);
        };
        let external = crate::emitter::helper_selection::external(options);
        let append = if external {
            crate::emitter::helper_selection::reference(&emitted.javascript, "__spreadArray")
        } else {
            unused_name(&emitted.javascript, "spread_array", &mut sequence)
        };
        let invoke = if external {
            "(function(callee,receiver,values){return Function.prototype.apply.call(callee,receiver,values);})".to_string()
        } else {
            unused_name(&emitted.javascript, "spread_call", &mut sequence)
        };
        let construct = if external {
            "(function(callee,values){var bound=Function.prototype.bind.apply(callee,[null].concat(values));return new bound();})".to_string()
        } else {
            unused_name(&emitted.javascript, "spread_construct", &mut sequence)
        };
        let iterator = unused_name(&emitted.javascript, "spread_iterator", &mut sequence);
        let step = unused_name(&emitted.javascript, "spread_step", &mut sequence);
        let direct = external && call && parts.len() == 1 && parts[0].2;
        let arguments_end = parts.last().map(|(_, end, _)| tokens[end - 1].end);
        let mut construction_end = None;
        let mut array = "[]".to_string();
        for (first, end, spread) in parts {
            let begin = first + usize::from(spread);
            let source = if begin < end {
                relocations::relocated_verbatim(tokens[begin].start, tokens[end - 1].end)
            } else {
                String::new()
            };
            let source = if spread {
                format!(
                    "({source}{})",
                    relocations::source_anchor(tokens[end - 1].end)
                )
            } else {
                let hole = !call && (begin == end || end > begin && tokens[end - 1].is(","));
                format!("[{source}{}]", if hole { "," } else { "" })
            };
            array = if direct {
                if options.downlevel_iteration {
                    format!(
                        "{}({source})",
                        crate::emitter::helper_selection::reference(&emitted.javascript, "__read")
                    )
                } else {
                    source
                }
            } else if external {
                let values = if spread && options.downlevel_iteration {
                    format!(
                        "{}({source})",
                        crate::emitter::helper_selection::reference(&emitted.javascript, "__read")
                    )
                } else {
                    source
                };
                format!(
                    "{append}({array}, {values}, {})",
                    spread && !options.downlevel_iteration
                )
            } else {
                format!(
                    "{append}({array}, {source}, {spread}, {})",
                    options.downlevel_iteration
                )
            };
        }
        let mut edits = Vec::new();
        let (start, replacement) = if call {
            let start = exponentiation::operand_start(&tokens, open).ok_or_else(|| {
                unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "spread callee was not retained",
                )
            })?;
            if tokens[start].is("super") {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "constructor spread requires retained construction semantics",
                ));
            }
            let receiver_end = if open >= 2 && tokens[open - 2].is(".") {
                Some(open - 2)
            } else if tokens[open - 1].is("]") {
                exponentiation::matching_open(&tokens, open - 1)
            } else {
                None
            };
            let replacement = if tokens[start].is("new") {
                construction_end = arguments_end;
                let callee = relocations::relocated_verbatim(tokens[start].end, tokens[open].start);
                format!("{construct}(({callee}), {array})")
            } else if let Some(receiver_end) = receiver_end.filter(|end| *end > start) {
                let temporary = unused_name(&emitted.javascript, "spread_receiver", &mut sequence);
                let receiver = relocations::relocated_verbatim(
                    tokens[start].start,
                    tokens[receiver_end].start,
                );
                let member =
                    relocations::relocated_verbatim(tokens[receiver_end].start, tokens[open].start);
                let insertion = directive_end(&tokens);
                edits.push(TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\nvar {temporary};\n"),
                });
                format!("({temporary} = ({receiver}), {invoke}({temporary}{member}, {temporary}, {array}))")
            } else {
                let callee =
                    relocations::relocated_verbatim(tokens[start].start, tokens[open - 1].end);
                origins::invoke(
                    &invoke,
                    &callee,
                    &array,
                    external,
                    tokens[start].start,
                    tokens[open - 1].end,
                )
            };
            (tokens[start].start, replacement)
        } else {
            (
                tokens[open].start,
                format!(
                    "{}{array}",
                    relocations::source_anchor(tokens[open - 1].start)
                ),
            )
        };
        let helper = if external {
            String::new()
        } else {
            include_str!("../spread_helpers.v1.js")
                .replace("__blueice_target_spread_array", &append)
                .replace("__blueice_target_spread_call", &invoke)
                .replace("__blueice_target_spread_construct", &construct)
                .replace("__blueice_target_iterator", &iterator)
                .replace("__blueice_target_step_result", &step)
        };
        let iterator_helper = if external {
            String::new()
        } else {
            include_str!("../iteration_helpers.v1.js")
                .replace("__blueice_target_iterator", &iterator)
                .replace("__blueice_target_step_result", &step)
        };
        let insertion = directive_end(&tokens);
        edits.push(TextEdit {
            start: insertion,
            end: insertion,
            replacement: format!("\n{helper}\n{iterator_helper}\n"),
        });
        edits.push(TextEdit {
            start,
            end: tokens[close].end,
            replacement,
        });
        emitted = if let Some(operand_end) = construction_end {
            origins::complete_construction(emitted, edits, tokens[close].end, operand_end)
        } else {
            mapped_edits(emitted, edits)
        };
    }
}

type Candidate = (usize, usize, Vec<(usize, usize, bool)>, bool);

fn candidate(tokens: &[Token]) -> Option<Candidate> {
    for open in (1..tokens.len()).rev() {
        let call = tokens[open].is("(")
            && (tokens[open - 1].kind == TokenKind::Identifier
                || matches!(tokens[open - 1].text.as_str(), ")" | "]")
                || open >= 2 && tokens[open - 2].is("."));
        let array = tokens[open].is("[")
            && matches!(
                tokens[open - 1].text.as_str(),
                "=" | "(" | "[" | "," | ":" | "return" | "=>"
            );
        if !(call || array)
            || call && open >= 2 && matches!(tokens[open - 2].text.as_str(), "function" | "*")
        {
            continue;
        }
        let close = exponentiation::matching_close(tokens, open)?;
        let mut parts = Vec::new();
        let mut start = open + 1;
        let mut index = start;
        while index < close {
            if matches!(tokens[index].text.as_str(), "(" | "[" | "{") {
                index = exponentiation::matching_close(tokens, index)? + 1;
                continue;
            }
            if tokens[index].is(",") {
                parts.push((start, index));
                start = index + 1;
            }
            index += 1;
        }
        if start < close {
            parts.push((start, close));
        }
        if !parts
            .iter()
            .any(|&(first, end)| first < end && tokens[first].is("..."))
        {
            continue;
        }
        let mut groups: Vec<(usize, usize, bool)> = Vec::new();
        for (first, end) in parts {
            let spread = first < end && tokens[first].is("...");
            if !spread {
                if let Some(previous) = groups.last_mut().filter(|group| !group.2) {
                    previous.1 = end;
                    continue;
                }
            }
            groups.push((first, end, spread));
        }
        return Some((open, close, groups, call));
    }
    None
}
