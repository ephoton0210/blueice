// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Case-insensitive matching without the `u` and `v` flags, done here instead
//! of by `regress` (see also `regex_escapes` and `regex_group_names`).
//!
//! Without those flags the specification compares two code units through
//! `Canonicalize`: the unit's upper case, unless that is not a single unit or
//! would turn a non-ASCII unit into an ASCII one, so that U+017F (long s) and
//! U+0131 (dotless i) are not `s` and `i` in disguise. `regress` folds through
//! its Unicode case-folding tables instead, which disagree in three places:
//!
//! * a literal is expanded to every unit with the same upper case, ignoring
//!   the ASCII rule, so `/s/i` matches U+017F and `/i/i` matches U+0131;
//! * a character class is closed under Unicode simple case folding, which
//!   also merges the Kelvin sign U+212A with `k`, U+1E9E with U+00DF, U+1FB3
//!   with U+1FBC, and so on, so `/[a-z]/i` and `/\w/i` match U+017F and
//!   U+212A, and `[^\W]` behaves as if `s` were not a word character;
//! * a backreference folds with the upper case table alone.
//!
//! The comparison `Canonicalize(a) = Canonicalize(b)` does not have to be
//! made by the matcher: the pattern and the subject can both be mapped
//! through `Canonicalize` beforehand and matched case-sensitively. The
//! subject is mapped one unit to one unit, so the ranges of a match are the
//! ranges of the original subject. For the pattern, a literal becomes its
//! canonical unit and a class becomes the set of canonical units of its
//! members (a negated class stays negated, which is exactly the specified
//! "no member canonicalizes to it"). Everything that does not depend on case
//! is untouched: `\d \s \w`, `.`, the assertions and the backreferences
//! (which now compare canonical text) all mean the same on canonical text as
//! on the original, because `Canonicalize` is idempotent, maps ASCII to ASCII
//! and non-ASCII to non-ASCII, and does not touch white space, line
//! terminators or digits.
//!
//! Only patterns whose syntax is fully understood here are rewritten; on
//! anything else ([`rewrite_pattern`] returns `None`) the pattern goes to
//! `regress` unchanged and with the `i` flag, exactly as it did before.

use crate::regexp::CharacterClassEscape;

const BACKSLASH: u32 = b'\\' as u32;

fn canonicalize_ascii(unit: u16) -> u16 {
    if (0x61..=0x7a).contains(&unit) {
        unit - 0x20
    } else {
        unit
    }
}

/// `Canonicalize` of a non-ASCII unit, from the upper case mapping.
fn canonicalize_non_ascii(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut upper = character.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => match u16::try_from(u32::from(single)) {
            Ok(mapped) if mapped >= 0x80 => mapped,
            _ => unit,
        },
        _ => unit,
    }
}

/// `Canonicalize` of every code unit, built the first time a non-ASCII unit
/// needs it.
fn table() -> &'static [u16] {
    static TABLE: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        (0..=u16::MAX)
            .map(|unit| {
                if unit < 0x80 {
                    canonicalize_ascii(unit)
                } else {
                    canonicalize_non_ascii(unit)
                }
            })
            .collect()
    })
}

/// ECMA-262 `Canonicalize(rer, ch)` for a pattern with neither `u` nor `v`.
pub(crate) fn canonicalize(unit: u16) -> u16 {
    if unit < 0x80 {
        canonicalize_ascii(unit)
    } else {
        table()[usize::from(unit)]
    }
}

/// `subject` with every code unit canonicalized.
pub(crate) fn canonicalize_subject(subject: &[u16]) -> Vec<u16> {
    if subject.iter().all(|&unit| unit < 0x80) {
        return subject
            .iter()
            .map(|&unit| canonicalize_ascii(unit))
            .collect();
    }
    let table = table();
    subject
        .iter()
        .map(|&unit| table[usize::from(unit)])
        .collect()
}

/// A set of code units.
struct UnitSet(Vec<u64>);

impl UnitSet {
    fn new() -> Self {
        Self(vec![0; 0x10000 / 64])
    }

    fn add(&mut self, unit: u16) {
        self.0[usize::from(unit >> 6)] |= 1 << (unit & 63);
    }

