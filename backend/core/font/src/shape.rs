// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The little shaping right-to-left scripts need before their characters can be measured and
//! drawn one at a time: Arabic letters join their neighbours, so each takes an isolated, final,
//! initial or medial form; paired punctuation is drawn mirrored in right-to-left text; and a run
//! drawn right to left is laid out in reverse, base characters keeping their combining marks.
//!
//! The joining forms are the Arabic Presentation Forms code points, which the bundled face
//! carries; a form the face lacks leaves the plain letter. There is no OpenType layout engine
//! here, so letters outside the table keep their base shape and Arabic ligatures other than
//! lam-alef are not formed.

use std::borrow::Cow;

/// `(base letter, isolated form, joins on both sides)`. A form's final shape is the next code
/// point after the isolated one; an initial shape and a medial one follow when it joins on both
/// sides.
const LETTERS: &[(char, u32, bool)] = &[
    ('\u{0621}', 0xFE80, false),
    ('\u{0622}', 0xFE81, false),
    ('\u{0623}', 0xFE83, false),
    ('\u{0624}', 0xFE85, false),
    ('\u{0625}', 0xFE87, false),
    ('\u{0626}', 0xFE89, true),
    ('\u{0627}', 0xFE8D, false),
    ('\u{0628}', 0xFE8F, true),
    ('\u{0629}', 0xFE93, false),
    ('\u{062A}', 0xFE95, true),
    ('\u{062B}', 0xFE99, true),
    ('\u{062C}', 0xFE9D, true),
    ('\u{062D}', 0xFEA1, true),
    ('\u{062E}', 0xFEA5, true),
    ('\u{062F}', 0xFEA9, false),
    ('\u{0630}', 0xFEAB, false),
    ('\u{0631}', 0xFEAD, false),
    ('\u{0632}', 0xFEAF, false),
    ('\u{0633}', 0xFEB1, true),
    ('\u{0634}', 0xFEB5, true),
    ('\u{0635}', 0xFEB9, true),
    ('\u{0636}', 0xFEBD, true),
    ('\u{0637}', 0xFEC1, true),
    ('\u{0638}', 0xFEC5, true),
    ('\u{0639}', 0xFEC9, true),
    ('\u{063A}', 0xFECD, true),
    ('\u{0641}', 0xFED1, true),
    ('\u{0642}', 0xFED5, true),
    ('\u{0643}', 0xFED9, true),
    ('\u{0644}', 0xFEDD, true),
    ('\u{0645}', 0xFEE1, true),
    ('\u{0646}', 0xFEE5, true),
    ('\u{0647}', 0xFEE9, true),
    ('\u{0648}', 0xFEED, false),
    ('\u{0649}', 0xFEEF, false),
    ('\u{064A}', 0xFEF1, true),
    // Persian and Urdu letters
    ('\u{067E}', 0xFB56, true),
    ('\u{0686}', 0xFB7A, true),
    ('\u{0698}', 0xFB8A, false),
    ('\u{06A9}', 0xFB8E, true),
    ('\u{06AF}', 0xFB92, true),
    ('\u{06CC}', 0xFBFC, true),
];

/// Lam followed by an alef joins into one glyph: `(alef, isolated ligature)`, the final form
/// being the next code point.
const LAM_ALEF: &[(char, u32)] = &[
    ('\u{0622}', 0xFEF5),
    ('\u{0623}', 0xFEF7),
    ('\u{0625}', 0xFEF9),
    ('\u{0627}', 0xFEFB),
];

#[derive(Clone, Copy, PartialEq)]
enum Joining {
    /// Joins on neither side (every non-Arabic character).
    None,
    /// Joins to the letter before it only.
    Right,
    Dual,
    /// A combining mark: it neither joins nor interrupts a join.
    Transparent,
}

