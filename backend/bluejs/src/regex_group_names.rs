// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Group-name handling between a JavaScript pattern and `regress`.
//!
//! Two properties of named capture groups are not something `regress` gets
//! right on its own, so the pattern is adjusted on its way in:
//!
//! * A group name is an identifier made of code points even without the `u`
//!   flag, but without it `regress` is fed one point per UTF-16 code unit. A
//!   literal surrogate pair inside a name (`(?<𝑓>x)`) is therefore joined into
//!   its code point, so it names the same group as `(?<\u{1d453}>x)`.
//! * `\k<name>` for a name that several groups share (allowed when they sit
//!   in different alternatives, so at most one takes part in a match) refers
//!   to whichever of them matched. `regress` emits an alternation of the
//!   groups' backreferences instead, and the first alternative -- an
//!   unmatched group, which matches the empty string -- always wins. Since a
//!   group that did not take part contributes the empty string, the
//!   *concatenation* of the backreferences is exactly the specified meaning,
//!   and the reference is rewritten to `(?:\i\j...)`.

use std::collections::HashMap;
use std::ops::Range;

/// What a scan of a pattern found. Positions index the scanned points.
#[derive(Default)]
struct Scan {
    /// One entry per capture group, in numbering order: its raw name span.
    groups: Vec<Option<Range<usize>>>,
    /// `\k<name>` references as (index of the backslash, name span). Only
    /// references with a closing `>` are recorded.
    references: Vec<(usize, Range<usize>)>,
}

const BACKSLASH: u32 = b'\\' as u32;

fn is(point: Option<&u32>, ascii: u8) -> bool {
    point == Some(&u32::from(ascii))
}

