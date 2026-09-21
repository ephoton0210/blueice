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
//! * The word characters of `/iu` are `[A-Za-z0-9_]` plus U+017F and U+212A
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

/// Rewrites `points` (one entry per code unit without the `u` and `v` flags,
/// one per code point with them) as described in the module comment.
pub(crate) fn adjust_escapes(points: Vec<u32>, flags: &str) -> Vec<u32> {
    let unicode = flags.contains(['u', 'v']);
    let extended_words = flags.contains('u') && flags.contains('i');
    if !points.contains(&BACKSLASH) {
        return points;
    }
    let mut adjusted = Vec::with_capacity(points.len());
    let (mut index, mut in_class) = (0, false);
    while index < points.len() {
        let point = points[index];
        let escaped = points.get(index + 1).copied();
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
                0x5b => in_class = true,
                0x5d => in_class = false,
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
            adjusted.extend(NON_WORD_UNICODE_IGNORE_CASE.chars().map(u32::from));
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
    fn braced_escapes_lose_their_backslash_without_the_u_flag() {
        assert_eq!(adjust(r"\u{41}", ""), r"u{41}");
        assert_eq!(adjust(r"[\u{41}]x\\u{41}", "i"), r"[u{41}]x\\u{41}");
        assert_eq!(adjust(r"\u{41}", "u"), r"\u{41}");
        assert_eq!(adjust(r"\u{41}", "v"), r"\u{41}");
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
        for flags in ["u", "i", "iv", ""] {
            assert_eq!(adjust(r"[^\W_]", flags), r"[^\W_]", "{flags}");
        }
        // Outside a class, and an escaped backslash before W, are untouched.
        assert_eq!(adjust(r"\W[\\W]", "iu"), r"\W[\\W]");
    }
}
