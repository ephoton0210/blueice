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

use crate::calc::Calc;
use crate::tokenizer::Token;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f64),
    Em(f64),
    Zero,
    /// Relative to the root element's font size.
    Rem(f64),
    /// Percent of the viewport width/height/smaller/larger side, per unit.
    Vw(f64),
    Vh(f64),
    Vmin(f64),
    Vmax(f64),
}

/// What a length needs to become pixels: the element's and root's font size,
/// the viewport, and what a percentage is a percentage of. `percent_base` is
/// NaN where a percentage is indeterminate (e.g. height of an auto-height box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LengthContext {
    pub font_size: f64,
    pub root_font_size: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub percent_base: f64,
}

impl Length {
    /// The length for `n` written with `unit`. Absolute units become pixels at
    /// the CSS reference density; `ex` and `ch` are approximated as half an em.
    pub fn from_unit(n: f64, unit: &str) -> Option<Length> {
        Some(match unit.to_ascii_lowercase().as_str() {
            "px" => Length::Px(n),
            "em" => Length::Em(n),
            "rem" => Length::Rem(n),
            "ex" | "ch" => Length::Em(n * 0.5),
            "vw" | "svw" | "lvw" | "dvw" => Length::Vw(n),
            "vh" | "svh" | "lvh" | "dvh" => Length::Vh(n),
            "vmin" | "svmin" | "lvmin" | "dvmin" => Length::Vmin(n),
            "vmax" | "svmax" | "lvmax" | "dvmax" => Length::Vmax(n),
            "in" => Length::Px(n * 96.0),
            "cm" => Length::Px(n * 96.0 / 2.54),
            "mm" => Length::Px(n * 96.0 / 25.4),
            "q" => Length::Px(n * 96.0 / 101.6),
            "pt" => Length::Px(n * 96.0 / 72.0),
            "pc" => Length::Px(n * 16.0),
            _ => return None,
        })
    }

    pub fn to_px(self, ctx: &LengthContext) -> f64 {
        match self {
            Length::Px(n) => n,
            Length::Em(n) => n * ctx.font_size,
            Length::Zero => 0.0,
            Length::Rem(n) => n * ctx.root_font_size,
            Length::Vw(n) => n * ctx.viewport_width / 100.0,
            Length::Vh(n) => n * ctx.viewport_height / 100.0,
            Length::Vmin(n) => n * ctx.viewport_width.min(ctx.viewport_height) / 100.0,
            Length::Vmax(n) => n * ctx.viewport_width.max(ctx.viewport_height) / 100.0,
        }
    }
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
    /// `calc()`/`min()`/`max()`/`clamp()`, evaluated by layout.
    Calc(Calc),
    /// A space-separated value with several components (`flex: 1 1 0`).
    List(Vec<Value>),
    /// A custom property's value, or a declaration that uses `var()`: kept as
    /// tokens until the cascade substitutes the element's custom properties.
    Unparsed(Vec<Token>),
}

impl Value {
    /// This value as pixels for `ctx`: a length of any unit, a percentage of
    /// `ctx.percent_base`, a `calc()`, or a bare `0`.
    pub fn to_px(&self, ctx: &LengthContext) -> Option<f64> {
        match self {
            Value::Length(length) => Some(length.to_px(ctx)),
            Value::Percentage(p) if !ctx.percent_base.is_nan() => {
                Some(ctx.percent_base * p / 100.0)
            }
            Value::Number(n) if *n == 0.0 => Some(0.0),
            Value::Calc(calc) => calc.to_px(ctx),
            _ => None,
        }
    }
}