fn joining(c: char) -> Joining {
    if c == '\u{0640}' {
        return Joining::Dual; // tatweel
    }
    if let Some((_, _, dual)) = LETTERS.iter().find(|(b, _, _)| *b == c) {
        return if *dual { Joining::Dual } else { Joining::Right };
    }
    match c {
        '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{06D6}'..='\u{06DC}' | '\u{06DF}'..='\u{06E4}'
        | '\u{06E7}' | '\u{06E8}' | '\u{06EA}'..='\u{06ED}' => Joining::Transparent,
        _ => Joining::None,
    }
}

fn form(base: char, offset: u32) -> Option<char> {
    let (_, iso, dual) = LETTERS.iter().find(|(b, _, _)| *b == base)?;
    if offset >= 2 && !dual {
        return None;
    }
    char::from_u32(iso + offset)
}

/// Whether `text` has a character [`shape`] could change.
fn needs_shaping(text: &str) -> bool {
    text.chars().any(|c| ('\u{0621}'..='\u{06D3}').contains(&c))
}

/// `text` with every Arabic letter in the form its neighbours call for, and lam-alef pairs
/// joined into their one glyph. `has_glyph` says whether a form can be drawn; a letter whose form
/// cannot is left as it is. Text without Arabic comes back untouched.
pub fn shape_with(text: &str, has_glyph: impl Fn(char) -> bool) -> Cow<'_, str> {
    if !needs_shaping(text) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(shape_with_map(text, has_glyph).0)
}

/// [`shape_with`], also saying which character of `text` each character of the result stands for
/// (a lam-alef ligature stands for the lam; the alef is gone).
pub fn shape_with_map(text: &str, has_glyph: impl Fn(char) -> bool) -> (String, Vec<usize>) {
    let chars: Vec<char> = text.chars().collect();
    let mut map: Vec<usize> = Vec::with_capacity(chars.len());
    // The nearest non-transparent neighbour on each side.
    let before = |i: usize| chars[..i].iter().rev().map(|c| joining(*c)).find(|j| *j != Joining::Transparent);
    let after = |i: usize| chars[i + 1..].iter().map(|c| joining(*c)).find(|j| *j != Joining::Transparent);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let kind = joining(c);
        if !matches!(kind, Joining::Right | Joining::Dual) {
            out.push(c);
            map.push(i);
            i += 1;
            continue;
        }
        // Joins a letter before it that reaches forward (dual joining).
        let joins_prev = matches!(before(i), Some(Joining::Dual));
        // Lam + alef (marks between them are kept after the ligature).
        if c == '\u{0644}' {
            let next = chars[i + 1..].iter().position(|c| joining(*c) != Joining::Transparent);
            if let Some(offset) = next {
                let alef = chars[i + 1 + offset];
                if let Some((_, iso)) = LAM_ALEF.iter().find(|(a, _)| *a == alef) {
                    let glyph = char::from_u32(iso + u32::from(joins_prev)).filter(|g| has_glyph(*g));
                    if let Some(glyph) = glyph {
                        out.push(glyph);
                        map.push(i);
                        out.extend(&chars[i + 1..i + 1 + offset]);
                        map.extend(i + 1..i + 1 + offset);
                        i += offset + 2;
                        continue;
                    }
                }
            }
        }
        let joins_next = kind == Joining::Dual && matches!(after(i), Some(Joining::Right | Joining::Dual));
        let offset = match (joins_prev, joins_next) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        };
        out.push(form(c, offset).filter(|f| has_glyph(*f)).unwrap_or(c));
        map.push(i);
        i += 1;
    }
    (out, map)
}

/// The character a right-to-left run draws in place of `c`: a bracket or quote faces the other
/// way. Anything else is itself.
pub fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '\u{00AB}' => '\u{00BB}',
        '\u{00BB}' => '\u{00AB}',
        '\u{2039}' => '\u{203A}',
        '\u{203A}' => '\u{2039}',
        '\u{2264}' => '\u{2265}',
        '\u{2265}' => '\u{2264}',
        '\u{2018}' => '\u{2019}',
        _ => c,
    }
}

