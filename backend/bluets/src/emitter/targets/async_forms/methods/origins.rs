// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Typed method origins survive generated signatures and adjacent class braces.
use super::*;
use crate::emitter::{source_line_column, ProvenanceSegment};

pub(super) struct Point {
    generated: usize,
    original: usize,
}

impl Point {
    pub(super) fn new(source: &str, prefix: &str, start: usize, original: usize) -> Self {
        let length: usize = relocations::expand(prefix, source, &[])
            .iter()
            .map(|part| part.text().len())
            .sum();
        Self {
            generated: start + length,
            original,
        }
    }

    pub(super) fn apply(self, emitted: &mut EmittedJavaScript, original: &str) {
        let (generated_line, generated_column) =
            source_line_column(&emitted.javascript, self.generated);
        let (source_line, source_column) = source_line_column(original, self.original);
        let point = ProvenanceSegment {
            generated_line,
            generated_column,
            source_line,
            source_column,
            mapped: true,
            name_index: None,
        };
        let key = (generated_line, generated_column);
        let index = emitted
            .provenance
            .partition_point(|point| (point.generated_line, point.generated_column) < key);
        if let Some(current) = emitted
            .provenance
            .get_mut(index)
            .filter(|point| (point.generated_line, point.generated_column) == key)
        {
            *current = point;
        } else if emitted
            .segment_limit
            .is_some_and(|limit| emitted.provenance.len() >= limit)
        {
            emitted.overflowed = true;
        } else {
            emitted.provenance.insert(index, point);
        }
    }
}
