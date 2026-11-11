// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bidirectional text (UAX #9) for inline layout: which embedding level each part of a
//! paragraph is at, and how a line's parts are ordered left to right on the screen.
//!
//! Levels come from the `unicode-bidi` crate over a paragraph string in which every inline piece
//! stands for some characters (a word for its text, a box edge or atomic box for an object
//! replacement character, a forced break for a line separator). Layout then splits words where
//! the level changes and puts each line's parts in visual order with [`visual_order`]. The
//! shaping of what is inside a part (Arabic joining, mirrored brackets, reversing the glyphs of
//! a right-to-left run) is `blueice-font`'s.

use unicode_bidi::{bidi_class, BidiClass, BidiInfo, Level};

/// The direction a paragraph starts from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Base {
    Ltr,
    Rtl,
    /// The first strong character decides (`dir=auto`, `unicode-bidi: plaintext`).
    Auto,
}

/// The characters that open and close an embedding, isolate or override.
pub(crate) const LRE: char = '\u{202A}';
pub(crate) const RLE: char = '\u{202B}';
pub(crate) const PDF: char = '\u{202C}';
pub(crate) const LRO: char = '\u{202D}';
pub(crate) const RLO: char = '\u{202E}';
pub(crate) const LRI: char = '\u{2066}';
pub(crate) const RLI: char = '\u{2067}';
pub(crate) const FSI: char = '\u{2068}';
pub(crate) const PDI: char = '\u{2069}';

/// Whether `text` could be laid out in any order but plain left to right: it holds a letter or
/// number that reads right to left, or a formatting character that changes direction.
pub(crate) fn needs_bidi(text: &str) -> bool {
    text.chars().any(|c| {
        c > '\u{058f}'
            && matches!(
                bidi_class(c),
                BidiClass::R
                    | BidiClass::AL
                    | BidiClass::AN
                    | BidiClass::RLE
                    | BidiClass::RLO
                    | BidiClass::RLI
                    | BidiClass::FSI
                    | BidiClass::LRE
                    | BidiClass::LRO
                    | BidiClass::LRI
            )
    })
}

/// The embedding level of every byte of `text` and of its paragraph.
pub(crate) fn levels(text: &str, base: Base) -> (Vec<u8>, u8) {
    let info = BidiInfo::new(
        text,
        match base {
            Base::Ltr => Some(Level::ltr()),
            Base::Rtl => Some(Level::rtl()),
            Base::Auto => None,
        },
    );
    let paragraph = info.paragraphs.first().map_or(0, |p| p.level.number());
    (info.levels.iter().map(|l| l.number()).collect(), paragraph)
}

/// The order, left to right, in which items of the given embedding `levels` (one per item, in
/// reading order) sit on a line: every run at or above each odd level is reversed, highest
/// level first (rule L2).
pub(crate) fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(&highest) = levels.iter().max() else {
        return order;
    };
    let Some(lowest_odd) = levels.iter().copied().filter(|l| l % 2 == 1).min() else {
        return order;
    };
    for level in (lowest_odd..=highest).rev() {
        let mut i = 0;
        while i < order.len() {
            if levels[order[i]] >= level {
                let start = i;
                while i < order.len() && levels[order[i]] >= level {
                    i += 1;
                }
                order[start..i].reverse();
            } else {
                i += 1;
            }
        }
    }
    order
}

