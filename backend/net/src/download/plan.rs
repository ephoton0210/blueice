// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Segment planning: pure functions with no I/O, so the arithmetic that
//! decides how a file is carved up (`phase-10-download-manager/PLAN.md`'s
//! "Segmentation") is checked exhaustively on its own.

/// A half-open byte range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The initial carve-up of `total` bytes: `n = min(max_connections,
/// ceil(total / min_split))` contiguous spans of equal size, the last one
/// taking the remainder. A resource shorter than `min_split` is one span;
/// an empty resource has none. Degenerate parameters are clamped (zero
/// connections behaves like one, a zero `min_split` like one byte) rather
/// than dividing by zero.
pub fn initial_split(total: u64, max_connections: usize, min_split: u64) -> Vec<Span> {
    if total == 0 {
        return Vec::new();
    }
    let min_split = min_split.max(1);
    let connections = max_connections.max(1) as u64;
    let n = total.div_ceil(min_split).clamp(1, connections);
    let size = total / n;
    (0..n)
        .map(|i| {
            let start = i * size;
            let end = if i == n - 1 { total } else { start + size };
            Span { start, end }
        })
        .collect()
}

/// Where an idle worker should cut a running segment that is at `pos` of
/// `end` (dynamic re-splitting): the midpoint of what remains, so the
/// original keeps the front half and the idle worker takes the back half.
/// `None` when less than twice `min_split` remains -- splitting a small
/// tail costs a new request for almost nothing.
pub fn split_point(pos: u64, end: u64, min_split: u64) -> Option<u64> {
    let min_split = min_split.max(1);
    let remaining = end.saturating_sub(pos);
    if remaining < min_split.saturating_mul(2) {
        return None;
    }
    Some(pos + remaining / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn assert_contiguous_cover(spans: &[Span], total: u64) {
        assert!(!spans.is_empty());
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans.last().unwrap().end, total);
        for pair in spans.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "gap or overlap in {spans:?}");
        }
        for span in spans {
            assert!(!span.is_empty(), "empty span in {spans:?}");
        }
    }

    #[test]
    fn an_empty_resource_has_nothing_to_split() {
        assert!(initial_split(0, 8, MIB).is_empty());
    }

    #[test]
    fn a_resource_smaller_than_the_minimum_split_is_one_span() {
        assert_eq!(
            initial_split(300, 8, MIB),
            vec![Span { start: 0, end: 300 }]
        );
    }

    #[test]
    fn an_evenly_divisible_resource_splits_into_equal_spans() {
        let spans = initial_split(8 * MIB, 8, MIB);
        assert_eq!(spans.len(), 8);
        assert!(spans.iter().all(|s| s.len() == MIB));
        assert_contiguous_cover(&spans, 8 * MIB);
    }

    #[test]
    fn a_large_resource_uses_every_connection_and_the_last_span_takes_the_remainder() {
        let total = 100 * MIB + 5;
        let spans = initial_split(total, 8, MIB);
        assert_eq!(spans.len(), 8);
        assert_contiguous_cover(&spans, total);
        assert_eq!(spans[0].len(), total / 8);
        assert_eq!(spans[7].len(), total / 8 + total % 8);
    }

    #[test]
    fn a_resource_only_a_few_minimum_splits_long_uses_only_that_many_spans() {
        // 3.5 MiB at a 1 MiB minimum: ceil(3.5) = 4 spans, not all 8 connections.
        let total = 3 * MIB + MIB / 2;
        let spans = initial_split(total, 8, MIB);
        assert_eq!(spans.len(), 4);
        assert_contiguous_cover(&spans, total);
    }

    #[test]
    fn degenerate_parameters_are_clamped_not_a_division_by_zero() {
        assert_eq!(
            initial_split(10, 0, 5).len(),
            1,
            "zero connections behaves like one"
        );
        assert_contiguous_cover(&initial_split(10, 4, 0), 10);
    }

    #[test]
    fn every_grid_point_yields_a_contiguous_cover_within_the_connection_limit() {
        for total in [
            1,
            2,
            7,
            1023,
            MIB - 1,
            MIB,
            MIB + 1,
            3 * MIB + 17,
            64 * MIB + 3,
        ] {
            for connections in [1, 2, 3, 8, 16] {
                for min_split in [1, 100, MIB] {
                    let spans = initial_split(total, connections, min_split);
                    assert_contiguous_cover(&spans, total);
                    assert!(
                        spans.len() <= connections,
                        "{total} {connections} {min_split}: {spans:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_segment_with_less_than_twice_the_minimum_left_is_not_split() {
        assert_eq!(split_point(0, 2 * MIB - 1, MIB), None);
        assert_eq!(split_point(MIB, 2 * MIB, MIB), None);
        assert_eq!(split_point(5, 5, MIB), None, "nothing remaining");
        assert_eq!(split_point(9, 5, MIB), None, "a position past the end");
    }

    #[test]
    fn a_segment_splits_at_the_midpoint_of_what_remains() {
        assert_eq!(split_point(0, 10 * MIB, MIB), Some(5 * MIB));
        assert_eq!(split_point(4 * MIB, 10 * MIB, MIB), Some(7 * MIB));
        assert_eq!(
            split_point(0, 2 * MIB, MIB),
            Some(MIB),
            "exactly twice the minimum splits into two minimums"
        );
    }

    #[test]
    fn the_split_point_leaves_at_least_the_minimum_on_both_sides() {
        for (pos, end) in [
            (0, 2 * MIB),
            (0, 2 * MIB + 1),
            (3, 3 + 2 * MIB + 1),
            (10, 10 + 7 * MIB + 13),
        ] {
            let mid = split_point(pos, end, MIB).unwrap();
            assert!(
                mid - pos >= MIB,
                "front half of {pos}..{end} split at {mid}"
            );
            assert!(end - mid >= MIB, "back half of {pos}..{end} split at {mid}");
        }
    }

    #[test]
    fn span_length_is_end_minus_start() {
        assert_eq!(Span { start: 10, end: 25 }.len(), 15);
    }
}
