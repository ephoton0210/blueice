// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Escapes whose meaning depends on the `u` flag, adjusted on their way into
//! `regress` (see also `regex_group_names`).
//!
//! * `regress` reads `\u{X...}` and joins `\uD83D\uDC38` into one code point
//!   whatever the flags are. Without `u` the first is `\u` (an identity
//!   escape for `u`) followed by ordinary text, and the second is two escapes
//!   for two code units, each with its own quantifier. With `u` a lead
//!   surrogate escape pairs only with an immediately following `\u` escape of
//!   a trail surrogate; on any other continuation `regress` gives up on the
//!   pair but keeps the second `\u` it had already consumed, so `\uD83D\u3042*`
//!   turns into the literal text `D83D` followed by `3042*` and `\uD83D\u` is
//!   no syntax error. A lead surrogate escape that does not form a pair is
//!   therefore spelled `\u{D83D}`, which `regress` never combines with what
//!   follows. Group names are left alone: there a lead and a trail escape
//!   form one code point whatever the flags are, and `regress` reads them so.
//! * The word characters of `/iu` and `/iv` are `[A-Za-z0-9_]` plus U+017F and U+212A
//!   (ECMA-262 WordCharacters), so `\W` never matches any case variant of
//!   `s` or `k`. `regress` closes the complement of the basic word set under
//!   case folding, which lets `[\W]` and `[^\W]` treat those two characters as
//!   non-word characters and drag `s` and `k` along. Inside a class `\W` is
//!   therefore written out as the complement of the extended word set.

const BACKSLASH: u32 = b'\\' as u32;

fn is(point: Option<&u32>, ascii: u8) -> bool {
    point == Some(&u32::from(ascii))
}

/// The value of four hex digits starting at `at`.
fn hex4(points: &[u32], at: usize) -> Option<u32> {
    points
        .get(at..at + 4)?
        .iter()
        .try_fold(0u32, |value, &digit| {
            Some(value * 16 + char::from_u32(digit)?.to_digit(16)?)
        })
}

/// Everything but `[0-9A-Za-z_\u{17F}\u{212A}]`, as class members.
const NON_WORD_UNICODE_IGNORE_CASE: &str =
    "\\x00-\\x2f\\x3a-\\x40\\x5b-\\x5e\\x60\\u{7b}-\\u{17e}\\u{180}-\\u{2129}\\u{212b}-\\u{10ffff}";

/// The pattern as the matcher process should receive it: in the `u` and `v`
/// modes a braced escape may carry any number of leading zeros
/// (`\u{000...0041}`), which say nothing, and the request that carries a
/// pattern has a fixed size limit that a pattern of millions of digits would
/// exceed. Runs of two or more leading zeros are removed; everything else,
/// and every pattern without such a run, is returned unchanged.
pub(crate) fn strip_braced_leading_zeros<'a>(
    units: &'a [u16],
    flags: &str,
) -> std::borrow::Cow<'a, [u16]> {
    if !flags.contains(['u', 'v']) {
        return std::borrow::Cow::Borrowed(units);
    }
    let zero = u16::from(b'0');
    let mut stripped: Option<Vec<u16>> = None;
    let (mut index, mut copied) = (0, 0);
    while index < units.len() {
        if units[index] != u16::from(b'\\') {
            index += 1;
            continue;
        }
        let braced_escape = units.get(index + 1) == Some(&u16::from(b'u'))
            && units.get(index + 2) == Some(&u16::from(b'{'));
        if !braced_escape {
            index += 2;
            continue;
        }
        let digits = index + 3;
        let zeros = units[digits.min(units.len())..]
            .iter()
            .take_while(|&&unit| unit == zero)
            .count();
        index = digits + zeros;
        if zeros >= 2 {
            // Keep one zero when nothing else is left in the escape.
            let keep = usize::from(units.get(index) == Some(&u16::from(b'}')));
            let stripped = stripped.get_or_insert_with(|| Vec::with_capacity(units.len()));
            stripped.extend_from_slice(&units[copied..digits]);
            stripped.extend(std::iter::repeat_n(zero, keep));
            copied = index;
        }
    }
    match stripped {
        Some(mut stripped) => {
            stripped.extend_from_slice(&units[copied..]);
            std::borrow::Cow::Owned(stripped)
        }
        None => std::borrow::Cow::Borrowed(units),
    }
}

