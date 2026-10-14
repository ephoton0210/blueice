// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array/call spread preserves receiver evaluation and sparse array entries.

use super::*;

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
        let append = unused_name(&emitted.javascript, "spread_array", &mut sequence);
        let invoke = unused_name(&emitted.javascript, "spread_call", &mut sequence);
        let iterator = unused_name(&emitted.javascript, "spread_iterator", &mut sequence);
        let step = unused_name(&emitted.javascript, "spread_step", &mut sequence);
        let mut array = "[]".to_string();
        for (first, end, spread) in parts {
            let begin = first + usize::from(spread);
            let source = if begin < end {
                &emitted.javascript[tokens[begin].start..tokens[end - 1].end]
            } else {
                ""
            };
            let source = if spread {
                format!("({source})")
            } else {
                let hole = !call && (begin == end || end > begin && tokens[end - 1].is(","));
                format!("[{source}{}]", if hole { "," } else { "" })
            };
            array = format!(
                "{append}({array}, {source}, {spread}, {})",
                options.downlevel_iteration
            );
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
            if tokens[start].is("new") || tokens[start].is("super") {
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
            let replacement = if let Some(receiver_end) = receiver_end.filter(|end| *end > start) {
                let temporary = unused_name(&emitted.javascript, "spread_receiver", &mut sequence);
                let receiver = &emitted.javascript[tokens[start].start..tokens[receiver_end].start];
                let member = &emitted.javascript[tokens[receiver_end].start..tokens[open].start];
                let insertion = directive_end(&tokens);
                edits.push(TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\nvar {temporary};\n"),
                });
                format!("({temporary} = ({receiver}), {invoke}({temporary}{member}, {temporary}, {array}))")
            } else {
                let callee = &emitted.javascript[tokens[start].start..tokens[open].start];
                format!("{invoke}(({callee}), void 0, {array})")
            };
            (tokens[start].start, replacement)
        } else {
            (tokens[open].start, array)
        };
        let helper = include_str!("../spread_helpers.v1.js")
            .replace("__blueice_target_spread_array", &append)
            .replace("__blueice_target_spread_call", &invoke)
            .replace("__blueice_target_iterator", &iterator)
            .replace("__blueice_target_step_result", &step);
        let iterator_helper = include_str!("../iteration_helpers.v1.js")
            .replace("__blueice_target_iterator", &iterator)
            .replace("__blueice_target_step_result", &step);
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
        emitted = mapped_edits(emitted, edits);
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
