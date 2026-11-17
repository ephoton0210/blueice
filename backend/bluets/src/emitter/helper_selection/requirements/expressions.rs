// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inspect retained source tokens without running a transform or printer.
use super::*;

impl Requirements<'_> {
    pub(super) fn expressions(&mut self) {
        let tokens = self.tokens.clone();
        let erased = merge_ranges(
            self.module
                .edits
                .iter()
                .filter(|edit| edit.replacement.is_empty())
                .map(|edit| (edit.start, edit.end))
                .collect(),
        );
        let bindings = merge_ranges(self.binding_ranges.clone());
        let mut containers = Vec::<&Token>::new();
        for (index, token) in tokens.iter().enumerate() {
            // Erased type syntax cannot create a runtime helper requirement.
            if contains(&erased, token) {
                continue;
            }
            if token.is("...") && self.options.target < crate::EcmaTarget::Es2018 {
                let binding = contains(&bindings, token);
                if !binding {
                    let helper = match containers.last().map(|token| token.text.as_str()) {
                        Some("[") if self.options.target < crate::EcmaTarget::Es2015 => {
                            Some("__spreadArray")
                        }
                        Some("{") => Some("__assign"),
                        _ => None,
                    };
                    if let Some(helper) = helper {
                        let end = operand_end(&tokens, index + 1);
                        let span = SourceSpan::new(&self.module.id, token.start, end);
                        self.add(helper, span.clone());
                        if helper == "__spreadArray" && self.options.downlevel_iteration {
                            self.add("__read", span);
                        }
                    }
                }
            }
            if self.options.target < crate::EcmaTarget::Es2015
                && token.kind == TokenKind::Template
                && index > 0
                && (tokens[index - 1].is_binding_identifier()
                    || tokens[index - 1].is(")")
                    || tokens[index - 1].is("]"))
            {
                self.add(
                    "__makeTemplateObject",
                    SourceSpan::new(&self.module.id, tokens[index - 1].start, token.end),
                );
            }
            if token.is("yield")
                && tokens.get(index + 1).is_some_and(|token| token.is("*"))
                && self.options.target < crate::EcmaTarget::Es2015
            {
                self.add("__values", token.span(&self.module.id));
            }
            if token.is("for")
                && self.options.target < crate::EcmaTarget::Es2015
                && self.options.downlevel_iteration
            {
                let end = tokens[index + 1..]
                    .iter()
                    .position(|token| token.is(")"))
                    .map(|end| index + 1 + end);
                if end.is_some_and(|end| tokens[index + 1..end].iter().any(|token| token.is("of")))
                {
                    self.add("__values", token.span(&self.module.id));
                }
            }
            // Private access may still need helpers when class-field assignment
            // semantics are selected, including the Native static-only path.
            if token.is("#") || token.text.starts_with('#') {
                let end = if token.is("#") {
                    tokens.get(index + 1).map_or(token.end, |token| token.end)
                } else {
                    token.end
                };
                let after = index + usize::from(token.is("#")) + 1;
                if self.options.target < crate::EcmaTarget::EsNext
                    || !self.options.defines_class_fields()
                {
                    if index >= 2 && tokens[index - 1].is(".") {
                        let span = SourceSpan::new(&self.module.id, tokens[index - 2].start, end);
                        let assignment = tokens.get(after).is_some_and(|token| {
                            matches!(
                                token.text.as_str(),
                                "=" | "+=" | "-=" | "*=" | "/=" | "++" | "--"
                            )
                        });
                        if assignment {
                            self.add("__classPrivateFieldSet", span.clone());
                        }
                        if !assignment || !tokens.get(after).is_some_and(|token| token.is("=")) {
                            self.add("__classPrivateFieldGet", span);
                        }
                    } else if tokens.get(after).is_some_and(|token| token.is("in")) {
                        self.add(
                            "__classPrivateFieldIn",
                            SourceSpan::new(&self.module.id, token.start, end),
                        );
                    }
                }
            }
            match token.text.as_str() {
                "(" | "[" | "{" => containers.push(token),
                ")" | "]" | "}" => {
                    containers.pop();
                }
                _ => {}
            }
        }
    }
}

fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(previous) = merged.last_mut().filter(|previous| previous.1 >= start) {
            previous.1 = previous.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn contains(ranges: &[(usize, usize)], token: &Token) -> bool {
    let end = ranges.partition_point(|(start, _)| *start <= token.start);
    end > 0 && token.end <= ranges[end - 1].1
}

fn operand_end(tokens: &[Token], start: usize) -> usize {
    let mut depth = 0usize;
    let mut end = tokens.get(start).map_or(0, |token| token.end);
    for token in &tokens[start..] {
        if depth == 0 && matches!(token.text.as_str(), "," | "]" | "}" | ")" | ";") {
            break;
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
        end = token.end;
    }
    end
}
