// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Backreferences under the `u` and `v` flags, adjusted on their way into
//! `regress` (see also `regex_escapes` and `regex_group_names`).
//!
//! With those flags the subject is a list of code points, and a backreference
//! matches only if each of its captured code points equals the code point at
//! the same place in the subject. `regress` compares code units, so a captured
//! lone lead surrogate "matches" the first half of a surrogate pair in the
//! subject and the match then ends between the two halves. In
//! `/foo(.+)bar\1/u` against `"foo\uD834bar\uD834\uDC00"` the specified result
//! is no match, and `regress` reports one ending after the lone `\uD834`.
//!
//! A backreference can only go wrong at an end that lies between the halves of
//! a pair, and every other atom leaves the matcher between code points, so a
//! position with a lead surrogate right before it and a trail surrogate right
//! after it is exactly what a backreference must not leave behind. Each
//! reference `R` is therefore rewritten to `(?:GRG)` with
//! `G = (?!(?<=[\u{d800}-\u{dbff}])[\u{dc00}-\u{dfff}])`. The guard is
//! needed on both sides because a reference inside a lookbehind is matched
//! backwards, so its far end is the one before it in the pattern. The class
//! members are spelled `\u{...}`, which `regress` never joins with what
//! follows (see `regex_escapes`).

const BACKSLASH: u32 = b'\\' as u32;

/// `(?!(?<=[\u{d800}-\u{dbff}])[\u{dc00}-\u{dfff}])`.
const GUARD: &str = "(?!(?<=[\\u{d800}-\\u{dbff}])[\\u{dc00}-\\u{dfff}])";

fn is(point: Option<&u32>, ascii: u8) -> bool {
    point == Some(&u32::from(ascii))
}

/// The index just past the backreference that starts at `at` (a backslash):
/// `\` and a decimal number that does not start with `0`, or `\k<name>`.
fn reference_end(points: &[u32], at: usize) -> Option<usize> {
    let escaped = *points.get(at + 1)?;
    if (u32::from(b'1')..=u32::from(b'9')).contains(&escaped) {
        let digits = points[at + 2..]
            .iter()
            .take_while(|&&point| (u32::from(b'0')..=u32::from(b'9')).contains(&point))
            .count();
        return Some(at + 2 + digits);
    }
    if escaped == u32::from(b'k') && is(points.get(at + 2), b'<') {
        let close = points[at + 3..]
            .iter()
            .position(|&point| point == u32::from(b'>'))?;
        return Some(at + 3 + close + 1);
    }
    None
}

/// Rewrites `points` (one entry per code point) as described in the module
/// comment. `unicode_sets` is the `v` flag, under which classes nest.
pub(crate) fn guard_backreferences(points: Vec<u32>, unicode_sets: bool) -> Vec<u32> {
    if !points.contains(&BACKSLASH) {
        return points;
    }
    let mut guarded = Vec::with_capacity(points.len());
    let (mut index, mut class_depth) = (0, 0usize);
    while index < points.len() {
        let point = points[index];
        if point == BACKSLASH {
            let end = reference_end(&points, index).filter(|_| class_depth == 0);
            if let Some(end) = end {
                guarded.extend("(?:".chars().map(u32::from));
                guarded.extend(GUARD.chars().map(u32::from));
                guarded.extend_from_slice(&points[index..end]);
                guarded.extend(GUARD.chars().map(u32::from));
                guarded.push(u32::from(b')'));
                index = end;
            } else {
                guarded.push(point);
                guarded.extend(points.get(index + 1));
                index += 2;
            }
            continue;
        }
        if point == u32::from(b'[') && (class_depth == 0 || unicode_sets) {
            class_depth += 1;
        } else if point == u32::from(b']') && class_depth > 0 {
            class_depth -= 1;
        }
        guarded.push(point);
        index += 1;
    }
    guarded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guarded(text: &str, unicode_sets: bool) -> String {
        let points = text.chars().map(u32::from).collect();
        guard_backreferences(points, unicode_sets)
            .into_iter()
            .map(|point| char::from_u32(point).unwrap())
            .collect()
    }

    fn wrapped(reference: &str) -> String {
        format!("(?:{GUARD}{reference}{GUARD})")
    }

    #[test]
    fn numbered_and_named_references_are_wrapped_in_guards() {
        assert_eq!(guarded(r"(a)\1", false), format!("(a){}", wrapped(r"\1")));
        assert_eq!(
            guarded(r"(a)\12+", false),
            format!("(a){}+", wrapped(r"\12"))
        );
        assert_eq!(
            guarded(r"(?<n>a)x\k<n>y\k<n>", false),
            format!("(?<n>a)x{}y{}", wrapped(r"\k<n>"), wrapped(r"\k<n>"))
        );
    }

    #[test]
    fn everything_else_is_left_alone() {
        for source in [
            "abc",
            r"\0\d\w\b\B\n\u{41}\\1\.",
            r"\k",
            r"\k<n",
            r"\",
            r"[\1\k<n>]",
            r"(?<n>a)[\k<n>]",
        ] {
            assert_eq!(guarded(source, false), source, "{source}");
        }
        // Escaped backslashes and brackets do not start or end anything.
        assert_eq!(
            guarded(r"\\1\[\1", false),
            format!(r"\\1\[{}", wrapped(r"\1"))
        );
    }

    #[test]
    fn a_class_ends_at_the_first_bracket_unless_the_v_flag_nests_it() {
        assert_eq!(guarded(r"[[]\1", false), format!("[[]{}", wrapped(r"\1")));
        assert_eq!(
            guarded(r"[[a]\1]\1", true),
            format!(r"[[a]\1]{}", wrapped(r"\1"))
        );
        assert_eq!(guarded(r"[a]\1", true), format!("[a]{}", wrapped(r"\1")));
    }
}
