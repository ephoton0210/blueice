// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Copied syntax retains mapping state when printing changes its trivia.

use super::*;

pub(super) struct CopyPolicy<'a> {
    pub(super) prefix_owners: &'a BTreeSet<(usize, usize)>,
    pub(super) suppressed_boundaries: &'a BTreeSet<usize>,
    pub(super) split_boundaries: &'a BTreeSet<usize>,
}

pub(super) fn copied_states(
    text: &str,
    points: &[ProvenanceSegment],
    offsets: &[usize],
    edits: &[TextEdit],
    ranges: &[(usize, usize, usize, usize)],
    policy: CopyPolicy<'_>,
) -> Vec<(usize, ProvenanceSegment)> {
    if points.is_empty() {
        return Vec::new();
    }
    let CopyPolicy {
        prefix_owners,
        suppressed_boundaries,
        split_boundaries,
    } = policy;
    let targets = edits
        .iter()
        .zip(ranges)
        .filter(|(edit, _)| {
            text[edit.start..edit.end].contains(['\r', '\n'])
                || edit.replacement.contains(['\r', '\n'])
                || edit.start == edit.end
                    && !edit.replacement.is_empty()
                    && split_boundaries.contains(&edit.end)
                || offsets
                    .get(offsets.partition_point(|offset| *offset <= edit.start))
                    .is_some_and(|offset| *offset < edit.end)
        })
        .filter(|(edit, _)| edit.end < text.len())
        .map(|(edit, range)| {
            let line_start = edit
                .replacement
                .rfind(['\r', '\n'])
                .map(|offset| range.2 + offset + 1);
            (edit.end, range.3, line_start)
        })
        .collect::<Vec<_>>();
    let mut target = 0;
    let mut point = 0;
    let mut owner = None;
    let mut result = Vec::with_capacity(targets.len());
    let mut chars = text.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        let before = owner;
        while offsets.get(point) == Some(&offset) {
            if !suppressed_boundaries.contains(&offset) {
                owner = Some(points[point]);
            }
            point += 1;
        }
        while let Some(&(end, generated, line_start)) = targets.get(target) {
            if end != offset {
                break;
            }
            if let Some(start) = line_start {
                // A continuous role retains ownership through inserted
                // indentation. A token beginning a different role keeps its
                // explicit start; its indentation begins unmapped instead.
                if let Some(point) = before.filter(|point: &ProvenanceSegment| {
                    point.mapped
                        && start < generated
                        && prefix_owners.contains(&(point.source_line, point.source_column))
                }) {
                    result.push((start, point));
                } else if owner.is_none_or(|point| !point.mapped) {
                    // The physical line reset already expresses this absence.
                    // CRLF conversion must not add another explicit map event.
                    target += 1;
                    continue;
                }
            }
            result.push((generated, owner.unwrap_or_else(unmapped)));
            target += 1;
        }
        if target == targets.len() {
            break;
        }
        // Source-map lookup starts unmapped on each physical generated line;
        // a removed newline must carry that absence into the joined line too.
        if is_line_break(c, chars.peek().map(|(_, c)| *c)) {
            owner = None;
        }
    }
    result
}

fn unmapped() -> ProvenanceSegment {
    ProvenanceSegment {
        generated_line: 0,
        generated_column: 0,
        source_line: 0,
        source_column: 0,
        mapped: false,
        name_index: None,
    }
}

pub(super) fn generated_entries(
    text: &str,
    points: &[ProvenanceSegment],
    offsets: &[usize],
    tokens: &[Token],
) -> BTreeSet<usize> {
    let boundaries = points
        .iter()
        .zip(offsets)
        .map(|(point, offset)| (*offset, point))
        .collect::<BTreeMap<_, _>>();
    let mut entries = BTreeSet::new();
    for pair in tokens.windows(2) {
        let end = pair[0].end;
        // A lowered source operation can end before a generated statement.
        // Its explicit reset also owns the intervening newline character.
        if !(pair[0].is("{") || pair[0].is(";"))
            || boundaries.get(&end).is_none_or(|point| point.mapped)
        {
            continue;
        }
        let next = pair[1].start;
        let mapped = boundaries
            .range(..=next)
            .next_back()
            .is_some_and(|(at, point)| point.mapped && !text[*at..next].contains(['\r', '\n']));
        if !mapped {
            entries.insert(end);
        }
    }
    entries
}
