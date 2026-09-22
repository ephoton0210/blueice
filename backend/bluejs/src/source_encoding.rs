// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source text that carries ill-formed UTF-16.
//!
//! ECMAScript source text is a sequence of UTF-16 code units (§11), and the
//! strings `eval`, `Function`, indirect eval and `ShadowRealm.prototype.evaluate`
//! compile may contain unpaired surrogates. They are legal inside comments,
//! string and template literals and regular-expression literals, and a syntax
//! error only where an identifier or punctuator is required. The tokenizer
//! and parser work on Rust text, which cannot hold a surrogate, so a source
//! string crosses into them in a lossless *lexer encoding*:
//!
//! * a lone surrogate `U+D800..=U+DFFF` becomes the single private-use
//!   character `U+10F000 + (unit - 0xD800)` (`U+10F000..=U+10F7FF`);
//! * [`ESCAPE`] (`U+10F800`) makes the one character after it literal, and
//!   precedes every *genuine* occurrence of a character in
//!   `U+10F000..=U+10F800`, so the encoding is a bijection on all UTF-16
//!   sequences and never confuses a real private-use character with a lone
//!   surrogate;
//! * every other character stands for itself, so source without a lone
//!   surrogate and without a character in that range is byte-for-byte
//!   unchanged (the common case costs one scan for the range's lead byte).
//!
//! Encoded characters are private-use characters (neither `ID_Start`,
//! `ID_Continue`, whitespace nor a line terminator), so the lexer treats each
//! as a UTF-16 engine treats the surrogate it stands for: skipped inside a
//! comment, an error where an identifier or punctuator must be. They are
//! decoded back to code units wherever source text turns into a JavaScript
//! value: cooked string and template values, template raw strings,
//! regular-expression literal patterns and `Function.prototype.toString`.

use crate::JsString;
use std::borrow::Cow;

/// First character of the block that stands for the surrogate `U+D800`.
const LONE_SURROGATE_BASE: u32 = 0x10_F000;
/// Number of surrogate code points, `U+D800..=U+DFFF`.
const SURROGATE_COUNT: u32 = 0x800;
/// Makes the character after it stand for itself.
pub(crate) const ESCAPE: char = '\u{10F800}';
/// The lead byte of every UTF-8 sequence for `U+100000..=U+10FFFF`, the
/// planes containing the reserved block: text without it needs no escaping.
const RESERVED_LEAD_BYTE: u8 = 0xF4;

/// The first code point of the reserved block; every character below it is an
/// ordinary source character, which is the tokenizer's fast path.
pub(crate) const RESERVED_START: u32 = LONE_SURROGATE_BASE;

fn is_reserved(character: char) -> bool {
    (RESERVED_START..=ESCAPE as u32).contains(&(character as u32))
}

fn encode_lone_surrogate(unit: u16) -> char {
    debug_assert!((0xD800..=0xDFFF).contains(&unit));
    char::from_u32(LONE_SURROGATE_BASE + u32::from(unit) - 0xD800)
        .expect("the reserved block holds valid scalar values")
}

/// The lone surrogate an encoded character stands for, if it is one.
pub(crate) fn lone_surrogate(character: char) -> Option<u16> {
    let offset = (character as u32).checked_sub(LONE_SURROGATE_BASE)?;
    (offset < SURROGATE_COUNT).then(|| (0xD800 + offset) as u16)
}

/// Encodes a JavaScript string as lexer text. Well-formed text without a
/// character in the reserved block is copied unchanged.
pub(crate) fn encode(source: &JsString) -> String {
    let units = source.as_code_units();
    let mut text = String::with_capacity(units.len());
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) if is_reserved(character) => {
                text.push(ESCAPE);
                text.push(character);
            }
            Ok(character) => text.push(character),
            Err(error) => text.push(encode_lone_surrogate(error.unpaired_surrogate())),
        }
    }
    text
}

/// Turns host text (valid Unicode, so no lone surrogate) into lexer text by
/// escaping any character that would otherwise read as an encoded one.
pub(crate) fn escape(text: &str) -> Cow<'_, str> {
    if !text.as_bytes().contains(&RESERVED_LEAD_BYTE) || !text.chars().any(is_reserved) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 4);
    for character in text.chars() {
        if is_reserved(character) {
            escaped.push(ESCAPE);
        }
        escaped.push(character);
    }
    Cow::Owned(escaped)
}

/// Decodes lexer text back to the JavaScript string it encodes.
pub(crate) fn decode(text: &str) -> JsString {
    if !text.as_bytes().contains(&RESERVED_LEAD_BYTE) {
        return JsString::from(text);
    }
    let mut units = JsString::default();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        units.push_code_point(decode_character(character, &mut characters));
    }
    units
}

/// The code point (a surrogate code point for an encoded lone surrogate) that
/// `character` stands for, taking the escaped character from `rest`.
pub(crate) fn decode_character(character: char, rest: &mut std::str::Chars<'_>) -> u32 {
    if character == ESCAPE {
        return rest.next().map_or(character as u32, |next| next as u32);
    }
    lone_surrogate(character).map_or(character as u32, u32::from)
}