/// Whether `c` only steers the algorithm and is drawn as nothing.
fn is_format_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// A single line of `text` in the order its characters are drawn left to right: each
/// right-to-left run reversed (combining marks staying behind their letter), mirrored characters
/// swapped, and the formatting characters dropped. For text drawn outside the page flow, like a
/// canvas, where there is no layout to place the runs.
pub fn visual_text(text: &str, rtl: bool) -> String {
    if !rtl && !needs_bidi(text) {
        return text.to_string();
    }
    let (byte_levels, _) = levels(text, if rtl { Base::Rtl } else { Base::Ltr });
    // A cluster is a character with the marks that follow it.
    let mut clusters: Vec<(String, u8)> = Vec::new();
    for (offset, c) in text.char_indices() {
        if is_format_control(c) {
            continue;
        }
        let level = byte_levels.get(offset).copied().unwrap_or(0);
        match clusters.last_mut() {
            Some((last, _)) if blueice_font::is_mark(c) => last.push(c),
            _ => clusters.push((c.to_string(), level)),
        }
    }
    let order = visual_order(&clusters.iter().map(|(_, l)| *l).collect::<Vec<_>>());
    let mut out = String::with_capacity(text.len());
    for i in order {
        let (cluster, level) = &clusters[i];
        for (k, c) in cluster.chars().enumerate() {
            out.push(if k == 0 && level % 2 == 1 { blueice_font::mirrored(c) } else { c });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_text_reverses_right_to_left_runs_only() {
        assert_eq!(visual_text("abc", false), "abc");
        assert_eq!(visual_text("\u{5d0}\u{5d1}", false), "\u{5d1}\u{5d0}");
        assert_eq!(
            visual_text("ab \u{5d0}\u{5d1} cd", false),
            "ab \u{5d1}\u{5d0} cd"
        );
    }

    #[test]
    fn visual_text_in_a_right_to_left_paragraph_reads_from_the_right() {
        // "abc" then Hebrew: abc is read first, so it ends up on the right
        assert_eq!(visual_text("abc \u{5d0}\u{5d1}", true), "\u{5d1}\u{5d0} abc");
        assert_eq!(visual_text("abc", true), "abc");
        // numbers keep their own order inside it
        assert_eq!(visual_text("\u{5d0} 12", true), "12 \u{5d0}");
    }

    #[test]
    fn visual_text_keeps_marks_on_their_letters_and_drops_formatting_characters() {
        assert_eq!(visual_text("\u{5d0}\u{5b0}\u{5d1}", false), "\u{5d1}\u{5d0}\u{5b0}");
        assert_eq!(visual_text("a\u{200f}b", false), "ab");
    }

    #[test]
    fn visual_text_mirrors_brackets_in_right_to_left_runs() {
        assert_eq!(visual_text("\u{5d0}(\u{5d1})", false), "(\u{5d1})\u{5d0}");
    }

    #[test]
    fn plain_latin_never_needs_the_algorithm() {
        assert!(!needs_bidi("Hello, world 123 — ✓ 你好"));
        assert!(needs_bidi("abc \u{5d0}"));
        assert!(needs_bidi("\u{627}"));
        assert!(needs_bidi("a\u{2067}b"));
    }

    #[test]
    fn levels_follow_the_script_of_each_character() {
        let text = "ab \u{5d0}\u{5d1} cd";
        let (levels, paragraph) = levels(text, Base::Ltr);
        assert_eq!(paragraph, 0);
        assert_eq!(levels[0], 0);
        assert_eq!(levels[text.find('\u{5d0}').unwrap()], 1);
        assert_eq!(levels[text.len() - 1], 0);
        let (_, auto) = super::levels("\u{5d0} ab", Base::Auto);
        assert_eq!(auto, 1);
        let (_, auto) = super::levels("ab \u{5d0}", Base::Auto);
        assert_eq!(auto, 0);
    }

    #[test]
    fn a_right_to_left_paragraph_puts_numbers_one_level_up() {
        let text = "\u{5d0} 12 \u{5d1}";
        let (levels, paragraph) = levels(text, Base::Rtl);
        assert_eq!(paragraph, 1);
        assert_eq!(levels[text.find('1').unwrap()], 2);
        assert_eq!(levels[0], 1);
    }

    #[test]
    fn visual_order_reverses_each_run_from_the_deepest_level_out() {
        assert_eq!(visual_order(&[]), Vec::<usize>::new());
        assert_eq!(visual_order(&[0, 0, 0]), [0, 1, 2]);
        assert_eq!(visual_order(&[1, 1, 1]), [2, 1, 0]);
        assert_eq!(visual_order(&[0, 1, 1, 0]), [0, 2, 1, 3]);
        // a right-to-left paragraph holding a left-to-right number: 2 stays in order inside
        assert_eq!(visual_order(&[1, 2, 2, 1]), [3, 1, 2, 0]);
        assert_eq!(visual_order(&[0, 1, 2, 2, 1, 0]), [0, 4, 2, 3, 1, 5]);
    }
}