/// Parses a single value component (already comma/space-split by the
/// caller) from its tokens. Leading/trailing [`Token::Whitespace`] is
/// ignored. A component is one token, or one function call
/// (`rgb(...)`, `calc(...)`, `oklch(...)`, ...).
pub fn parse_value(tokens: &[Token]) -> Option<Value> {
    let meaningful: Vec<Token> = tokens
        .iter()
        .filter(|t| !matches!(t, Token::Whitespace | Token::Eof))
        .cloned()
        .collect();
    let [first, rest @ ..] = &meaningful[..] else {
        return None;
    };
    if !rest.is_empty() {
        return parse_function(&meaningful);
    }
    match first {
        Token::Ident(name) => {
            let lower = name.to_ascii_lowercase();
            if lower == "currentcolor" {
                return Some(Value::Color(Color::CurrentColor));
            }
            if let Some(color) = crate::color::named_color(&lower) {
                return Some(Value::Color(color));
            }
            Some(Value::Keyword(lower))
        }
        Token::Hash(hex) => {
            crate::color::parse_color(&[Token::Hash(hex.clone())]).map(Value::Color)
        }
        Token::Number(n) => Some(Value::Number(*n)),
        Token::Percentage(n) => Some(Value::Percentage(*n)),
        Token::Dimension(n, unit) => Length::from_unit(*n, unit).map(Value::Length),
        _ => None,
    }
}

/// A value that is exactly one function call: a colour function or a math
/// function.
fn parse_function(tokens: &[Token]) -> Option<Value> {
    let Some(Token::Ident(name)) = tokens.first() else {
        return None;
    };
    match name.to_ascii_lowercase().as_str() {
        "calc" | "-webkit-calc" | "-moz-calc" | "min" | "max" | "clamp" => {
            crate::calc::parse_calc(tokens).map(Value::Calc)
        }
        _ => crate::color::parse_color(tokens).map(Value::Color),
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
    fn every_supported_unit_and_an_unknown_one() {
        assert_eq!(value_of("3vh"), Some(Value::Length(Length::Vh(3.0))));
        assert_eq!(value_of("2rem"), Some(Value::Length(Length::Rem(2.0))));
        assert_eq!(value_of("1in"), Some(Value::Length(Length::Px(96.0))));
        assert_eq!(value_of("12pt"), Some(Value::Length(Length::Px(16.0))));
        assert_eq!(value_of("2ch"), Some(Value::Length(Length::Em(1.0))));
        assert_eq!(value_of("3furlongs"), None);
    }

    fn ctx() -> LengthContext {
        LengthContext {
            font_size: 10.0,
            root_font_size: 16.0,
            viewport_width: 800.0,
            viewport_height: 600.0,
            percent_base: 200.0,
        }
    }

    #[test]
    fn lengths_resolve_against_the_context() {
        let px = |css: &str| value_of(css).and_then(|v| v.to_px(&ctx()));
        assert_eq!(px("3px"), Some(3.0));
        assert_eq!(px("2em"), Some(20.0));
        assert_eq!(px("2rem"), Some(32.0));
        assert_eq!(px("10vw"), Some(80.0));
        assert_eq!(px("10vh"), Some(60.0));
        assert_eq!(px("10vmin"), Some(60.0));
        assert_eq!(px("10vmax"), Some(80.0));
        assert_eq!(px("50%"), Some(100.0));
        assert_eq!(px("0"), Some(0.0));
        assert_eq!(px("auto"), None);
        assert_eq!(px("calc(100% - 1rem)"), Some(184.0));
        let mut indeterminate = ctx();
        indeterminate.percent_base = f64::NAN;
        assert_eq!(value_of("50%").unwrap().to_px(&indeterminate), None);
    }

    #[test]
    fn color_functions_and_math_functions_are_single_values() {
        assert_eq!(
            value_of("rgb(1, 2, 3)"),
            Some(Value::Color(Color::Rgba(1, 2, 3, 255)))
        );
        assert!(matches!(value_of("calc(1px + 2px)"), Some(Value::Calc(_))));
        assert_eq!(value_of("calc(1px +"), None);
        assert_eq!(value_of("nonsense(1)"), None);
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