fn scan(points: &[u32], unicode_sets: bool) -> Scan {
    let find_end = |from: usize| {
        points[from.min(points.len())..]
            .iter()
            .position(|&point| point == u32::from(b'>'))
            .map(|offset| from + offset)
    };
    let mut scan = Scan::default();
    let (mut index, mut class_depth) = (0, 0usize);
    while index < points.len() {
        let point = points[index];
        if point == BACKSLASH {
            if class_depth == 0
                && is(points.get(index + 1), b'k')
                && is(points.get(index + 2), b'<')
            {
                match find_end(index + 3) {
                    Some(end) => {
                        scan.references.push((index, index + 3..end));
                        index = end + 1;
                    }
                    None => index = points.len(),
                }
            } else {
                index += 2;
            }
        } else if class_depth > 0 {
            if point == u32::from(b']') {
                class_depth -= 1;
            } else if point == u32::from(b'[') && unicode_sets {
                class_depth += 1;
            }
            index += 1;
        } else if point == u32::from(b'[') {
            class_depth = 1;
            index += 1;
        } else if point == u32::from(b'(') {
            if !is(points.get(index + 1), b'?') {
                scan.groups.push(None);
                index += 1;
            } else if is(points.get(index + 2), b'<')
                && !is(points.get(index + 3), b'=')
                && !is(points.get(index + 3), b'!')
            {
                match find_end(index + 3) {
                    Some(end) => {
                        scan.groups.push(Some(index + 3..end));
                        index = end + 1;
                    }
                    None => {
                        scan.groups.push(Some(index + 3..points.len()));
                        index = points.len();
                    }
                }
            } else {
                // `(?:`, a lookaround or a modifier group: not a capture.
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    scan
}

/// The identifier a raw name denotes: `\uXXXX`, a pair of them, and
/// `\u{X...}` escapes are decoded, and every other point stands for itself.
fn decode_name(raw: &[u32]) -> String {
    let hex = |digits: &[u32]| {
        digits.iter().try_fold(0u32, |value, &digit| {
            Some(value * 16 + char::from_u32(digit)?.to_digit(16)?)
        })
    };
    let unit_escape = |at: usize| {
        (raw.get(at) == Some(&BACKSLASH) && is(raw.get(at + 1), b'u'))
            .then(|| raw.get(at + 2..at + 6).and_then(hex))
            .flatten()
    };
    let mut name = String::new();
    let mut index = 0;
    while index < raw.len() {
        let mut point = raw[index];
        let mut width = 1;
        if point == BACKSLASH && is(raw.get(index + 1), b'u') {
            if is(raw.get(index + 2), b'{') {
                let close = raw[index..].iter().position(|&p| p == u32::from(b'}'));
                if let Some(close) = close {
                    if let Some(value) = hex(&raw[index + 3..index + close]) {
                        point = value;
                        width = close + 1;
                    }
                }
            } else if let Some(unit) = unit_escape(index) {
                point = unit;
                width = 6;
                if (0xD800..0xDC00).contains(&unit) {
                    if let Some(low) =
                        unit_escape(index + 6).filter(|l| (0xDC00..0xE000).contains(l))
                    {
                        point = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                        width = 12;
                    }
                }
            }
        }
        name.push(char::from_u32(point).unwrap_or('\u{FFFD}'));
        index += width;
    }
    name
}

/// Joins a literal surrogate pair inside `span` into one code point.
fn join_pairs(points: &[u32], spans: &[Range<usize>]) -> Vec<u32> {
    let mut joined = Vec::with_capacity(points.len());
    let mut index = 0;
    while index < points.len() {
        let high = points[index];
        let low = points.get(index + 1).copied().unwrap_or(0);
        let in_name = spans
            .iter()
            .any(|span| span.contains(&index) && span.contains(&(index + 1)));
        if in_name && (0xD800..0xDC00).contains(&high) && (0xDC00..0xE000).contains(&low) {
            joined.push(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00));
            index += 2;
        } else {
            joined.push(high);
            index += 1;
        }
    }
    joined
}

/// The number of capture groups of a pattern read one code unit at a time and
/// whether any of them is named.
pub(crate) fn capture_group_counts(points: &[u32]) -> (usize, bool) {
    let found = scan(points, false);
    (found.groups.len(), found.groups.iter().any(Option::is_some))
}

/// A pattern as `regress` should read it.
pub(crate) struct RegressPattern {
    pub(crate) points: Vec<u32>,
    /// The pattern is case-insensitive without the `u` and `v` flags and was
    /// rewritten by `regex_canonicalize`: `regress` must run it without the `i`
    /// flag, on the subject after `regex_canonicalize::canonicalize_subject`.
    pub(crate) canonical: bool,
}

/// The pattern as `regress` should read it: one point per code point with the
/// `u` and `v` flags, one per UTF-16 code unit without them, and in both
/// cases with the group-name adjustments described in the module comment, the
/// escape adjustments of `regex_escapes` and, under the `u` and `v` flags, the
/// backreference guards of `regex_backrefs`. A case-insensitive pattern
/// without those flags is first rewritten by `regex_canonicalize`.
pub(crate) fn regress_points(source: &[u16], flags: &str) -> RegressPattern {
    let (points, canonical) = named_group_points(source, flags);
    let points = if flags.contains(['u', 'v']) {
        crate::regex_backrefs::guard_backreferences(points, flags.contains('v'))
    } else {
        points
    };
    RegressPattern { points, canonical }
}

fn named_group_points(source: &[u16], flags: &str) -> (Vec<u32>, bool) {
    let unicode = flags.contains(['u', 'v']);
    let unicode_sets = flags.contains('v');
    let points: Vec<u32> = if unicode {
        char::decode_utf16(source.iter().copied())
            .map(|c| c.map_or_else(|e| u32::from(e.unpaired_surrogate()), |c| c as u32))
            .collect()
    } else {
        source.iter().map(|&unit| u32::from(unit)).collect()
    };
    let canonical_points = (!unicode && flags.contains('i'))
        .then(|| crate::regex_canonicalize::rewrite_pattern(&points))
        .flatten();
    let canonical = canonical_points.is_some();
    let points = canonical_points.unwrap_or(points);
    let mut points = crate::regex_escapes::adjust_escapes(points, flags);
    if !points.contains(&u32::from(b'<')) {
        return (points, canonical);
    }
    let mut found = scan(&points, unicode_sets);
    let has_named_group = found.groups.iter().any(Option::is_some);
    if !unicode {
        // `\k<name>` is only a reference when the pattern has a named group.
        let mut spans: Vec<Range<usize>> = found.groups.iter().flatten().cloned().collect();
        if has_named_group {
            spans.extend(found.references.iter().map(|(_, span)| span.clone()));
        }
        points = join_pairs(&points, &spans);
        found = scan(&points, unicode_sets);
    }
    let mut numbers: HashMap<String, Vec<usize>> = HashMap::new();
    for (number, group) in found.groups.iter().enumerate() {
        if let Some(span) = group {
            numbers
                .entry(decode_name(&points[span.clone()]))
                .or_default()
                .push(number + 1);
        }
    }
    let mut rewritten = Vec::with_capacity(points.len());
    let mut copied = 0;
    for (backslash, span) in &found.references {
        let Some(groups) = numbers
            .get(&decode_name(&points[span.clone()]))
            .filter(|groups| groups.len() > 1)
        else {
            continue;
        };
        rewritten.extend_from_slice(&points[copied..*backslash]);
        rewritten.extend("(?:".chars().map(u32::from));
        for number in groups {
            rewritten.push(BACKSLASH);
            rewritten.extend(number.to_string().chars().map(u32::from));
        }
        rewritten.push(u32::from(b')'));
        copied = span.end + 1;
    }
    if copied == 0 {
        return (points, canonical);
    }
    rewritten.extend_from_slice(&points[copied..]);
    (rewritten, canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn text(points: &[u32]) -> String {
        points
            .iter()
            .map(|&p| char::from_u32(p).unwrap_or('\u{FFFD}'))
            .collect()
    }

    fn points(source: &str, flags: &str) -> Vec<u32> {
        regress_points(&units(source), flags).points
    }

    #[test]
    fn a_reference_to_a_shared_name_becomes_the_concatenation_of_its_groups() {
        let rewritten = points(r"(?:(?<x>a)|(?<x>b))\k<x>", "");
        assert_eq!(text(&rewritten), r"(?:(?<x>a)|(?<x>b))(?:\1\2)");
        // Names that are not shared, unrelated groups and classes are untouched.
        for source in [
            r"(?<x>a)\k<x>",
            r"(a)(?<x>b)\k<x>",
            r"[\k<x>](?<y>a)|(?<y>b)",
            r"(?<=a)(?<x>b)",
        ] {
            assert_eq!(text(&points(source, "")), source, "{source}");
        }
        // Escapes spell the same name; numbering skips non-capturing groups and lookarounds.
        let (rewritten, _) =
            named_group_points(&units(r"(?=z)(?<x>a)|(?:q)(?<x>b)\k<\u{78}>"), "u");
        assert_eq!(text(&rewritten), r"(?=z)(?<x>a)|(?:q)(?<x>b)(?:\1\2)");
    }

    #[test]
    fn the_unicode_flags_guard_every_reference_including_the_rewritten_ones() {
        let source = r"(?:(?<x>a)|(?<x>b))\k<x>\1";
        let (plain, _) = named_group_points(&units(source), "u");
        assert_eq!(text(&plain), r"(?:(?<x>a)|(?<x>b))(?:\1\2)\1");
        for flags in ["u", "v", "ui"] {
            let guarded = points(source, flags);
            let expected =
                crate::regex_backrefs::guard_backreferences(plain.clone(), flags.contains('v'));
            assert_eq!(guarded, expected, "{flags}");
            assert_eq!(text(&guarded).matches("(?<=").count(), 6, "{flags}");
        }
        // Without them the subject is code units, so nothing is guarded.
        for flags in ["", "g"] {
            assert_eq!(
                text(&points(source, flags)),
                r"(?:(?<x>a)|(?<x>b))(?:\1\2)\1",
                "{flags}"
            );
        }
    }

    #[test]
    fn a_case_insensitive_pattern_without_the_unicode_flags_is_canonicalized() {
        let pattern = regress_points(&units(r"(?<name>s)\k<name>"), "i");
        assert!(pattern.canonical);
        // Letters become their canonical form, names are left alone.
        assert_eq!(text(&pattern.points), r"(?<name>S)\k<name>");
        for flags in ["", "g", "u", "iu", "iv", "v"] {
            assert!(
                !regress_points(&units(r"(?<name>s)"), flags).canonical,
                "{flags}"
            );
        }
        // A pattern the rewrite does not model is left for `regress` to fold.
        let unmodelled = regress_points(&units(r"(?i:s)"), "i");
        assert!(!unmodelled.canonical);
        assert_eq!(text(&unmodelled.points), r"(?i:s)");
        // The rewritten pattern still goes through the group-name adjustments.
        let shared = regress_points(&units(r"(?:(?<x>a)|(?<x>b))\k<x>"), "i");
        assert!(shared.canonical);
        assert_eq!(text(&shared.points), r"(?:(?<x>A)|(?<x>B))(?:\1\2)");
    }

    #[test]
    fn capture_groups_are_counted_outside_classes_and_escapes() {
        let counts = |source: &str| {
            let points: Vec<u32> = source.encode_utf16().map(u32::from).collect();
            capture_group_counts(&points)
        };
        assert_eq!(counts(r"(a)(?:b)(?=c)(?<=d)(?!e)(?<!f)[(]\(g"), (1, false));
        assert_eq!(counts(r"(?<x>a)(b)(?<y>c)\k<x>"), (3, true));
        assert_eq!(counts(""), (0, false));
    }

    #[test]
    fn astral_names_are_joined_only_where_they_are_names() {
        let joined = points("(?<\u{1d453}>x)\\k<\u{1d453}>", "");
        assert_eq!(
            joined.len(),
            "(?<\u{1d453}>x)\\k<\u{1d453}>".chars().count()
        );
        // Without a named group `\k<...>` is plain text, so its pair stays as two units.
        let plain = points("\\k<\u{1d453}>", "");
        assert_eq!(plain.len(), "\\k<\u{1d453}>".encode_utf16().count());
        // With the u flag every point is already a code point.
        assert_eq!(points("(?<\u{1d453}>x)", "u").len(), 7);
    }
}