/// Rewrites `points` (one entry per code unit without the `u` and `v` flags,
/// one per code point with them) as described in the module comment.
pub(crate) fn adjust_escapes(points: Vec<u32>, flags: &str) -> Vec<u32> {
    let unicode = flags.contains(['u', 'v']);
    let unicode_sets = flags.contains('v');
    let extended_words = unicode && flags.contains('i');
    if !points.contains(&BACKSLASH) {
        return points;
    }
    let mut adjusted = Vec::with_capacity(points.len());
    // Classes nest only under the `v` flag.
    let (mut index, mut class_depth) = (0, 0usize);
    while index < points.len() {
        let point = points[index];
        let escaped = points.get(index + 1).copied();
        let in_class = class_depth > 0;
        // `(?<name>` and `\k<name>`: copied as they are, through the `>`.
        let name_start = if in_class {
            None
        } else if point == u32::from(b'(')
            && is(points.get(index + 1), b'?')
            && is(points.get(index + 2), b'<')
        {
            Some(index + 3).filter(|&at| !is(points.get(at), b'=') && !is(points.get(at), b'!'))
        } else if point == BACKSLASH
            && escaped == Some(u32::from(b'k'))
            && is(points.get(index + 2), b'<')
        {
            Some(index + 3)
        } else {
            None
        };
        if let Some(start) = name_start {
            let end = points[start..]
                .iter()
                .position(|&p| p == u32::from(b'>'))
                .map_or(points.len(), |offset| start + offset + 1);
            adjusted.extend_from_slice(&points[index..end]);
            index = end;
            continue;
        }
        if point != BACKSLASH {
            match point {
                0x5b if !in_class || unicode_sets => class_depth += 1,
                0x5d => class_depth = class_depth.saturating_sub(1),
                _ => {}
            }
            adjusted.push(point);
            index += 1;
            continue;
        }
        if escaped == Some(u32::from(b'u')) {
            if !unicode && is(points.get(index + 2), b'{') {
                // Not an escape: `u`, then whatever follows.
                adjusted.push(u32::from(b'u'));
                index += 2;
                continue;
            }
            if let Some(lead) =
                hex4(&points, index + 2).filter(|lead| (0xd800..0xdc00).contains(lead))
            {
                let pairs = unicode
                    && is(points.get(index + 6), b'\\')
                    && is(points.get(index + 7), b'u')
                    && hex4(&points, index + 8)
                        .is_some_and(|trail| (0xdc00..0xe000).contains(&trail));
                if pairs {
                    adjusted.extend_from_slice(&points[index..index + 12]);
                    index += 12;
                } else {
                    adjusted.extend(format!("\\u{{{lead:x}}}").chars().map(u32::from));
                    index += 6;
                }
                continue;
            }
        } else if escaped == Some(u32::from(b'W')) && extended_words && in_class {
            // Under `v` the operand of a set operation must be a class of its own.
            let open = unicode_sets.then_some('[');
            let close = unicode_sets.then_some(']');
            adjusted.extend(
                open.into_iter()
                    .chain(NON_WORD_UNICODE_IGNORE_CASE.chars())
                    .chain(close)
                    .map(u32::from),
            );
            index += 2;
            continue;
        }
        adjusted.push(point);
        adjusted.extend(escaped);
        index += 2;
    }
    adjusted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(text: &str, flags: &str) -> Vec<u32> {
        if flags.contains(['u', 'v']) {
            text.chars().map(u32::from).collect()
        } else {
            text.encode_utf16().map(u32::from).collect()
        }
    }

    fn adjust(text: &str, flags: &str) -> String {
        adjust_escapes(points(text, flags), flags)
            .into_iter()
            .map(|point| char::from_u32(point).unwrap())
            .collect()
    }

    #[test]
    fn four_digit_escape_rejects_a_non_scalar_input_point() {
        assert_eq!(
            hex4(
                &[u32::from(b'0'), u32::from(b'0'), u32::from(b'0'), 0x11_0000],
                0
            ),
            None
        );
    }

    #[test]
    fn braced_escapes_lose_their_backslash_without_the_u_flag() {
        assert_eq!(adjust(r"\u{41}", ""), r"u{41}");
        assert_eq!(adjust(r"[\u{41}]x\\u{41}", "i"), r"[u{41}]x\\u{41}");
        assert_eq!(adjust(r"\u{41}", "u"), r"\u{41}");
        assert_eq!(adjust(r"\u{41}", "v"), r"\u{41}");
    }

    #[test]
    fn leading_zeros_of_braced_escapes_are_stripped_only_with_the_u_or_v_flag() {
        let strip = |text: &str, flags: &str| {
            let units: Vec<u16> = text.encode_utf16().collect();
            String::from_utf16(&strip_braced_leading_zeros(&units, flags)).unwrap()
        };
        assert_eq!(strip(r"\u{000041}", "u"), r"\u{41}");
        assert_eq!(strip(r"\u{000041}", "v"), r"\u{41}");
        assert_eq!(strip(r"\u{0000}", "u"), r"\u{0}");
        assert_eq!(
            strip(r"[\u{0001}\u{00000002}]\u{003}x\u{00g}", "u"),
            r"[\u{1}\u{2}]\u{3}x\u{g}"
        );
        // One zero, a lone escape, an escaped backslash and no u flag are all left as they are.
        assert_eq!(
            strip(r"\u{01}\\u{0002}\u0041", "u"),
            r"\u{01}\\u{0002}\u0041"
        );
        assert_eq!(strip(r"\u{0002}", ""), r"\u{0002}");
        assert_eq!(strip(r"\u{000", "u"), r"\u{");
        assert_eq!(strip("abc", "u"), "abc");
    }

    #[test]
    fn a_lead_surrogate_escape_is_braced_unless_it_pairs() {
        assert_eq!(adjust(r"\uD83D\uDC38", ""), r"\u{d83d}\uDC38");
        assert_eq!(adjust(r"\uD83D\uDC38", "u"), r"\uD83D\uDC38");
        assert_eq!(adjust(r"\uD83D\u3042*", "u"), r"\u{d83d}\u3042*");
        assert_eq!(adjust(r"\uD83D\u{DC38}", "u"), r"\u{d83d}\u{DC38}");
        assert_eq!(adjust(r"\uD83D\u", "u"), r"\u{d83d}\u");
        assert_eq!(adjust(r"\uD83D", "u"), r"\u{d83d}");
        // A trail or a non-surrogate escape is left alone.
        assert_eq!(adjust(r"\uDC38\u0041", "u"), r"\uDC38\u0041");
        // The lower-case spelling of the hex digits is understood too.
        assert_eq!(adjust(r"\ud83d\udc38", "u"), r"\ud83d\udc38");
        assert_eq!(adjust(r"\ud83d\udc38", ""), r"\u{d83d}\udc38");
    }

    #[test]
    fn group_names_are_copied_verbatim() {
        for flags in ["", "u"] {
            for source in [
                r"(?<\ud835\udc9c>x)\k<\ud835\udc9c>",
                r"(?<\u{1d49c}\ud83d>x)",
                r"(?<a\ud83d>x)",
            ] {
                assert_eq!(adjust(source, flags), source, "{source} {flags}");
            }
        }
        // Lookbehinds and classes are not names; the text after a name is adjusted again.
        assert_eq!(adjust(r"(?<=\ud83d)", ""), r"(?<=\u{d83d})");
        assert_eq!(adjust(r"[(?<\ud83d>]", ""), r"[(?<\u{d83d}>]");
        assert_eq!(adjust(r"(?<x>\ud83d)", ""), r"(?<x>\u{d83d})");
    }

    #[test]
    fn w_in_a_class_is_written_out_only_for_unicode_ignore_case() {
        let written_out = adjust(r"[^\W_]", "iu");
        assert!(written_out.starts_with("[^\\x00-\\x2f") && written_out.ends_with("\\u{10ffff}_]"));
        for flags in ["u", "i", "v", ""] {
            assert_eq!(adjust(r"[^\W_]", flags), r"[^\W_]", "{flags}");
        }
        // Outside a class, and an escaped backslash before W, are untouched.
        assert_eq!(adjust(r"\W[\\W]", "iu"), r"\W[\\W]");
        assert_eq!(adjust(r"\W[\\W]", "iv"), r"\W[\\W]");
    }

    #[test]
    fn w_in_a_class_is_a_class_of_its_own_under_the_v_flag() {
        // The class text is the same as under `u`, but an operand of a set operation
        // must be a class, and classes nest, so `\W` after an inner class is still inside one.
        let members = adjust(r"[^\W_]", "iu");
        let members = &members[2..members.len() - 2];
        assert_eq!(adjust(r"[^\W_]", "iv"), format!("[^[{members}]_]"));
        assert_eq!(adjust(r"[[a]\W]", "iv"), format!("[[a][{members}]]"));
        assert_eq!(adjust(r"[[a]--\W]", "iv"), format!("[[a]--[{members}]]"));
        // Under `u` the class ends at the first `]`, so this `\W` is outside it.
        assert_eq!(adjust(r"[[a]\W]", "iu"), r"[[a]\W]");
        assert_eq!(adjust(r"[[\W]", "iu"), format!("[[{members}]"));
        // After the class has ended `\W` is outside again.
        assert_eq!(adjust(r"[[a]]\W", "iv"), r"[[a]]\W");
    }
}
