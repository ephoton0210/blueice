// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS value types and single-component parsing, per
//! `phase-2-mvp-scope/PLAN.md`'s "MVP CSS scope": keywords, lengths
//! (`px`, `em`, unitless `0`), percentages, plain numbers (`flex-grow`,
//! unitless `line-height`, numeric `font-weight`), and colors (a named
//! subset, `#rgb`/`#rrggbb` hex, and `currentColor`).
//!
//! This module parses *one* value component at a time (the tokens
//! between two commas, or the whole declaration value when there's no
//! comma) -- splitting a declaration's value into components (for
//! `margin: 1px 2px` or `font-family: a, b`) is `parser.rs`'s job, since
//! how a value is split is property-specific (space list vs. comma
//! list vs. shorthand), not something a single generic value parser can
//! decide on its own.

use crate::tokenizer::Token;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f64),
    Em(f64),
    Zero,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Rgba(u8, u8, u8, u8),
    CurrentColor,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Any bare identifier this module doesn't give special meaning to
    /// -- `block`, `none`, `bold`, `auto`, `static`, a font family name,
    /// ... -- left as a lowercased string for the cascade/property
    /// layer to interpret per-property.
    Keyword(String),
    Length(Length),
    Percentage(f64),
    Number(f64),
    Color(Color),
}

const NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("black", (0, 0, 0)),
    ("white", (255, 255, 255)),
    ("red", (255, 0, 0)),
    ("green", (0, 128, 0)),
    ("blue", (0, 0, 255)),
    ("yellow", (255, 255, 0)),
    ("gray", (128, 128, 128)),
    ("grey", (128, 128, 128)),
    ("silver", (192, 192, 192)),
    ("orange", (255, 165, 0)),
    ("purple", (128, 0, 128)),
    ("transparent", (0, 0, 0)),
];

fn parse_hex_color(hex: &str) -> Option<Color> {
    let expand = |c: char| -> Option<u8> {
        let s: String = [c, c].iter().collect();
        u8::from_str_radix(&s, 16).ok()
    };
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let r = expand(chars.next()?)?;
            let g = expand(chars.next()?)?;
            let b = expand(chars.next()?)?;
            Some(Color::Rgba(r, g, b, 255))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgba(r, g, b, 255))
        }
        _ => None,
    }
}

/// Parses a single value component (already comma/space-split by the
/// caller) from its tokens. Leading/trailing [`Token::Whitespace`] is
/// ignored; internal whitespace between multiple tokens is treated as
/// invalid for MVP (every supported value shape is exactly one token).
pub fn parse_value(tokens: &[Token]) -> Option<Value> {
    let meaningful: Vec<&Token> = tokens.iter().filter(|t| **t != Token::Whitespace).collect();
    let [tok] = meaningful[..] else { return None };

    match tok {
        Token::Ident(name) => {
            let lower = name.to_ascii_lowercase();
            if lower == "currentcolor" {
                return Some(Value::Color(Color::CurrentColor));
            }
            if let Some((_, (r, g, b))) = NAMED_COLORS.iter().find(|(n, _)| *n == lower) {
                return Some(Value::Color(Color::Rgba(*r, *g, *b, 255)));
            }
            Some(Value::Keyword(lower))
        }
        Token::Hash(hex) => parse_hex_color(hex).map(Value::Color),
        Token::Number(n) => Some(Value::Number(*n)),
        Token::Percentage(n) => Some(Value::Percentage(*n)),
        Token::Dimension(n, unit) => match unit.as_str() {
            "px" => Some(Value::Length(Length::Px(*n))),
            "em" => Some(Value::Length(Length::Em(*n))),
            _ => None,
        },
        _ => None,
    }
}

/// `0` is a valid length with no unit -- the one numeric special case
/// every CSS value grammar carves out. Call this before falling back to
/// [`parse_value`] wherever a length is expected but a plain `Number`
/// token should still be accepted as `0`.
pub fn zero_as_length(value: &Value) -> Option<Length> {
    match value {
        Value::Number(n) if *n == 0.0 => Some(Length::Zero),
        Value::Length(l) => Some(*l),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn value_of(css: &str) -> Option<Value> {
        parse_value(&tokenize(css))
    }

    #[test]
    fn keyword() {
        assert_eq!(value_of("block"), Some(Value::Keyword("block".to_string())));
        assert_eq!(
            value_of("Block"),
            Some(Value::Keyword("block".to_string())),
            "keywords are case-insensitive"
        );
    }

    #[test]
    fn length_px_and_em() {
        assert_eq!(value_of("12px"), Some(Value::Length(Length::Px(12.0))));
        assert_eq!(value_of("1.5em"), Some(Value::Length(Length::Em(1.5))));
        assert_eq!(value_of("-3px"), Some(Value::Length(Length::Px(-3.0))));
    }

    #[test]
    fn unsupported_dimension_unit_is_none() {
        assert_eq!(value_of("3vh"), None);
    }

    #[test]
    fn percentage() {
        assert_eq!(value_of("50%"), Some(Value::Percentage(50.0)));
    }

    #[test]
    fn bare_number() {
        assert_eq!(value_of("2"), Some(Value::Number(2.0)));
        assert_eq!(value_of("1.5"), Some(Value::Number(1.5)));
    }

    #[test]
    fn zero_as_length_accepts_bare_zero_number() {
        assert_eq!(zero_as_length(&value_of("0").unwrap()), Some(Length::Zero));
        assert_eq!(
            zero_as_length(&value_of("5px").unwrap()),
            Some(Length::Px(5.0))
        );
        assert_eq!(zero_as_length(&value_of("block").unwrap()), None);
    }

    #[test]
    fn named_colors() {
        assert_eq!(
            value_of("red"),
            Some(Value::Color(Color::Rgba(255, 0, 0, 255)))
        );
        assert_eq!(
            value_of("BLUE"),
            Some(Value::Color(Color::Rgba(0, 0, 255, 255)))
        );
    }

    #[test]
    fn current_color_keyword() {
        assert_eq!(
            value_of("currentColor"),
            Some(Value::Color(Color::CurrentColor))
        );
    }

    #[test]
    fn hex_colors_short_and_long() {
        assert_eq!(
            value_of("#f00"),
            Some(Value::Color(Color::Rgba(255, 0, 0, 255)))
        );
        assert_eq!(
            value_of("#ff0000"),
            Some(Value::Color(Color::Rgba(255, 0, 0, 255)))
        );
        assert_eq!(
            value_of("#1a2b3c"),
            Some(Value::Color(Color::Rgba(0x1a, 0x2b, 0x3c, 255)))
        );
    }

    #[test]
    fn invalid_hex_length_is_none() {
        assert_eq!(value_of("#12345"), None);
    }

    #[test]
    fn empty_or_multi_token_component_is_none() {
        assert_eq!(parse_value(&[]), None);
        assert_eq!(
            value_of("1px solid"),
            None,
            "two meaningful tokens is not a single value component"
        );
    }
}