/// The presentation form a character takes when it stands upright in a vertical line: brackets
/// and corner marks turn to face down the line, dashes and ellipses stand on end, and the
/// ideographic commas and stops move to the top right of their cell. A character without one is
/// itself.
pub fn vertical_form(c: char) -> char {
    match c {
        '\u{FF0C}' => '\u{FE10}',
        '\u{3001}' => '\u{FE11}',
        '\u{3002}' => '\u{FE12}',
        '\u{FF1A}' => '\u{FE13}',
        '\u{FF1B}' => '\u{FE14}',
        '\u{FF01}' => '\u{FE15}',
        '\u{FF1F}' => '\u{FE16}',
        '\u{2026}' => '\u{FE19}',
        '\u{2014}' => '\u{FE31}',
        '\u{2013}' => '\u{FE32}',
        '\u{FF08}' => '\u{FE35}',
        '\u{FF09}' => '\u{FE36}',
        '\u{FF5B}' => '\u{FE37}',
        '\u{FF5D}' => '\u{FE38}',
        '\u{3014}' => '\u{FE39}',
        '\u{3015}' => '\u{FE3A}',
        '\u{3010}' => '\u{FE3B}',
        '\u{3011}' => '\u{FE3C}',
        '\u{300A}' => '\u{FE3D}',
        '\u{300B}' => '\u{FE3E}',
        '\u{3008}' => '\u{FE3F}',
        '\u{3009}' => '\u{FE40}',
        '\u{300C}' => '\u{FE41}',
        '\u{300D}' => '\u{FE42}',
        '\u{300E}' => '\u{FE43}',
        '\u{300F}' => '\u{FE44}',
        '\u{FF3B}' => '\u{FE47}',
        '\u{FF3D}' => '\u{FE48}',
        _ => c,
    }
}

/// Whether `c` is a combining mark that stays with the character before it.
pub fn is_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}' | '\u{0591}'..='\u{05BD}' | '\u{05BF}' | '\u{05C1}'
        | '\u{05C2}' | '\u{05C4}' | '\u{05C5}' | '\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}'
        | '\u{06D6}'..='\u{06DC}' | '\u{06DF}'..='\u{06E4}' | '\u{06E7}' | '\u{06E8}'
        | '\u{06EA}'..='\u{06ED}')
}