    /// The members, in order.
    fn members(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().enumerate().flat_map(|(index, &word)| {
            let mut bits = word;
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                Some((index * 64 + bit) as u16)
            })
        })
    }

    fn add_atom(&mut self, atom: Atom) {
        match atom {
            Atom::Unit(unit) => self.add(unit),
            Atom::Escape(escape) => {
                for unit in 0..=u16::MAX {
                    if escape.matches(u32::from(unit)) {
                        self.add(unit);
                    }
                }
            }
        }
    }

    /// The maximal runs of the canonical units of the members.
    fn canonical_ranges(&self) -> Vec<(u16, u16)> {
        let mut canonical = Self::new();
        for unit in self.members() {
            canonical.add(canonicalize(unit));
        }
        let mut ranges: Vec<(u16, u16)> = Vec::new();
        for unit in canonical.members() {
            match ranges.last_mut() {
                Some((_, high)) if *high + 1 == unit => *high = unit,
                _ => ranges.push((unit, unit)),
            }
        }
        ranges
    }
}

#[derive(Clone, Copy)]
enum Atom {
    Unit(u16),
    Escape(CharacterClassEscape),
}

fn is(points: &[u32], at: usize, ascii: u8) -> bool {
    points.get(at) == Some(&u32::from(ascii))
}

fn ascii_char(point: u32) -> Option<char> {
    u8::try_from(point)
        .ok()
        .filter(u8::is_ascii)
        .map(char::from)
}

fn is_decimal(point: u32) -> bool {
    (u32::from(b'0')..=u32::from(b'9')).contains(&point)
}

/// The value of `count` hex digits at `at`.
fn hex(points: &[u32], at: usize, count: usize) -> Option<u16> {
    points
        .get(at..at + count)?
        .iter()
        .try_fold(0u32, |value, &digit| {
            Some(value * 16 + char::from_u32(digit)?.to_digit(16)?)
        })
        .and_then(|value| u16::try_from(value).ok())
}

/// A legacy octal escape (Annex B) whose digits start at `at`: one to three
/// octal digits, three only when the first is at most `3`, so that the value
/// is at most 255. Returns the value and the index after the digits.
fn octal(points: &[u32], at: usize) -> Option<(u16, usize)> {
    let digit = |index: usize| {
        points
            .get(index)
            .and_then(|&point| char::from_u32(point))
            .and_then(|character| character.to_digit(8))
    };
    let first = digit(at)?;
    let longest = if first <= 3 { 3 } else { 2 };
    let (mut value, mut end) = (first, at + 1);
    while end < at + longest {
        let Some(next) = digit(end) else { break };
        value = value * 8 + next;
        end += 1;
    }
    Some((value as u16, end))
}

fn is_syntax(unit: u16) -> bool {
    unit < 0x80 && b"^$\\.*+?()[]{}|".contains(&(unit as u8))
}

/// A pattern character that stands for itself.
fn push_literal(out: &mut Vec<u32>, unit: u16) {
    let unit = canonicalize(unit);
    if is_syntax(unit) {
        out.push(BACKSLASH);
    }
    out.push(u32::from(unit));
}

/// A literal the pattern spelled with an escape: like [`push_literal`], but a
/// digit is spelled `\xNN` so that it cannot become part of a preceding `\1`
/// or `\0`.
fn push_escaped_literal(out: &mut Vec<u32>, unit: u16) {
    let unit = canonicalize(unit);
    if (u16::from(b'0')..=u16::from(b'9')).contains(&unit) {
        out.extend(format!("\\x{unit:02x}").chars().map(u32::from));
    } else {
        push_literal(out, unit);
    }
}

