// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expand decorator relocation markers without discarding their source ranges.

use super::{compact_expression, TextEdit};

pub(super) enum Part {
    Generated(String),
    Anchor(usize),
    Relocated {
        text: String,
        start: usize,
        end: usize,
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
        for edit in edits.iter().filter(|edit| {
            edit.start >= start && edit.end <= end && !edit.replacement.contains('\u{0}')
        }) {
            if edit.start < cursor {
                continue;
            }
            text.push_str(&source[cursor..edit.start]);
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
        });
    }
    if !rest.is_empty() {
        parts.push(Part::Generated(rest.to_string()));
    }
    parts
}