/// A code point (or lone surrogate) for an error message: a surrogate is
/// spelled `\uXXXX`, so no private-use encoding leaks into what a script reads.
pub(crate) fn describe_code_point(code: u32) -> String {
    match char::from_u32(code) {
        Some(character) => character.to_string(),
        None => format!("\\u{code:04X}"),
    }
}

/// Lexer text for an error message; see [`describe_code_point`].
pub(crate) fn describe(text: &str) -> String {
    let mut described = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        described.push_str(&describe_code_point(decode_character(
            character,
            &mut characters,
        )));
    }
    described
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(units: &[u16]) -> JsString {
        JsString::from_code_units(units.to_vec())
    }

    #[test]
    fn well_formed_text_without_reserved_characters_is_unchanged() {
        for text in ["", "a + b", "é\u{1F438}\u{10EFFF}\u{10F801}\u{10FFFF}"] {
            let source = JsString::from(text);
            assert_eq!(encode(&source), text);
            assert!(matches!(escape(text), Cow::Borrowed(_)), "{text:?}");
            assert_eq!(decode(text), source);
        }
    }

    #[test]
    fn a_lone_surrogate_becomes_one_reserved_character_and_back() {
        for unit in [0xD800, 0xD83D, 0xDBFF, 0xDC00, 0xDC38, 0xDFFF] {
            let source = units(&[0x61, unit, 0x62]);
            let text = encode(&source);
            let mut characters = text.chars();
            assert_eq!(characters.next(), Some('a'));
            assert_eq!(lone_surrogate(characters.next().unwrap()), Some(unit));
            assert_eq!(characters.next(), Some('b'));
            assert_eq!(decode(&text), source);
        }
    }

    #[test]
    fn a_surrogate_pair_is_one_ordinary_character() {
        let source = units(&[0xD83D, 0xDC38]);
        assert_eq!(encode(&source), "\u{1F438}");
        // A lead followed by a lead, and a trail followed by a lead, are not pairs.
        for pair in [[0xD83D, 0xD83D], [0xDC38, 0xD83D], [0xDC38, 0xDC38]] {
            let source = units(&pair);
            let text = encode(&source);
            assert_eq!(text.chars().count(), 2, "{pair:?}");
            assert_eq!(decode(&text), source);
        }
    }

    #[test]
    fn genuine_reserved_characters_are_escaped_and_never_read_as_surrogates() {
        for code in [0x10F000, 0x10F001, 0x10F7FF, 0x10F800] {
            let character = char::from_u32(code).unwrap();
            let source = JsString::from(character.to_string());
            let text = encode(&source);
            assert_eq!(text, format!("{ESCAPE}{character}"), "{code:X}");
            assert_eq!(escape(&character.to_string()), text);
            assert_eq!(decode(&text), source);
            assert_eq!(
                lone_surrogate(character),
                (code < 0x10F800).then_some(0xD800 + (code - 0x10F000) as u16)
            );
        }
    }

    #[test]
    fn every_kind_of_neighbour_round_trips() {
        let mut sequences = Vec::new();
        let pieces: [&[u16]; 8] = [
            &[0x41],
            &[0xD800],
            &[0xDC00],
            &[0xD83D, 0xDC38],
            &[0xDBFC, 0xDC00], // U+10F000 as a pair
            &[0xDBFE, 0xDC00], // U+10F800 as a pair
            &[0xDBFF, 0xDFFF], // U+10FFFF
            &[0xDBFC, 0xDFFF], // U+10F3FF: outside the block
        ];
        for first in pieces {
            for second in pieces {
                for third in pieces {
                    sequences.push([first, second, third].concat());
                }
            }
        }
        for sequence in sequences {
            let source = units(&sequence);
            assert_eq!(decode(&encode(&source)), source, "{sequence:X?}");
        }
    }

    #[test]
    fn escape_agrees_with_encode_for_well_formed_text() {
        for text in [
            "a\u{10F000}b",
            "\u{10F800}\u{10F800}",
            "\u{10F7FF}\u{10F801}x",
        ] {
            assert_eq!(escape(text), encode(&JsString::from(text)), "{text:?}");
        }
    }

    #[test]
    fn a_truncated_escape_decodes_to_itself() {
        assert_eq!(
            decode(&ESCAPE.to_string()),
            JsString::from(ESCAPE.to_string())
        );
    }

    #[test]
    fn descriptions_spell_lone_surrogates_and_hide_the_encoding() {
        let text = encode(&units(&[0x61, 0xD800, 0x62, 0xDFFF]));
        assert_eq!(describe(&text), "a\\uD800b\\uDFFF");
        assert_eq!(
            describe(&encode(&JsString::from("\u{10F000}"))),
            "\u{10F000}"
        );
    }
}
