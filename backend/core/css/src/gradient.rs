// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The values paint needs that are not single tokens: `linear-gradient()` /
//! `radial-gradient()` (and their `repeating-` forms, treated as plain
//! ones) and `box-shadow` lists. They are stored as raw tokens by the parser
//! and read here, so the cascade carries them unchanged.

use crate::color::parse_color;
use crate::tokenizer::Token;
use crate::value::{Color, Length, LengthContext};

#[derive(Debug, Clone, PartialEq)]
pub enum StopPosition {
    Percent(f64),
    Px(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorStop {
    pub color: Color,
    pub position: Option<StopPosition>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinearDirection {
    /// CSS angle in degrees: 0 points up, 90 right.
    Angle(f64),
    /// `to top right` and the like: the corner picks the angle from the box.
    Corner { right: bool, bottom: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum GradientKind {
    Linear(LinearDirection),
    /// Circle or ellipse reaching the farthest corner, centred at
    /// (`x`, `y`) fractions of the box.
    Radial {
        circle: bool,
        x: f64,
        y: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<ColorStop>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shadow {
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
    pub spread: f64,
    pub color: Option<Color>,
    pub inset: bool,
}

/// The tokens inside the call `name( ... )` that `tokens` consists of.
fn call_args<'a>(tokens: &'a [Token], names: &[&str]) -> Option<&'a [Token]> {
    let trimmed = trim(tokens);
    let [Token::Ident(name), Token::LeftParen, rest @ ..] = trimmed else {
        return None;
    };
    if !names.contains(&name.to_ascii_lowercase().as_str()) {
        return None;
    }
    match rest.split_last() {
        Some((Token::RightParen, inner)) => Some(inner),
        _ => None,
    }
}

fn trim(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|t| *t != Token::Whitespace)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|t| *t != Token::Whitespace)
        .map_or(start, |e| e + 1);
    &tokens[start..end]
}

fn split_top(tokens: &[Token], is_sep: impl Fn(&Token) -> bool) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let (mut start, mut depth) = (0usize, 0usize);
    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::LeftParen => depth += 1,
            Token::RightParen => depth = depth.saturating_sub(1),
            _ if depth == 0 && is_sep(t) => {
                parts.push(trim(&tokens[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(trim(&tokens[start..]));
    parts.retain(|p| !p.is_empty());
    parts
}

fn angle_degrees(token: &Token) -> Option<f64> {
    match token {
        Token::Dimension(n, unit) => match unit.to_ascii_lowercase().as_str() {
            "deg" => Some(*n),
            "grad" => Some(n * 0.9),
            "rad" => Some(n.to_degrees()),
            "turn" => Some(n * 360.0),
            _ => None,
        },
        Token::Number(n) if *n == 0.0 => Some(0.0),
        _ => None,
    }
}

fn keyword_of(token: &Token) -> Option<String> {
    match token {
        Token::Ident(w) => Some(w.to_ascii_lowercase()),
        _ => None,
    }
}

fn parse_stop(part: &[Token]) -> Option<ColorStop> {
    // The colour is the first value (a hash, name or function call); an
    // optional position follows.
    let (color_tokens, rest): (&[Token], &[Token]) = match part {
        [Token::Ident(_), Token::LeftParen, ..] => {
            let mut depth = 0usize;
            let mut end = part.len();
            for (i, t) in part.iter().enumerate() {
                match t {
                    Token::LeftParen => depth += 1,
                    Token::RightParen => {
                        depth -= 1;
                        if depth == 0 {
                            end = i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            (&part[..end], &part[end..])
        }
        [first, rest @ ..] => (std::slice::from_ref(first), rest),
        [] => return None,
    };
    let color = match color_tokens {
        [Token::Ident(name)] if name.eq_ignore_ascii_case("transparent") => Color::Rgba(0, 0, 0, 0),
        [Token::Ident(name)] if name.eq_ignore_ascii_case("currentcolor") => Color::CurrentColor,
        other => parse_color(other)?,
    };
    let position = rest.iter().find_map(|t| match t {
        Token::Percentage(p) => Some(StopPosition::Percent(*p)),
        Token::Dimension(n, unit) => {
            Length::from_unit(*n, unit).map(|l| StopPosition::Px(l.to_px(&unit_context())))
        }
        Token::Number(n) if *n == 0.0 => Some(StopPosition::Px(0.0)),
        _ => None,
    });
    Some(ColorStop { color, position })
}

fn unit_context() -> LengthContext {
    LengthContext {
        font_size: 16.0,
        root_font_size: 16.0,
        viewport_width: 1280.0,
        viewport_height: 720.0,
        percent_base: f64::NAN,
    }
}

/// Parses `linear-gradient(...)` / `radial-gradient(...)` (also `repeating-`
/// and `-webkit-` forms), or `None` for anything else.
pub fn parse_gradient(tokens: &[Token]) -> Option<Gradient> {
    let linear = call_args(
        tokens,
        &[
            "linear-gradient",
            "repeating-linear-gradient",
            "-webkit-linear-gradient",
        ],
    );
    let radial = call_args(
        tokens,
        &[
            "radial-gradient",
            "repeating-radial-gradient",
            "-webkit-radial-gradient",
        ],
    );
    let (args, is_linear) = match (linear, radial) {
        (Some(a), _) => (a, true),
        (_, Some(a)) => (a, false),
        _ => return None,
    };
    let mut parts = split_top(args, |t| *t == Token::Comma);
    if parts.is_empty() {
        return None;
    }
    let mut kind = if is_linear {
        GradientKind::Linear(LinearDirection::Angle(180.0))
    } else {
        GradientKind::Radial {
            circle: false,
            x: 0.5,
            y: 0.5,
        }
    };
    let first = parts[0];
    let words: Vec<&Token> = first.iter().filter(|t| **t != Token::Whitespace).collect();
    let starts_with_color = parse_stop(first).is_some()
        && !matches!(words.first(), Some(Token::Ident(w)) if matches!(w.as_str(), "to" | "circle" | "ellipse" | "at" | "closest-side" | "farthest-side" | "closest-corner" | "farthest-corner"));
    if !starts_with_color {
        if is_linear {
            if let Some(angle) = words.first().and_then(|t| angle_degrees(t)) {
                kind = GradientKind::Linear(LinearDirection::Angle(angle));
            } else if matches!(words.first(), Some(Token::Ident(w)) if w == "to") {
                let sides: Vec<String> = words[1..].iter().filter_map(|t| keyword_of(t)).collect();
                let has = |s: &str| sides.iter().any(|x| x == s);
                let direction = match (has("top"), has("bottom"), has("left"), has("right")) {
                    (true, _, false, false) => LinearDirection::Angle(0.0),
                    (_, true, false, false) => LinearDirection::Angle(180.0),
                    (false, false, true, _) => LinearDirection::Angle(270.0),
                    (false, false, _, true) => LinearDirection::Angle(90.0),
                    (top, bottom, left, right) => LinearDirection::Corner {
                        right: right && !left,
                        bottom: bottom && !top,
                    },
                };
                kind = GradientKind::Linear(direction);
            }
        } else {
            let mut circle = false;
            let (mut x, mut y) = (0.5, 0.5);
            let mut at = false;
            let mut coords: Vec<f64> = Vec::new();
            for t in &words {
                match t {
                    Token::Ident(w) if w == "circle" => circle = true,
                    Token::Ident(w) if w == "at" => at = true,
                    Token::Ident(w) if at => match w.as_str() {
                        "left" => coords.push(0.0),
                        "right" => coords.push(1.0),
                        "top" => coords.push(0.0),
                        "bottom" => coords.push(1.0),
                        _ => coords.push(0.5),
                    },
                    Token::Percentage(p) if at => coords.push(p / 100.0),
                    _ => {}
                }
            }
            if let Some(cx) = coords.first() {
                x = *cx;
                y = coords.get(1).copied().unwrap_or(0.5);
            }
            kind = GradientKind::Radial { circle, x, y };
        }
        parts.remove(0);
    }
    let stops: Vec<ColorStop> = parts.iter().filter_map(|p| parse_stop(p)).collect();
    (stops.len() >= 2).then_some(Gradient { kind, stops })
}

/// Parses a `box-shadow` value into its shadows (`None` for `none` or when
/// nothing parses). Lengths resolve against `font_size` for `em`.
pub fn parse_shadows(tokens: &[Token], font_size: f64) -> Vec<Shadow> {
    let ctx = LengthContext {
        font_size,
        ..unit_context()
    };
    split_top(tokens, |t| *t == Token::Comma)
        .into_iter()
        .filter_map(|part| {
            let mut lengths: Vec<f64> = Vec::new();
            let mut color = None;
            let mut inset = false;
            let mut i = 0;
            while i < part.len() {
                match &part[i] {
                    Token::Whitespace => i += 1,
                    Token::Ident(w) if w.eq_ignore_ascii_case("inset") => {
                        inset = true;
                        i += 1;
                    }
                    Token::Number(n) if *n == 0.0 => {
                        lengths.push(0.0);
                        i += 1;
                    }
                    Token::Dimension(n, unit) => {
                        lengths.push(Length::from_unit(*n, unit)?.to_px(&ctx));
                        i += 1;
                    }
                    Token::Ident(name) if matches!(part.get(i + 1), Some(Token::LeftParen)) => {
                        let mut depth = 0usize;
                        let mut end = part.len();
                        for (k, t) in part.iter().enumerate().skip(i) {
                            match t {
                                Token::LeftParen => depth += 1,
                                Token::RightParen => {
                                    depth -= 1;
                                    if depth == 0 {
                                        end = k + 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        let _ = name;
                        color = Some(parse_color(&part[i..end])?);
                        i = end;
                    }
                    other => {
                        color = Some(match other {
                            Token::Ident(w) if w.eq_ignore_ascii_case("currentcolor") => {
                                Color::CurrentColor
                            }
                            t => parse_color(std::slice::from_ref(t))?,
                        });
                        i += 1;
                    }
                }
            }
            if lengths.len() < 2 {
                return None;
            }
            Some(Shadow {
                dx: lengths[0],
                dy: lengths[1],
                blur: lengths.get(2).copied().unwrap_or(0.0).max(0.0),
                spread: lengths.get(3).copied().unwrap_or(0.0),
                color,
                inset,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn tokens(css: &str) -> Vec<Token> {
        tokenize(css)
    }

    #[test]
    fn a_linear_gradient_has_a_direction_and_stops() {
        let g = parse_gradient(&tokens(
            "linear-gradient(to right, #ff0000, rgba(0, 0, 255, 0.5) 80%)",
        ))
        .unwrap();
        assert_eq!(g.kind, GradientKind::Linear(LinearDirection::Angle(90.0)));
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].color, Color::Rgba(255, 0, 0, 255));
        assert_eq!(g.stops[1].position, Some(StopPosition::Percent(80.0)));
    }

    #[test]
    fn the_default_direction_is_downward_and_an_angle_is_read() {
        let g = parse_gradient(&tokens("linear-gradient(red, blue)")).unwrap();
        assert_eq!(g.kind, GradientKind::Linear(LinearDirection::Angle(180.0)));
        let g = parse_gradient(&tokens("linear-gradient(0.25turn, red, blue)")).unwrap();
        assert_eq!(g.kind, GradientKind::Linear(LinearDirection::Angle(90.0)));
        let g = parse_gradient(&tokens("linear-gradient(to bottom right, red, blue)")).unwrap();
        assert_eq!(
            g.kind,
            GradientKind::Linear(LinearDirection::Corner {
                right: true,
                bottom: true
            })
        );
    }

    #[test]
    fn a_radial_gradient_reads_shape_and_centre() {
        let g = parse_gradient(&tokens("radial-gradient(circle at 20% 30%, red, blue)")).unwrap();
        assert_eq!(
            g.kind,
            GradientKind::Radial {
                circle: true,
                x: 0.2,
                y: 0.3
            }
        );
    }

    #[test]
    fn non_gradients_and_one_stop_gradients_are_rejected() {
        assert!(parse_gradient(&tokens("url(a.png)")).is_none());
        assert!(parse_gradient(&tokens("linear-gradient(red)")).is_none());
    }

    #[test]
    fn shadows_parse_offsets_blur_spread_colour_and_inset() {
        let s = parse_shadows(
            &tokens("0 2px 4px rgba(0,0,0,.2), inset 1px 1px 0 3px #fff"),
            16.0,
        );
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].dx, s[0].dy, s[0].blur, s[0].spread),
            (0.0, 2.0, 4.0, 0.0)
        );
        assert!(matches!(s[0].color, Some(Color::Rgba(0, 0, 0, _))));
        assert!(s[1].inset);
        assert_eq!(s[1].spread, 3.0);
    }

    #[test]
    fn a_shadow_without_two_offsets_is_ignored() {
        assert!(parse_shadows(&tokens("red"), 16.0).is_empty());
    }
}
