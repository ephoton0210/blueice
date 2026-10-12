// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expand decorator relocation markers without discarding their source ranges.

use super::{compact_expression, token_maps, ProvenanceEmitter, TextEdit};
use crate::syntax::{lex, TokenKind};

pub(super) enum Part {
    Generated(String),
    Anchor(usize),
    Relocated {
        text: String,
        start: usize,
        end: usize,
        erased: Vec<(usize, usize)>,
        rewritten: bool,
    },
}

impl Part {
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Generated(text) | Self::Relocated { text, .. } => text,
            Self::Anchor(_) => "",
        }
    }
}

/// Expand `\0M<start>,<end>\0` relocation and `\0A<offset>\0` source-anchor
/// markers. Nested edits are applied before a relocated expression is compacted.
pub(super) fn expand(replacement: &str, source: &str, edits: &[TextEdit]) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut rest = replacement;
    while let Some(open) = rest.find('\u{0}') {
        if open > 0 {
            parts.push(Part::Generated(rest[..open].to_string()));
        }
        let after = &rest[open + 1..];
        let Some(close) = after.find('\u{0}') else {
            parts.push(Part::Generated(rest[open..].to_string()));
            return parts;
        };
        let marker = &after[..close];
        rest = &after[close + 1..];
        if let Some(offset) = marker.strip_prefix('A').and_then(|text| text.parse().ok()) {
            parts.push(Part::Anchor(offset));
            continue;
        }
        let Some(range) = marker.strip_prefix('M') else {
            continue;
        };
        let Some((start, end)) = range.split_once(',').and_then(|(start, end)| {
            Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
        }) else {
            continue;
        };
        let mut text = String::new();
        let mut cursor = start;
        let mut erased = Vec::new();
        let mut rewritten = false;
        for edit in edits.iter().filter(|edit| {
            edit.start >= start && edit.end <= end && !edit.replacement.contains('\u{0}')
        }) {
            if edit.start < cursor {
                continue;
            }
            text.push_str(&source[cursor..edit.start]);
            if edit.replacement.is_empty() {
                if edit.start != edit.end {
                    erased.push((edit.start, edit.end));
                }
            } else {
                rewritten = true;
            }
            for part in expand(&edit.replacement, source, edits) {
                text.push_str(part.text());
            }
            cursor = edit.end;
        }
        text.push_str(&source[cursor..end]);
        parts.push(Part::Relocated {
            text: compact_expression(&text),
            start,
            end,
            erased,
            rewritten,
        });
    }
    if !rest.is_empty() {
        parts.push(Part::Generated(rest.to_string()));
    }
    parts
}

impl ProvenanceEmitter<'_> {
    pub(super) fn relocate(
        &mut self,
        value: &str,
        source_start: usize,
        source_end: usize,
        erased: &[(usize, usize)],
        rewritten: bool,
    ) {
        if value == &self.source[source_start..source_end] {
            self.copy(value, source_start);
            self.mark(source_end);
            return;
        }
        if self.segment_limit.is_none() || rewritten {
            self.replace(value, source_start);
            self.mark(source_end);
            return;
        }
        let source_tokens = self
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| {
                token.start >= source_start
                    && token.end <= source_end
                    && token.kind != TokenKind::Eof
            })
            .collect::<Vec<_>>();
        // Only whole erased tokens may be omitted. A splice through a token
        // changes its spelling, so the surviving token sequence cannot claim
        // byte-for-byte provenance even if a later token happens to match.
        if source_tokens.iter().any(|(_, token)| {
            erased.iter().any(|&(start, end)| {
                token.start < end
                    && start < token.end
                    && !(start <= token.start && token.end <= end)
            })
        }) {
            self.replace(value, source_start);
            self.mark(source_end);
            return;
        }
        let source_tokens = source_tokens
            .into_iter()
            .filter(|(_, token)| {
                !erased
                    .iter()
                    .any(|&(start, end)| start <= token.start && token.end <= end)
            })
            .collect::<Vec<_>>();
        let Ok(generated_tokens) = lex("<relocated>", value) else {
            self.replace(value, source_start);
            self.mark(source_end);
            return;
        };
        let generated_tokens = generated_tokens
            .iter()
            .filter(|token| token.kind != TokenKind::Eof)
            .collect::<Vec<_>>();
        if source_tokens.len() != generated_tokens.len()
            || source_tokens
                .iter()
                .zip(&generated_tokens)
                .any(|((_, source), generated)| source.text != generated.text)
        {
            self.replace(value, source_start);
            self.mark(source_end);
            return;
        }
        let mut marks = Vec::with_capacity(source_tokens.len().saturating_mul(2));
        for ((index, source), generated) in source_tokens.into_iter().zip(generated_tokens) {
            if token_maps::mark_start(self.source, &self.tokens, index) {
                marks.push((generated.start, source.start));
            }
            if token_maps::mark_end(source) {
                marks.push((generated.end, source.end));
            }
        }
        marks.sort_by_key(|(generated, _)| *generated);
        self.mark(source_start);
        let mut marks = marks.into_iter().peekable();
        let mut characters = value.char_indices().peekable();
        while let Some((offset, character)) = characters.next() {
            while marks
                .peek()
                .is_some_and(|(generated, _)| *generated == offset)
            {
                self.mark(marks.next().unwrap().1);
            }
            self.push(character, characters.peek().map(|(_, next)| *next));
        }
        self.mark(source_end);
    }
}