/// The rewrite of the escape that starts at `at` (a backslash); returns the
/// index after it.
fn escape(
    points: &[u32],
    at: usize,
    groups: usize,
    named: bool,
    out: &mut Vec<u32>,
) -> Option<usize> {
    let escaped = *points.get(at + 1)?;
    let next = at + 2;
    match ascii_char(escaped) {
        // Class escapes, assertions and control escapes mean the same on canonical text.
        Some('d' | 'D' | 's' | 'S' | 'w' | 'W' | 'b' | 'B' | 'f' | 'n' | 'r' | 't' | 'v') => {
            out.extend([BACKSLASH, escaped]);
            Some(next)
        }
        Some('0') => {
            // `\0` is NUL; followed by a digit it is a legacy octal escape (or NUL
            // and then an `8` or a `9`).
            if !points.get(next).is_some_and(|&point| is_decimal(point)) {
                out.extend([BACKSLASH, escaped]);
                return Some(next);
            }
            let (unit, end) = octal(points, at + 1)?;
            push_escaped_literal(out, unit);
            Some(end)
        }
        Some('c') => {
            let letter = points.get(next).copied().and_then(ascii_char);
            if let Some(letter) = letter.filter(char::is_ascii_alphabetic) {
                out.extend([BACKSLASH, escaped, u32::from(letter)]);
                return Some(next + 1);
            }
            // Without a letter the backslash stands for itself and the `c` is an
            // ordinary character.
            out.extend([BACKSLASH, BACKSLASH]);
            Some(at + 1)
        }
        Some('x') => match hex(points, next, 2) {
            Some(unit) => {
                push_escaped_literal(out, unit);
                Some(next + 2)
            }
            None => {
                push_escaped_literal(out, u16::from(b'x'));
                Some(next)
            }
        },
        Some('u') => match hex(points, next, 4) {
            Some(unit) => {
                push_escaped_literal(out, unit);
                Some(next + 4)
            }
            None => {
                push_escaped_literal(out, u16::from(b'u'));
                Some(next)
            }
        },
        Some('1'..='9') => {
            let digits = points[at + 1..]
                .iter()
                .take_while(|&&point| is_decimal(point))
                .count();
            let end = at + 1 + digits;
            let number = points[at + 1..end].iter().fold(0usize, |number, &digit| {
                number
                    .saturating_mul(10)
                    .saturating_add((digit - u32::from(b'0')) as usize)
            });
            if number <= groups {
                out.extend_from_slice(&points[at..end]);
                Some(end)
            } else if escaped >= u32::from(b'8') {
                push_escaped_literal(out, escaped as u16);
                Some(next)
            } else {
                let (unit, end) = octal(points, at + 1)?;
                push_escaped_literal(out, unit);
                Some(end)
            }
        }
        Some('k') => {
            if !named {
                push_escaped_literal(out, u16::from(b'k'));
                return Some(next);
            }
            if !is(points, next, b'<') {
                return None;
            }
            let close = points[next + 1..]
                .iter()
                .position(|&point| point == u32::from(b'>'))?;
            let end = next + 1 + close + 1;
            out.extend_from_slice(&points[at..end]);
            Some(end)
        }
        // An identity escape.
        _ => {
            let unit = u16::try_from(escaped).ok()?;
            if is_syntax(unit) || unit == u16::from(b'/') || unit == u16::from(b'-') {
                out.extend([BACKSLASH, escaped]);
            } else {
                push_escaped_literal(out, unit);
            }
            Some(next)
        }
    }
}

/// The rewrite of the group that opens at `at`; returns the index after its
/// prefix. The names of named groups are copied as they are.
fn group(points: &[u32], at: usize, out: &mut Vec<u32>) -> Option<usize> {
    if !is(points, at + 1, b'?') {
        out.push(points[at]);
        return Some(at + 1);
    }
    let end = match points.get(at + 2).copied().and_then(ascii_char) {
        Some(':' | '=' | '!') => at + 3,
        Some('<')
            if matches!(
                points.get(at + 3).copied().and_then(ascii_char),
                Some('=' | '!')
            ) =>
        {
            at + 4
        }
        Some('<') => {
            let close = points[at + 3..]
                .iter()
                .position(|&point| point == u32::from(b'>'))?;
            at + 3 + close + 1
        }
        // A modifier group, or nothing valid.
        _ => return None,
    };
    out.extend_from_slice(&points[at..end]);
    Some(end)
}

/// The class an escape letter names; the caller has already matched the letter.
fn class_escape(letter: char) -> CharacterClassEscape {
    match letter {
        'd' => CharacterClassEscape::Digit,
        'D' => CharacterClassEscape::NonDigit,
        's' => CharacterClassEscape::Whitespace,
        'S' => CharacterClassEscape::NonWhitespace,
        'w' => CharacterClassEscape::Word,
        _ => CharacterClassEscape::NonWord,
    }
}