/// `text` in the order a right-to-left run is drawn left to right: its characters reversed, each
/// base character still followed by its combining marks, and mirrored characters swapped.
pub fn visual_order(text: &str) -> String {
    let mut clusters: Vec<String> = Vec::new();
    for c in text.chars() {
        match clusters.last_mut() {
            Some(last) if is_mark(c) => last.push(c),
            _ => clusters.push(mirrored(c).to_string()),
        }
    }
    clusters.iter().rev().flat_map(|c| c.chars()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: char) -> bool {
        true
    }

    fn shaped(text: &str) -> Vec<u32> {
        shape_with(text, all).chars().map(|c| c as u32).collect()
    }

    #[test]
    fn text_without_arabic_is_left_alone() {
        assert!(matches!(shape_with("hello שלום", all), Cow::Borrowed(_)));
    }

    #[test]
    fn a_letter_alone_is_isolated_and_joined_letters_take_initial_medial_and_final_forms() {
        // beh alone
        assert_eq!(shaped("\u{0628}"), [0xFE8F]);
        // beh beh beh: initial, medial, final
        assert_eq!(shaped("\u{0628}\u{0628}\u{0628}"), [0xFE91, 0xFE92, 0xFE90]);
    }

    #[test]
    fn a_right_joining_letter_does_not_reach_the_next_letter() {
        // dal beh: dal cannot join forward, so beh starts a new word shape
        assert_eq!(shaped("\u{062F}\u{0628}"), [0xFEA9, 0xFE8F]);
        // beh dal: beh initial, dal final
        assert_eq!(shaped("\u{0628}\u{062F}"), [0xFE91, 0xFEAA]);
    }

    #[test]
    fn marks_between_letters_do_not_break_the_join() {
        // beh fatha beh -> initial, fatha, final
        assert_eq!(shaped("\u{0628}\u{064E}\u{0628}"), [0xFE91, 0x064E, 0xFE90]);
    }

    #[test]
    fn non_arabic_neighbours_break_joins() {
        assert_eq!(shaped("a\u{0628}b"), [0x61, 0xFE8F, 0x62]);
    }

    #[test]
    fn lam_followed_by_alef_is_one_ligature_glyph() {
        assert_eq!(shaped("\u{0644}\u{0627}"), [0xFEFB]);
        // joined to a letter before: the final form of the ligature
        assert_eq!(shaped("\u{0628}\u{0644}\u{0627}"), [0xFE91, 0xFEFC]);
    }

    #[test]
    fn a_form_the_face_lacks_leaves_the_plain_letter() {
        let shaped: String = shape_with("\u{0628}\u{0628}", |c| c != '\u{FE91}').into_owned();
        assert_eq!(shaped.chars().next(), Some('\u{0628}'));
    }

    #[test]
    fn every_form_in_the_table_is_the_presentation_form_of_its_letter() {
        use unicode_normalization::UnicodeNormalization;
        let nfkc = |c: u32| char::from_u32(c).unwrap().to_string().nfkc().collect::<String>();
        for (base, iso, dual) in LETTERS {
            // Hamza alone has only its isolated form.
            let count = if *dual { 4 } else if *base == '\u{0621}' { 1 } else { 2 };
            for offset in 0..count {
                assert_eq!(nfkc(iso + offset), base.to_string(), "U+{:04X}", iso + offset);
            }
        }
        for (alef, iso) in LAM_ALEF {
            for offset in 0..2 {
                assert_eq!(nfkc(iso + offset), format!("\u{0644}{alef}"));
            }
        }
    }

    #[test]
    fn visual_order_reverses_characters_but_keeps_marks_on_their_base() {
        assert_eq!(visual_order("abc"), "cba");
        assert_eq!(visual_order("a\u{064E}b"), "ba\u{064E}");
    }

    #[test]
    fn upright_punctuation_has_a_vertical_form_and_other_characters_keep_theirs() {
        assert_eq!(vertical_form('\u{300C}'), '\u{FE41}');
        assert_eq!(vertical_form('\u{3002}'), '\u{FE12}');
        assert_eq!(vertical_form('\u{65e5}'), '\u{65e5}');
        assert_eq!(vertical_form('a'), 'a');
    }

    #[test]
    fn every_vertical_form_is_the_compatibility_form_of_its_character() {
        use unicode_normalization::UnicodeNormalization;
        for c in "\u{FF0C}\u{3001}\u{3002}\u{FF1A}\u{FF1B}\u{FF01}\u{FF1F}\u{2026}\u{2014}\u{2013}\u{FF08}\u{FF09}\u{FF5B}\u{FF5D}\u{3014}\u{3015}\u{3010}\u{3011}\u{300A}\u{300B}\u{3008}\u{3009}\u{300C}\u{300D}\u{300E}\u{300F}\u{FF3B}\u{FF3D}".chars() {
            let form = vertical_form(c);
            assert_ne!(form, c);
            // the form decomposes to the plain character, or (for the fullwidth ones) to its
            // halfwidth twin, so they are the same mark in another orientation
            let plain: String = c.to_string().nfkc().collect();
            let from_form: String = form.to_string().nfkc().collect();
            assert_eq!(plain, from_form, "U+{:04X} -> U+{:04X}", c as u32, form as u32);
        }
    }

    #[test]
    fn visual_order_mirrors_brackets() {
        assert_eq!(visual_order("(a)"), "(a)");
        assert_eq!(visual_order("<a"), "a>");
    }

    #[test]
    fn the_bundled_face_has_the_forms_the_table_uses() {
        let face = crate::font_for(false, false);
        for (_, iso, dual) in LETTERS.iter().filter(|(b, _, _)| *b < '\u{0670}') {
            let count = if *dual { 4 } else { 2 };
            for offset in 0..count {
                let c = char::from_u32(iso + offset).unwrap();
                assert!(face.has_glyph(c) || c == '\u{FE81}', "missing U+{:04X}", c as u32);
            }
        }
    }
}
