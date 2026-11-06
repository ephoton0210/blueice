// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! User bindings and abrupt statements retain origins across continuation edges.
use super::*;

pub(super) struct InitializerOrigin {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) statement: bool,
}

pub(super) struct PredicateOrigin {
    start: usize,
    end: usize,
}

impl Builder<'_> {
    pub(super) fn predicate_origin(&mut self, name: &str, start: usize, tokens: &[Token]) {
        if self.marker.is_none() && !self.suspends(tokens) {
            if let Some(last) = tokens.last() {
                self.predicates.insert(
                    name.to_string(),
                    PredicateOrigin {
                        start,
                        end: last.end,
                    },
                );
            }
        }
    }

    pub(super) fn assignment_terminator(&self, binding: Option<&str>) -> String {
        let end = binding.and_then(|name| {
            self.predicates
                .get(name)
                .map(|origin| origin.end)
                .or_else(|| self.binding_initializers.get(name).map(|origin| origin.end))
        });
        let anchor = end.map_or_else(String::new, |end| {
            relocations::source_anchor(self.source_offset + end)
        });
        let punctuation = binding
            .and_then(|name| self.case_punctuation.get(name))
            .map_or_else(String::new, |end| {
                relocations::source_anchor(self.source_offset + end)
            });
        format!("{anchor};{punctuation} {}", relocations::unmapped())
    }

    pub(super) fn initializer_terminator(&self, binding: Option<&str>, tokens: &[Token]) -> String {
        let whole_initializer = binding
            .and_then(|name| self.binding_initializers.get(name))
            .is_some_and(|origin| {
                origin.statement
                    && tokens
                        .first()
                        .is_some_and(|token| token.start == origin.start)
                    && tokens.last().is_some_and(|token| token.end == origin.end)
            });
        // A split initializer's synthetic stores end at the expression, while
        // an intact initializer also retains the original statement endpoint.
        if whole_initializer {
            self.resume_terminator(tokens, true)
        } else {
            self.assignment_terminator(binding)
        }
    }

    pub(super) fn resume_terminator(&self, tokens: &[Token], statement: bool) -> String {
        let end = tokens
            .last()
            .expect("a suspension contains its keyword")
            .end;
        let statement_end = self
            .tokens
            .iter()
            .find(|token| statement && token.start >= end)
            .filter(|token| token.is(";"))
            .map_or(end, |token| token.end);
        format!(
            "{};{} {}",
            relocations::source_anchor(self.source_offset + end),
            relocations::source_anchor(self.source_offset + statement_end),
            relocations::unmapped()
        )
    }

    pub(super) fn binding_prefix(&self, name: &str) -> String {
        if let Some(origin) = self.predicates.get(name) {
            return format!(
                "{}{name} = ",
                relocations::source_anchor(self.source_offset + origin.start)
            );
        }
        let reference = self.bindings.get(name).map_or_else(
            || name.to_string(),
            |&(start, end)| {
                relocations::relocated_verbatim(
                    self.source_offset + start,
                    self.source_offset + end,
                )
            },
        );
        format!("{reference} = ")
    }

    pub(super) fn completion_origin(&self) -> String {
        self.completion_site.map_or_else(
            || relocations::unmapped().to_string(),
            |(start, _, _)| relocations::source_anchor(start),
        )
    }

    pub(super) fn completion_code(&self, kind: &str, value: &str) -> Option<String> {
        let expression_end = self.completion_site.map_or_else(
            || relocations::unmapped().to_string(),
            |(_, end, _)| relocations::source_anchor(end),
        );
        let statement_end = self.completion_site.map_or_else(
            || relocations::unmapped().to_string(),
            |(_, _, end)| relocations::source_anchor(end),
        );
        Some(format!(
            "{}return {{kind:{}, value:{}({value}{expression_end})}};{statement_end} {}",
            self.completion_origin(),
            serde_json::to_string(kind).ok()?,
            relocations::unmapped(),
            relocations::unmapped()
        ))
    }

    pub(super) fn value_reference(&self, name: &str, tokens: &[Token]) -> String {
        let anchor = tokens.first().map_or_else(
            || relocations::unmapped().to_string(),
            |token| relocations::source_anchor(self.source_offset + token.start),
        );
        format!("{anchor}{name}{}", relocations::unmapped())
    }

    pub(super) fn grouped_value_reference(&self, name: &str, tokens: &[Token]) -> String {
        let inner = &tokens[1..tokens.len() - 1];
        let Some(first) = inner.first() else {
            return self.value_reference(name, tokens);
        };
        let origin = self.source_offset + first.start;
        let prefix = &self.previous.javascript[..origin];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
        let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
        let key = (line, column);
        let index = self
            .previous
            .provenance
            .partition_point(|segment| (segment.generated_line, segment.generated_column) <= key);
        let mapped = index
            .checked_sub(1)
            .and_then(|index| self.previous.provenance.get(index))
            .is_some_and(|segment| segment.generated_line == line && segment.mapped);
        // A lowered template's first captured literal is generated-only; its
        // surrounding logical expression still retains the template boundary.
        if !mapped
            && inner
                .first()
                .is_some_and(|token| token.kind == TokenKind::String)
        {
            self.value_reference(name, tokens)
        } else {
            self.value_reference(name, inner)
        }
    }

    pub(super) fn scoped_completion(
        &mut self,
        tokens: &[Token],
        kind: &str,
        span: &SourceSpan,
        next: usize,
    ) -> Option<usize> {
        let previous = self.completion_site.replace((
            self.source_offset + span.start,
            self.source_offset + tokens.last().map_or(span.start, |token| token.end),
            self.source_offset + span.end,
        ));
        let result = self.expression(tokens, None, kind, next);
        self.completion_site = previous;
        result
    }
}