/// One member of a class, read at `*index`.
fn class_atom(points: &[u32], index: &mut usize, named: bool) -> Option<Atom> {
    let point = *points.get(*index)?;
    if point != BACKSLASH {
        *index += 1;
        return Some(Atom::Unit(u16::try_from(point).ok()?));
    }
    let escaped = *points.get(*index + 1)?;
    let next = *index + 2;
    let (atom, end) = match ascii_char(escaped) {
        Some(letter @ ('d' | 'D' | 's' | 'S' | 'w' | 'W')) => {
            (Atom::Escape(class_escape(letter)), next)
        }
        Some('b') => (Atom::Unit(0x08), next),
        Some('t') => (Atom::Unit(0x09), next),
        Some('n') => (Atom::Unit(0x0a), next),
        Some('v') => (Atom::Unit(0x0b), next),
        Some('f') => (Atom::Unit(0x0c), next),
        Some('r') => (Atom::Unit(0x0d), next),
        Some('0'..='7') => {
            let (unit, end) = octal(points, *index + 1)?;
            (Atom::Unit(unit), end)
        }
        Some('c') => match points.get(next).copied().and_then(ascii_char) {
            // Annex B lets a digit or an underscore follow `\c` in a class.
            Some(control) if control.is_ascii_alphanumeric() || control == '_' => {
                (Atom::Unit(u16::from(control as u8) % 32), next + 1)
            }
            // Otherwise the backslash is a member and the `c` is another.
            _ => (Atom::Unit(u16::from(b'\\')), *index + 1),
        },
        Some('x') => match hex(points, next, 2) {
            Some(unit) => (Atom::Unit(unit), next + 2),
            None => (Atom::Unit(u16::from(b'x')), next),
        },
        Some('u') => match hex(points, next, 4) {
            Some(unit) => (Atom::Unit(unit), next + 4),
            None => (Atom::Unit(u16::from(b'u')), next),
        },
        Some('k') if named => return None,
        _ => (Atom::Unit(u16::try_from(escaped).ok()?), next),
    };
    *index = end;
    Some(atom)
}

/// A member of a class as it is written back.
fn push_class_unit(out: &mut Vec<u32>, unit: u16) {
    if unit < 0x80 && b"\\]^-".contains(&(unit as u8)) {
        out.push(BACKSLASH);
    }
    out.push(u32::from(unit));
}

/// The rewrite of the class that opens at `at`; returns the index after it.
fn class(points: &[u32], at: usize, named: bool, out: &mut Vec<u32>) -> Option<usize> {
    let mut index = at + 1;
    let negated = is(points, index, b'^');
    index += usize::from(negated);
    let mut members = UnitSet::new();
    while !is(points, index, b']') {
        let first = class_atom(points, &mut index, named)?;
        let range = is(points, index, b'-')
            && points
                .get(index + 1)
                .is_some_and(|&point| point != u32::from(b']'));
        if !range {
            members.add_atom(first);
            continue;
        }
        index += 1;
        match (first, class_atom(points, &mut index, named)?) {
            (Atom::Unit(low), Atom::Unit(high)) if low <= high => {
                (low..=high).for_each(|unit| members.add(unit));
            }
            // A reversed range is an error.
            (Atom::Unit(_), Atom::Unit(_)) => return None,
            // With a class escape at either end the hyphen is a member.
            (first, second) => {
                members.add_atom(first);
                members.add(u16::from(b'-'));
                members.add_atom(second);
            }
        }
    }
    out.push(u32::from(b'['));
    if negated {
        out.push(u32::from(b'^'));
    }
    for (low, high) in members.canonical_ranges() {
        push_class_unit(out, low);
        if high > low {
            out.push(u32::from(b'-'));
            push_class_unit(out, high);
        }
    }
    out.push(u32::from(b']'));
    Some(index + 1)
}

