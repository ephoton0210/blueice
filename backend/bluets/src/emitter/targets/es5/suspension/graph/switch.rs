// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Switch tests run once, case tests retain order, and arms retain fallthrough.
use super::*;

impl Builder<'_> {
    pub(super) fn switch(&mut self, tokens: &[Token], start: usize, next: usize) -> Option<usize> {
        let close = exponentiation::matching_close(tokens, start + 1)?;
        let end = exponentiation::matching_close(tokens, close + 1)?;
        let mut arms = Vec::new();
        let mut cursor = close + 2;
        while cursor < end {
            let default = tokens[cursor].is("default");
            if !default && !tokens[cursor].is("case") {
                return None;
            }
            let first = cursor + 1;
            let mut conditional = 0;
            cursor = first;
            while cursor < end {
                match tokens[cursor].text.as_str() {
                    "?" => conditional += 1,
                    ":" if conditional == 0 => break,
                    ":" => conditional -= 1,
                    "(" | "[" | "{" => cursor = exponentiation::matching_close(tokens, cursor)?,
                    _ => {}
                }
                cursor += 1;
            }
            let test = (!default).then_some(&tokens[first..cursor]);
            let punctuation_end = tokens[cursor].end;
            let body = cursor + 1;
            cursor = body;
            while cursor < end {
                if tokens[cursor].is("case") || tokens[cursor].is("default") {
                    break;
                }
                if matches!(tokens[cursor].text.as_str(), "(" | "[" | "{") {
                    cursor = exponentiation::matching_close(tokens, cursor)?;
                }
                cursor += 1;
            }
            arms.push((test, punctuation_end, body, cursor));
        }
        self.loops.push(JumpTargets {
            broken: (next, self.regions),
            continued: self.loops.last().and_then(|targets| targets.continued),
        });
        let selector = self.temporary();
        let selector_tokens = &tokens[start + 2..close];
        self.binding_initializers.insert(
            selector.clone(),
            origins::InitializerOrigin {
                start: selector_tokens.first()?.start,
                end: selector_tokens.last()?.end,
                statement: false,
            },
        );
        let mut default = next;
        let mut fallthrough = next;
        let mut tests = Vec::new();
        for (test, punctuation_end, begin, end) in arms.into_iter().rev() {
            let items = if begin == end {
                Vec::new()
            } else {
                crate::parser::parse_emitted_statement(
                    self.module,
                    tokens.to_vec(),
                    begin,
                    end,
                    self.depth,
                )?
            };
            let entry = self.sequence(&items, fallthrough)?;
            fallthrough = entry;
            if let Some(test) = test {
                tests.push((test, punctuation_end, entry));
            } else {
                default = entry;
            }
        }
        self.loops.pop();
        // Keeping a synchronous case test in a switch preserves its original
        // punctuation and ordered selection without extra continuation states.
        if !self.external_abi
            && !tests.is_empty()
            && tests.iter().all(|(test, _, _)| !self.suspends(test))
        {
            let mut dispatch = format!("switch ({selector}) {{\n");
            for (test, punctuation_end, matched) in tests.iter().rev() {
                let value = self.text(test)?;
                let start = relocations::source_anchor(self.source_offset + test.first()?.start);
                let end = relocations::source_anchor(self.source_offset + test.last()?.end);
                let punctuation = relocations::source_anchor(self.source_offset + punctuation_end);
                dispatch.push_str(&format!(
                    "{}case {value}{end}:{punctuation}\n{}{start}{}.label = {matched};{end} {}continue;\n",
                    relocations::unmapped(),
                    relocations::unmapped(),
                    self.context,
                    relocations::unmapped(),
                ));
            }
            dispatch.push_str(&format!(
                "default: {}.label = {default}; continue;\n}}",
                self.context
            ));
            let entry = self.block(dispatch);
            return self.expression(&tokens[start + 2..close], Some(&selector), "", entry);
        }
        let mut entry = default;
        // Tests were collected in reverse source order. Prepending them here
        // restores source order while default remains the final fallback.
        for (test, punctuation_end, matched) in tests {
            let value = self.temporary();
            self.binding_initializers.insert(
                value.clone(),
                origins::InitializerOrigin {
                    start: test.first()?.start,
                    end: test.last()?.end,
                    statement: false,
                },
            );
            self.case_punctuation.insert(value.clone(), punctuation_end);
            let start = relocations::source_anchor(self.source_offset + test.first()?.start);
            let end = relocations::source_anchor(self.source_offset + test.last()?.end);
            let branch = self.block(format!(
                "{start}{}.label = {selector} === {value} ? {matched} : {entry};{end} {}continue;",
                self.context,
                relocations::unmapped(),
            ));
            entry = self.expression(test, Some(&value), "", branch)?;
        }
        self.expression(&tokens[start + 2..close], Some(&selector), "", entry)
    }
}