/// The case-sensitive pattern that matches the canonicalized subject exactly
/// where `points` (one entry per code unit of a pattern with the `i` flag and
/// neither `u` nor `v`) matches the subject; `None` when the pattern uses
/// syntax this rewrite does not model (modifier groups such as `(?i:...)`,
/// which change the flag for part of the pattern while the subject is
/// canonicalized as a whole) or is not valid.
pub(crate) fn rewrite_pattern(points: &[u32]) -> Option<Vec<u32>> {
    let (groups, named) = crate::regex_group_names::capture_group_counts(points);
    let mut out = Vec::with_capacity(points.len());
    let mut index = 0;
    while index < points.len() {
        let point = points[index];
        index = match point {
            BACKSLASH => escape(points, index, groups, named, &mut out)?,
            0x28 => group(points, index, &mut out)?,
            0x5b => class(points, index, named, &mut out)?,
            // `) | ^ $ . * + ? { } ]` are syntax, and a character that is neither
            // syntax nor a letter is not changed by `Canonicalize` anyway.
            0x29 | 0x7c | 0x5e | 0x24 | 0x2e | 0x2a | 0x2b | 0x3f | 0x7b | 0x7d | 0x5d => {
                out.push(point);
                index + 1
            }
            _ if is_decimal(point) => {
                out.push(point);
                index + 1
            }
            _ => {
                push_literal(&mut out, u16::try_from(point).ok()?);
                index + 1
            }
        };
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG_S: u16 = 0x17f;
    const DOTLESS_I: u16 = 0x131;
    const KELVIN: u16 = 0x212a;

    fn units(text: &str) -> Vec<u32> {
        text.encode_utf16().map(u32::from).collect()
    }

    /// The rewrite of `pattern`, as text (a lone surrogate reads as U+FFFD).
    fn rewrite(pattern: &str) -> Option<String> {
        rewrite_pattern(&units(pattern)).map(|points| {
            points
                .into_iter()
                .map(|point| char::from_u32(point).unwrap_or('\u{fffd}'))
                .collect()
        })
    }

    /// The units in a rewritten class.
    fn class_members(rewritten: &str) -> Vec<u16> {
        let body: Vec<u16> = rewritten.encode_utf16().collect();
        assert_eq!(body[0], u16::from(b'['), "{rewritten}");
        let mut index = 1 + usize::from(body[1] == u16::from(b'^'));
        let read = |index: &mut usize| {
            *index += usize::from(body[*index] == u16::from(b'\\'));
            *index += 1;
            body[*index - 1]
        };
        let mut members = Vec::new();
        while body[index] != u16::from(b']') {
            let low = read(&mut index);
            if body[index] == u16::from(b'-') {
                index += 1;
                members.extend(low..=read(&mut index));
            } else {
                members.push(low);
            }
        }
        assert_eq!(index + 1, body.len(), "{rewritten}");
        members
    }

    #[test]
    fn canonicalize_is_the_upper_case_except_into_ascii_or_into_several_units() {
        let cases = [
            (0x61u16, 0x41u16),
            (0x7a, 0x5a),
            (0x41, 0x41),
            (0x30, 0x30),
            (0x7f, 0x7f),
            (0xb5, 0x39c),
            (0xff, 0x178),
            (0xe5, 0xc5),
            (0x1c6, 0x1c4),
            (0x1c5, 0x1c4),
            (0x3c2, 0x3a3),
            (0x3c3, 0x3a3),
            (0x345, 0x399),
            (0x1e9b, 0x1e60),
            // Upper case is ASCII: not folded.
            (LONG_S, LONG_S),
            (DOTLESS_I, DOTLESS_I),
            // Already upper case, or several units: not folded.
            (KELVIN, KELVIN),
            (0x212b, 0x212b),
            (0xdf, 0xdf),
            (0x1e9e, 0x1e9e),
            (0x149, 0x149),
            (0x1fb3, 0x1fb3),
            (0x1fbc, 0x1fbc),
            // A lone surrogate is a unit like any other.
            (0xd800, 0xd800),
            (0xdfff, 0xdfff),
        ];
        for (unit, canonical) in cases {
            assert_eq!(canonicalize(unit), canonical, "{unit:#x}");
        }
    }

    #[test]
    fn canonicalize_is_idempotent_and_never_crosses_the_ascii_boundary() {
        for unit in 0..=u16::MAX {
            let canonical = canonicalize(unit);
            assert_eq!(canonicalize(canonical), canonical, "{unit:#x}");
            assert_eq!(unit < 0x80, canonical < 0x80, "{unit:#x}");
            // Only ASCII letters change in the ASCII range.
            if unit < 0x80 && !(unit as u8).is_ascii_alphabetic() {
                assert_eq!(canonical, unit, "{unit:#x}");
            }
            // White space and line terminators are their own canonical form.
            if CharacterClassEscape::Whitespace.matches(u32::from(unit)) {
                assert_eq!(canonical, unit, "{unit:#x}");
            }
        }
        assert_eq!(canonicalize(u16::from(b'q')), u16::from(b'Q'));
        assert_eq!(canonicalize(u16::from(b'_')), u16::from(b'_'));
    }

    #[test]
    fn a_subject_is_canonicalized_unit_by_unit() {
        let subject: Vec<u16> = "aB\u{17f}\u{1f600}\u{212a}z".encode_utf16().collect();
        let expected: Vec<u16> = "AB\u{17f}\u{1f600}\u{212a}Z".encode_utf16().collect();
        assert_eq!(canonicalize_subject(&subject), expected);
        assert_eq!(canonicalize_subject(&[]), Vec::<u16>::new());
        // A surrogate pair is two units that stay as they are.
        assert_eq!(canonicalize_subject(&[0xd83d, 0xde00]), [0xd83d, 0xde00]);
    }

    #[test]
    fn literals_become_their_canonical_unit() {
        assert_eq!(
            rewrite("sS\u{17f}i\u{131}k\u{212a}").unwrap(),
            "SS\u{17f}I\u{131}K\u{212a}"
        );
        assert_eq!(rewrite("\u{1c6}\u{b5}z").unwrap(), "\u{1c4}\u{39c}Z");
        assert_eq!(rewrite("").unwrap(), "");
        // Digits and other characters that do not change are copied, including the
        // parts of a quantifier and the brace and bracket that are literals.
        assert_eq!(rewrite("a1{2,3}b{,4}c{").unwrap(), "A1{2,3}B{,4}C{");
        assert_eq!(rewrite("]}-,#%").unwrap(), "]}-,#%");
        // The operators are copied.
        assert_eq!(rewrite("(a|b)*?c+?d??^$.").unwrap(), "(A|B)*?C+?D??^$.");
        assert_eq!(
            rewrite("(?:a)(?=b)(?!c)(?<=d)(?<!e)").unwrap(),
            "(?:A)(?=B)(?!C)(?<=D)(?<!E)"
        );
        // A lone surrogate is a unit that does not change.
        assert_eq!(
            rewrite_pattern(&[0xd83d, 0xde00]).unwrap(),
            [0xd83d, 0xde00]
        );
        // A code point above U+FFFF cannot be in such a pattern.
        assert_eq!(rewrite_pattern(&[0x1f600]), None);
    }

    #[test]
    fn group_names_are_copied_as_they_are() {
        assert_eq!(
            rewrite(r"(?<abc>s)\k<abc>(?<x\u0079z>t)").unwrap(),
            r"(?<abc>S)\k<abc>(?<x\u0079z>T)"
        );
        assert_eq!(rewrite("(?<\u{17f}>a)").unwrap(), "(?<\u{17f}>A)");
        // Without a name, or with a modifier, the pattern is left to `regress`.
        assert_eq!(rewrite("(?<n"), None);
        assert_eq!(rewrite("(?<"), None);
        assert_eq!(rewrite("(?"), None);
        assert_eq!(rewrite("(?i:a)"), None);
        assert_eq!(rewrite("(?-i:a)"), None);
        assert_eq!(rewrite("(?x)"), None);
    }

    #[test]
    fn escapes_that_do_not_depend_on_case_are_copied() {
        for escape in [
            r"\d", r"\D", r"\s", r"\S", r"\w", r"\W", r"\b", r"\B", r"\f", r"\n", r"\r", r"\t",
            r"\v", r"\0", r"\cJ", r"\cj", r"\.", r"\*", r"\+", r"\?", r"\(", r"\)", r"\[", r"\]",
            r"\{", r"\}", r"\|", r"\\", r"\^", r"\$", r"\/", r"\-",
        ] {
            assert_eq!(rewrite(escape).unwrap(), escape, "{escape}");
        }
        // Even when a letter follows.
        assert_eq!(rewrite(r"\0s\cJs").unwrap(), r"\0S\cJS");
    }

    #[test]
    fn escapes_that_spell_a_unit_are_canonicalized() {
        assert_eq!(rewrite(r"\x73\x53\xb5\xff").unwrap(), "SS\u{39c}\u{178}");
        assert_eq!(
            rewrite(r"\u0073\u017f\u212A\uD83D\uDE00").unwrap(),
            "S\u{17f}\u{212a}\u{fffd}\u{fffd}"
        );
        // A syntax character stays escaped, and a digit stays a digit.
        assert_eq!(
            rewrite(r"\x2e\x2a\x5c\x28\x7c\x5b\x5d\x7b\x7d\x5e\x24\x3f\x2b\x29").unwrap(),
            r"\.\*\\\(\|\[\]\{\}\^\$\?\+\)"
        );
        assert_eq!(rewrite(r"\x31\u0030").unwrap(), r"\x31\x30");
        assert_eq!(rewrite(r"(a)\1\x30").unwrap(), r"(A)\1\x30");
        // An escape that is not one is the letter that follows the backslash.
        assert_eq!(rewrite(r"\x").unwrap(), "X");
        assert_eq!(rewrite(r"\xg1").unwrap(), "XG1");
        assert_eq!(rewrite(r"\x4").unwrap(), "X4");
        assert_eq!(rewrite(r"\u").unwrap(), "U");
        assert_eq!(rewrite(r"\u00g0").unwrap(), "U00G0");
        assert_eq!(rewrite(r"\u{41}").unwrap(), "U{41}");
        assert_eq!(rewrite(r"\p{Lu}\P").unwrap(), "P{LU}P");
        assert_eq!(rewrite(r"\a\i\_\ \e").unwrap(), "AI_ E");
        assert_eq!(rewrite("\\\u{17f}\\\u{e9}").unwrap(), "\u{17f}\u{c9}");
        assert_eq!(rewrite_pattern(&[0x5c, 0xd83d]).unwrap(), [0xd83d]);
        // `\c` without a letter is a backslash and then a `c`.
        assert_eq!(rewrite(r"\c").unwrap(), r"\\C");
        assert_eq!(rewrite(r"\c1").unwrap(), r"\\C1");
        assert_eq!(rewrite(r"\c_").unwrap(), r"\\C_");
        assert_eq!(rewrite(r"a\cb\c\cA").unwrap(), r"A\cb\\C\cA");
        // A trailing backslash is left to `regress`.
        assert_eq!(rewrite(r"a\"), None);
    }

    #[test]
    fn decimal_escapes_are_references_octal_escapes_or_digits() {
        // A number up to the number of groups is a reference and is copied whole.
        assert_eq!(rewrite(r"(a)\1").unwrap(), r"(A)\1");
        assert_eq!(
            rewrite(r"(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\10s").unwrap(),
            r"(A)(B)(C)(D)(E)(F)(G)(H)(I)(J)\10S"
        );
        assert_eq!(rewrite(r"(?<x>a)(b)\2\1").unwrap(), r"(?<x>A)(B)\2\1");
        // `\8` and `\9` that are not references are the digits.
        assert_eq!(rewrite(r"\8\9").unwrap(), r"\x38\x39");
        assert_eq!(rewrite(r"(a)\81").unwrap(), r"(A)\x381");
        let huge = format!("\\{}", "9".repeat(40));
        assert_eq!(rewrite(&huge).unwrap(), format!("\\x39{}", "9".repeat(39)));
        // The others are legacy octal escapes: one to three digits, at most 255.
        assert_eq!(rewrite(r"\1").unwrap(), "\u{1}");
        assert_eq!(rewrite(r"(a)\2").unwrap(), "(A)\u{2}");
        assert_eq!(rewrite(r"(a)\10").unwrap(), "(A)\u{8}");
        assert_eq!(rewrite(r"(a)\18").unwrap(), "(A)\u{1}8");
        // 0o77 is `?`, a syntax character, so it stays escaped.
        assert_eq!(rewrite(r"\7\77\777").unwrap(), "\u{7}\\?\\?7");
        assert_eq!(rewrite(r"\00").unwrap(), "\u{0}");
        assert_eq!(rewrite(r"\01").unwrap(), "\u{1}");
        assert_eq!(rewrite(r"\08").unwrap(), "\u{0}8");
        assert_eq!(rewrite(r"\012\0123").unwrap(), "\u{a}\u{a}3");
        assert_eq!(rewrite(r"\377\400").unwrap(), "\u{178} 0");
        // A digit that an octal escape produces stays unambiguous, and a letter is folded.
        assert_eq!(rewrite(r"\60\61").unwrap(), r"\x30\x31");
        assert_eq!(rewrite(r"\151\163").unwrap(), "IS");
    }

    #[test]
    fn named_references_are_copied_and_k_is_a_letter_without_names() {
        assert_eq!(rewrite(r"(?<Ab>a)\k<Ab>").unwrap(), r"(?<Ab>A)\k<Ab>");
        assert_eq!(rewrite(r"\k").unwrap(), "K");
        assert_eq!(rewrite(r"\k<a>").unwrap(), "K<A>");
        assert_eq!(rewrite(r"(a)\k").unwrap(), "(A)K");
        assert_eq!(rewrite(r"(?<n>a)\k"), None);
        assert_eq!(rewrite(r"(?<n>a)\k<n"), None);
        assert_eq!(rewrite(r"(?<n>a)[\k]"), None);
    }

    #[test]
    fn a_class_becomes_the_canonical_units_of_its_members() {
        assert_eq!(rewrite("[a-c]").unwrap(), "[A-C]");
        assert_eq!(rewrite("[^a-c]").unwrap(), "[^A-C]");
        assert_eq!(rewrite("[acb]").unwrap(), "[A-C]");
        assert_eq!(rewrite("[aA]").unwrap(), "[A]");
        assert_eq!(rewrite("[]").unwrap(), "[]");
        assert_eq!(rewrite("[^]").unwrap(), "[^]");
        assert_eq!(rewrite("a[s]b").unwrap(), "A[S]B");
        assert_eq!(rewrite("[\u{17f}]").unwrap(), "[\u{17f}]");
        assert_eq!(
            rewrite("[s\u{17f}k\u{212a}]").unwrap(),
            "[KS\u{17f}\u{212a}]"
        );
        assert_eq!(rewrite("[\u{b5}\u{39c}\u{3bc}]").unwrap(), "[\u{39c}]");
        assert_eq!(rewrite("[\u{1fb3}\u{1fbc}]").unwrap(), "[\u{1fb3}\u{1fbc}]");
        // A range holds the canonical units of its members, not the members.
        let latin_extended_a = class_members(&rewrite("[\u{100}-\u{17f}]").unwrap());
        assert!(latin_extended_a.contains(&0x100) && latin_extended_a.contains(&LONG_S));
        assert!(!latin_extended_a.contains(&0x101) && !latin_extended_a.contains(&0x53));
        // Members that are special in a class are escaped again.
        assert_eq!(rewrite(r"[\]\\\^\-]").unwrap(), r"[\-\\-\^]");
        assert_eq!(rewrite(r"[a\-z]").unwrap(), r"[\-AZ]");
        assert_eq!(rewrite("[-a]").unwrap(), r"[\-A]");
        assert_eq!(rewrite("[a-]").unwrap(), r"[\-A]");
        assert_eq!(rewrite("[--a]").unwrap(), r"[\--`]");
        assert_eq!(rewrite("[a-b-c]").unwrap(), r"[\-A-C]");
        assert_eq!(rewrite("[[]").unwrap(), "[[]");
    }

    #[test]
    fn class_escapes_in_a_class_are_expanded_and_canonicalized() {
        let word = class_members(&rewrite(r"[\w]").unwrap());
        assert_eq!(word.len(), 26 + 10 + 1);
        assert!(word.contains(&u16::from(b'S')) && !word.contains(&u16::from(b's')));
        assert!(!word.contains(&LONG_S) && !word.contains(&KELVIN));
        let non_word = class_members(&rewrite(r"[\W]").unwrap());
        assert!(non_word.contains(&LONG_S) && non_word.contains(&KELVIN));
        assert!(!non_word.contains(&u16::from(b's')) && !non_word.contains(&u16::from(b'S')));
        assert!(non_word.iter().all(|&unit| canonicalize(unit) == unit));
        // Every non-word unit is represented by its canonical form.
        for unit in 0..=u16::MAX {
            let is_word = CharacterClassEscape::Word.matches(u32::from(unit));
            assert_eq!(
                non_word.contains(&canonicalize(unit)),
                !is_word,
                "{unit:#x}"
            );
        }
    }

    #[test]
    fn class_atoms_are_read_like_their_pattern_counterparts() {
        let members = |pattern: &str| class_members(&rewrite(pattern).unwrap());
        assert_eq!(members(r"[\b\t\n\v\f\r\0]"), [0, 8, 9, 10, 11, 12, 13]);
        assert_eq!(members(r"[\cJ\ck\cA]"), [1, 10, 11]);
        assert_eq!(
            members(r"[\x41\x7a\u017f\u0073]"),
            [0x41, 0x53, 0x5a, LONG_S]
        );
        assert_eq!(members(r"[\x\u\xg\u00]"), [0x30, 0x47, 0x55, 0x58]);
        assert_eq!(
            members(r"[\8\9\B\p\k\.\a]"),
            [0x2e, 0x38, 0x39, 0x41, 0x42, 0x4b, 0x50]
        );
        assert_eq!(members(r"[\d]"), (0x30..=0x39).collect::<Vec<u16>>());
        assert_eq!(members(r"[\s]").len(), 25);
        // A range with a class escape at either end is a union with a hyphen.
        let digits_hyphen_a: Vec<u16> = [0x2d]
            .into_iter()
            .chain(0x30..=0x39)
            .chain([0x41])
            .collect();
        assert_eq!(members(r"[\d-a]"), digits_hyphen_a);
        assert_eq!(members(r"[a-\d]"), digits_hyphen_a);
        assert_eq!(members(r"[\d-\w]").len(), 26 + 10 + 1 + 1);
        // Legacy octal escapes and `\c` with a digit or an underscore (Annex B).
        assert_eq!(
            members(r"[\1\12\123\377\400]"),
            [1, 10, 32, 0x30, 0x53, 0x178]
        );
        assert_eq!(members(r"[\0a1\01\08]"), [0, 1, 0x31, 0x38, 0x41]);
        assert_eq!(members(r"[\c1\c_\cJ\cj]"), [10, 17, 31]);
        assert_eq!(members(r"[\c]"), [0x43, 0x5c]);
        // Bad syntax is left to `regress`.
        for pattern in ["[", "[a", "[a-", "[^", "[z-a]", r"[\"] {
            assert_eq!(rewrite(pattern), None, "{pattern}");
        }
        assert_eq!(rewrite("[\u{17f}-\u{131}]"), None);
    }
}
