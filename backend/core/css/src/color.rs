// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS Color 4 parsing: the named-color keywords, `#rgb[a]`/`#rrggbb[aa]`,
//! `rgb()`/`rgba()`, `hsl()`/`hsla()`, `hwb()`, `oklab()`/`oklch()`,
//! `color(srgb ...)`, `color-mix()` in srgb/oklab/oklch and `light-dark()`
//! (always the light value). Results are sRGB with straight alpha; wide-gamut
//! colours are clamped into sRGB, which is what an sRGB display shows anyway.

use crate::tokenizer::Token;
use crate::value::Color;

/// The CSS named colors (CSS Color 4 section 6.1), lowercase.
const NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("aliceblue", (240, 248, 255)),
    ("antiquewhite", (250, 235, 215)),
    ("aqua", (0, 255, 255)),
    ("aquamarine", (127, 255, 212)),
    ("azure", (240, 255, 255)),
    ("beige", (245, 245, 220)),
    ("bisque", (255, 228, 196)),
    ("rebeccapurple", (102, 51, 153)),
    ("black", (0, 0, 0)),
    ("blanchedalmond", (255, 235, 205)),
    ("blue", (0, 0, 255)),
    ("blueviolet", (138, 43, 226)),
    ("brown", (165, 42, 42)),
    ("burlywood", (222, 184, 135)),
    ("cadetblue", (95, 158, 160)),
    ("chartreuse", (127, 255, 0)),
    ("chocolate", (210, 105, 30)),
    ("coral", (255, 127, 80)),
    ("cornflowerblue", (100, 149, 237)),
    ("cornsilk", (255, 248, 220)),
    ("crimson", (220, 20, 60)),
    ("cyan", (0, 255, 255)),
    ("darkblue", (0, 0, 139)),
    ("darkcyan", (0, 139, 139)),
    ("darkgoldenrod", (184, 134, 11)),
    ("darkgray", (169, 169, 169)),
    ("darkgreen", (0, 100, 0)),
    ("darkgrey", (169, 169, 169)),
    ("darkkhaki", (189, 183, 107)),
    ("darkmagenta", (139, 0, 139)),
    ("darkolivegreen", (85, 107, 47)),
    ("darkorange", (255, 140, 0)),
    ("darkorchid", (153, 50, 204)),
    ("darkred", (139, 0, 0)),
    ("darksalmon", (233, 150, 122)),
    ("darkseagreen", (143, 188, 143)),
    ("darkslateblue", (72, 61, 139)),
    ("darkslategray", (47, 79, 79)),
    ("darkslategrey", (47, 79, 79)),
    ("darkturquoise", (0, 206, 209)),
    ("darkviolet", (148, 0, 211)),
    ("deeppink", (255, 20, 147)),
    ("deepskyblue", (0, 191, 255)),
    ("dimgray", (105, 105, 105)),
    ("dimgrey", (105, 105, 105)),
    ("dodgerblue", (30, 144, 255)),
    ("firebrick", (178, 34, 34)),
    ("floralwhite", (255, 250, 240)),
    ("forestgreen", (34, 139, 34)),
    ("fuchsia", (255, 0, 255)),
    ("gainsboro", (220, 220, 220)),
    ("ghostwhite", (248, 248, 255)),
    ("gold", (255, 215, 0)),
    ("goldenrod", (218, 165, 32)),
    ("gray", (128, 128, 128)),
    ("green", (0, 128, 0)),
    ("greenyellow", (173, 255, 47)),
    ("grey", (128, 128, 128)),
    ("honeydew", (240, 255, 240)),
    ("hotpink", (255, 105, 180)),
    ("indianred", (205, 92, 92)),
    ("indigo", (75, 0, 130)),
    ("ivory", (255, 255, 240)),
    ("khaki", (240, 230, 140)),
    ("lavender", (230, 230, 250)),
    ("lavenderblush", (255, 240, 245)),
    ("lawngreen", (124, 252, 0)),
    ("lemonchiffon", (255, 250, 205)),
    ("lightblue", (173, 216, 230)),
    ("lightcoral", (240, 128, 128)),
    ("lightcyan", (224, 255, 255)),
    ("lightgoldenrodyellow", (250, 250, 210)),
    ("lightgray", (211, 211, 211)),
    ("lightgreen", (144, 238, 144)),
    ("lightgrey", (211, 211, 211)),
    ("lightpink", (255, 182, 193)),
    ("lightsalmon", (255, 160, 122)),
    ("lightseagreen", (32, 178, 170)),
    ("lightskyblue", (135, 206, 250)),
    ("lightslategray", (119, 136, 153)),
    ("lightslategrey", (119, 136, 153)),
    ("lightsteelblue", (176, 196, 222)),
    ("lightyellow", (255, 255, 224)),
    ("lime", (0, 255, 0)),
    ("limegreen", (50, 205, 50)),
    ("linen", (250, 240, 230)),
    ("magenta", (255, 0, 255)),
    ("maroon", (128, 0, 0)),
    ("mediumaquamarine", (102, 205, 170)),
    ("mediumblue", (0, 0, 205)),
    ("mediumorchid", (186, 85, 211)),
    ("mediumpurple", (147, 112, 219)),
    ("mediumseagreen", (60, 179, 113)),
    ("mediumslateblue", (123, 104, 238)),
    ("mediumspringgreen", (0, 250, 154)),
    ("mediumturquoise", (72, 209, 204)),
    ("mediumvioletred", (199, 21, 133)),
    ("midnightblue", (25, 25, 112)),
    ("mintcream", (245, 255, 250)),
    ("mistyrose", (255, 228, 225)),
    ("moccasin", (255, 228, 181)),
    ("navajowhite", (255, 222, 173)),
    ("navy", (0, 0, 128)),
    ("oldlace", (253, 245, 230)),
    ("olive", (128, 128, 0)),
    ("olivedrab", (107, 142, 35)),
    ("orange", (255, 165, 0)),
    ("orangered", (255, 69, 0)),
    ("orchid", (218, 112, 214)),
    ("palegoldenrod", (238, 232, 170)),
    ("palegreen", (152, 251, 152)),
    ("paleturquoise", (175, 238, 238)),
    ("palevioletred", (219, 112, 147)),
    ("papayawhip", (255, 239, 213)),
    ("peachpuff", (255, 218, 185)),
    ("peru", (205, 133, 63)),
    ("pink", (255, 192, 203)),
    ("plum", (221, 160, 221)),
    ("powderblue", (176, 224, 230)),
    ("purple", (128, 0, 128)),
    ("red", (255, 0, 0)),
    ("rosybrown", (188, 143, 143)),
    ("royalblue", (65, 105, 225)),
    ("saddlebrown", (139, 69, 19)),
    ("salmon", (250, 128, 114)),
    ("sandybrown", (244, 164, 96)),
    ("seagreen", (46, 139, 87)),
    ("seashell", (255, 245, 238)),
    ("sienna", (160, 82, 45)),
    ("silver", (192, 192, 192)),
    ("skyblue", (135, 206, 235)),
    ("slateblue", (106, 90, 205)),
    ("slategray", (112, 128, 144)),
    ("slategrey", (112, 128, 144)),
    ("snow", (255, 250, 250)),
    ("springgreen", (0, 255, 127)),
    ("steelblue", (70, 130, 180)),
    ("tan", (210, 180, 140)),
    ("teal", (0, 128, 128)),
    ("thistle", (216, 191, 216)),
    ("tomato", (255, 99, 71)),
    ("turquoise", (64, 224, 208)),
    ("violet", (238, 130, 238)),
    ("wheat", (245, 222, 179)),
    ("white", (255, 255, 255)),
    ("whitesmoke", (245, 245, 245)),
    ("yellow", (255, 255, 0)),
    ("yellowgreen", (154, 205, 50)),
];

pub(crate) fn named_color(name: &str) -> Option<Color> {
    let lower = name.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(Color::Rgba(0, 0, 0, 0));
    }
    NAMED_COLORS
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|(_, (r, g, b))| Color::Rgba(*r, *g, *b, 255))
}

fn hex(digits: &str) -> Option<Color> {
    let nibble = |c: char| c.to_digit(16).map(|d| (d * 17) as u8);
    let byte = |s: &str| u8::from_str_radix(s, 16).ok();
    match digits.len() {
        3 | 4 => {
            let mut chars = digits.chars();
            let (r, g, b) = (
                nibble(chars.next()?)?,
                nibble(chars.next()?)?,
                nibble(chars.next()?)?,
            );
            let a = chars.next().map_or(Some(255), nibble)?;
            Some(Color::Rgba(r, g, b, a))
        }
        6 | 8 => {
            let a = if digits.len() == 8 {
                byte(&digits[6..8])?
            } else {
                255
            };
            Some(Color::Rgba(
                byte(&digits[0..2])?,
                byte(&digits[2..4])?,
                byte(&digits[4..6])?,
                a,
            ))
        }
        _ => None,
    }
}

/// What a numeric component token was written as.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Percent,
    Angle,
}

/// One component: its value (an angle in degrees) and how it was written.
/// `none` is a missing component, which acts as zero here.
fn component(token: &Token) -> Option<(f64, Kind)> {
    match token {
        Token::Number(n) => Some((*n, Kind::Number)),
        Token::Percentage(p) => Some((*p, Kind::Percent)),
        Token::Ident(word) if word.eq_ignore_ascii_case("none") => Some((0.0, Kind::Number)),
        Token::Dimension(n, unit) => {
            let degrees = match unit.to_ascii_lowercase().as_str() {
                "deg" => *n,
                "grad" => n * 0.9,
                "rad" => n.to_degrees(),
                "turn" => n * 360.0,
                _ => return None,
            };
            Some((degrees, Kind::Angle))
        }
        _ => None,
    }
}

/// A channel written as a number (scaled by `number_scale`) or a percentage
/// of `percent_scale`.
fn channel(token: &Token, number_scale: f64, percent_scale: f64) -> Option<f64> {
    match component(token)? {
        (v, Kind::Number) => Some(v * number_scale),
        (p, Kind::Percent) => Some(p / 100.0 * percent_scale),
        _ => None,
    }
}

fn alpha(token: Option<&Token>) -> Option<f64> {
    let Some(token) = token else { return Some(1.0) };
    channel(token, 1.0, 1.0).map(|a| a.clamp(0.0, 1.0))
}

fn hue(token: &Token) -> Option<f64> {
    match component(token)? {
        (v, Kind::Number | Kind::Angle) => Some(v),
        _ => None,
    }
}

fn byte(unit: f64) -> u8 {
    (unit.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rgba(r: f64, g: f64, b: f64, a: f64) -> Color {
    Color::Rgba(byte(r), byte(g), byte(b), byte(a))
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(360.0) / 360.0;
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |mut t: f64| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

fn to_gamma(linear: f64) -> f64 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn to_linear(gamma: f64) -> f64 {
    if gamma <= 0.040_45 {
        gamma / 12.92
    } else {
        ((gamma + 0.055) / 1.055).powf(2.4)
    }
}

fn oklab_to_srgb(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    (
        to_gamma(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        to_gamma(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        to_gamma(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    )
}

fn srgb_to_oklab(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    (
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    )
}

/// `tokens` without whitespace, split into the arguments of a colour function:
/// whitespace/comma separated components, then an optional `/ alpha` (or a
/// fourth legacy comma component).
fn arguments(inner: &[Token]) -> (Vec<&Token>, Option<&Token>) {
    let parts: Vec<&Token> = inner.iter().filter(|t| **t != Token::Comma).collect();
    if let Some(slash) = parts.iter().position(|t| **t == Token::Delim('/')) {
        return (parts[..slash].to_vec(), parts.get(slash + 1).copied());
    }
    if parts.len() == 4 {
        return (parts[..3].to_vec(), Some(parts[3]));
    }
    (parts, None)
}

/// The tokens between a function's outer parentheses, if `tokens` is exactly
/// one call `name( ... )`.
fn call(tokens: &[Token]) -> Option<(String, &[Token])> {
    let [Token::Ident(name), Token::LeftParen, ..] = tokens else {
        return None;
    };
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(1) {
        match token {
            Token::LeftParen => depth += 1,
            Token::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return (i == tokens.len() - 1)
                        .then(|| (name.to_ascii_lowercase(), &tokens[2..i]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits `tokens` on commas that are not inside parentheses.
pub(crate) fn split_top_level_commas(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let (mut start, mut depth) = (0, 0usize);
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen => depth += 1,
            Token::RightParen => depth = depth.saturating_sub(1),
            Token::Comma if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

/// Parses exactly one colour value. `currentColor` is returned as such.
pub(crate) fn parse_color(tokens: &[Token]) -> Option<Color> {
    let tokens: Vec<Token> = tokens
        .iter()
        .filter(|t| !matches!(t, Token::Whitespace | Token::Eof))
        .cloned()
        .collect();
    parse_color_tokens(&tokens)
}

fn parse_color_tokens(tokens: &[Token]) -> Option<Color> {
    match tokens {
        [Token::Ident(word)] => {
            if word.eq_ignore_ascii_case("currentcolor") {
                Some(Color::CurrentColor)
            } else {
                named_color(word)
            }
        }
        [Token::Hash(digits)] => hex(digits),
        _ => {
            let (name, inner) = call(tokens)?;
            match name.as_str() {
                "rgb" | "rgba" => {
                    let (c, a) = arguments(inner);
                    let [r, g, b] = c[..] else { return None };
                    Some(rgba(
                        channel(r, 1.0 / 255.0, 1.0)?,
                        channel(g, 1.0 / 255.0, 1.0)?,
                        channel(b, 1.0 / 255.0, 1.0)?,
                        alpha(a)?,
                    ))
                }
                "hsl" | "hsla" => {
                    let (c, a) = arguments(inner);
                    let [h, s, l] = c[..] else { return None };
                    let (r, g, b) =
                        hsl_to_rgb(hue(h)?, channel(s, 0.01, 1.0)?, channel(l, 0.01, 1.0)?);
                    Some(rgba(r, g, b, alpha(a)?))
                }
                "hwb" => {
                    let (c, a) = arguments(inner);
                    let [h, w, k] = c[..] else { return None };
                    let (w, k) = (channel(w, 0.01, 1.0)?, channel(k, 0.01, 1.0)?);
                    let (w, k) = if w + k > 1.0 {
                        (w / (w + k), k / (w + k))
                    } else {
                        (w, k)
                    };
                    let (r, g, b) = hsl_to_rgb(hue(h)?, 1.0, 0.5);
                    let mix = |c: f64| c * (1.0 - w - k) + w;
                    Some(rgba(mix(r), mix(g), mix(b), alpha(a)?))
                }
                "oklab" => {
                    let (c, a) = arguments(inner);
                    let [l, aa, bb] = c[..] else { return None };
                    let (r, g, b) = oklab_to_srgb(
                        channel(l, 1.0, 1.0)?,
                        channel(aa, 1.0, 0.4)?,
                        channel(bb, 1.0, 0.4)?,
                    );
                    Some(rgba(r, g, b, alpha(a)?))
                }
                "oklch" => {
                    let (c, a) = arguments(inner);
                    let [l, chroma, h] = c[..] else { return None };
                    let (l, chroma, h) = (
                        channel(l, 1.0, 1.0)?,
                        channel(chroma, 1.0, 0.4)?,
                        hue(h)?.to_radians(),
                    );
                    let (r, g, b) = oklab_to_srgb(l, chroma * h.cos(), chroma * h.sin());
                    Some(rgba(r, g, b, alpha(a)?))
                }
                "color" => {
                    // No legacy comma syntax here: a fourth bare component is
                    // an error, not an alpha.
                    let parts: Vec<&Token> = inner.iter().collect();
                    let slash = parts.iter().position(|t| **t == Token::Delim('/'));
                    let (c, a) = match slash {
                        Some(i) => (&parts[..i], parts.get(i + 1).copied()),
                        None => (&parts[..], None),
                    };
                    let [Token::Ident(space), r, g, b] = c[..] else {
                        return None;
                    };
                    let (r, g, b) = (
                        channel(r, 1.0, 1.0)?,
                        channel(g, 1.0, 1.0)?,
                        channel(b, 1.0, 1.0)?,
                    );
                    match space.to_ascii_lowercase().as_str() {
                        "srgb" => Some(rgba(r, g, b, alpha(a)?)),
                        "srgb-linear" => {
                            Some(rgba(to_gamma(r), to_gamma(g), to_gamma(b), alpha(a)?))
                        }
                        _ => None,
                    }
                }
                "light-dark" => {
                    let parts = split_top_level_commas(inner);
                    parse_color_tokens(parts.first()?)
                }
                "color-mix" => color_mix(inner),
                _ => None,
            }
        }
    }
}

fn color_mix(inner: &[Token]) -> Option<Color> {
    let parts = split_top_level_commas(inner);
    let [space, first, second] = parts[..] else {
        return None;
    };
    let [Token::Ident(keyword), Token::Ident(space)] = space else {
        return None;
    };
    if !keyword.eq_ignore_ascii_case("in") {
        return None;
    }
    let linear_srgb = space.eq_ignore_ascii_case("srgb");
    let operand = |tokens: &[Token]| -> Option<(Color, Option<f64>)> {
        let percent = tokens
            .iter()
            .position(|t| matches!(t, Token::Percentage(_)));
        let (color_tokens, percent) = match percent {
            Some(i) => {
                let Token::Percentage(p) = tokens[i] else {
                    return None;
                };
                let rest: Vec<Token> = tokens
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, t)| t.clone())
                    .collect();
                (rest, Some(p))
            }
            None => (tokens.to_vec(), None),
        };
        Some((parse_color(&color_tokens)?, percent))
    };
    let ((a, pa), (b, pb)) = (operand(first)?, operand(second)?);
    let (Color::Rgba(ar, ag, ab, aa), Color::Rgba(br, bg, bb, ba)) = (a, b) else {
        return None;
    };
    let (pa, pb) = match (pa, pb) {
        (None, None) => (50.0, 50.0),
        (Some(p), None) => (p, 100.0 - p),
        (None, Some(p)) => (100.0 - p, p),
        (Some(p), Some(q)) => (p, q),
    };
    let total = pa + pb;
    if total <= 0.0 {
        return None;
    }
    let (wa, wb) = (pa / total, pb / total);
    let alpha_scale = (total / 100.0).min(1.0);
    let (a_alpha, b_alpha) = (f64::from(aa) / 255.0, f64::from(ba) / 255.0);
    let mixed_alpha = a_alpha * wa + b_alpha * wb;
    let unit = |v: u8| f64::from(v) / 255.0;
    let channels = |c: Color| {
        let Color::Rgba(r, g, b, _) = c else {
            unreachable!("only Rgba reaches here")
        };
        let (r, g, b) = (unit(r), unit(g), unit(b));
        if linear_srgb {
            (r, g, b)
        } else {
            srgb_to_oklab(r, g, b)
        }
    };
    let (x0, y0, z0) = channels(Color::Rgba(ar, ag, ab, 255));
    let (x1, y1, z1) = channels(Color::Rgba(br, bg, bb, 255));
    // Premultiplied interpolation: a transparent operand contributes no colour.
    let weight = |own: f64, w: f64| own * w;
    let denominator = weight(a_alpha, wa) + weight(b_alpha, wb);
    let mix = |p: f64, q: f64| {
        if denominator == 0.0 {
            0.0
        } else {
            (p * weight(a_alpha, wa) + q * weight(b_alpha, wb)) / denominator
        }
    };
    let (x, y, z) = (mix(x0, x1), mix(y0, y1), mix(z0, z1));
    let (r, g, b) = if linear_srgb {
        (x, y, z)
    } else {
        oklab_to_srgb(x, y, z)
    };
    Some(rgba(r, g, b, mixed_alpha * alpha_scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn color(css: &str) -> Option<Color> {
        parse_color(&tokenize(css))
    }
    fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
        Some(Color::Rgba(r, g, b, 255))
    }
    /// Within one unit per channel, for the float conversions.
    fn near(css: &str, r: u8, g: u8, b: u8, a: u8) {
        let Some(Color::Rgba(pr, pg, pb, pa)) = color(css) else {
            panic!("{css} did not parse");
        };
        let close = |x: u8, y: u8| (i16::from(x) - i16::from(y)).abs() <= 1;
        assert!(
            close(pr, r) && close(pg, g) && close(pb, b) && close(pa, a),
            "{css} -> {:?}, wanted {:?}",
            (pr, pg, pb, pa),
            (r, g, b, a)
        );
    }

    #[test]
    fn every_named_color_resolves_and_keywords_ignore_case() {
        assert_eq!(color("rebeccapurple"), rgb(102, 51, 153));
        assert_eq!(color("CornflowerBlue"), rgb(100, 149, 237));
        assert_eq!(color("transparent"), Some(Color::Rgba(0, 0, 0, 0)));
        assert_eq!(color("currentColor"), Some(Color::CurrentColor));
        assert_eq!(color("notacolor"), None);
        assert_eq!(
            NAMED_COLORS.len(),
            148,
            "CSS Color 4 defines 148 names; transparent is separate"
        );
    }

    #[test]
    fn hex_forms_including_alpha() {
        assert_eq!(color("#f80"), rgb(255, 136, 0));
        assert_eq!(color("#f808"), Some(Color::Rgba(255, 136, 0, 136)));
        assert_eq!(color("#ff8800"), rgb(255, 136, 0));
        assert_eq!(color("#ff880080"), Some(Color::Rgba(255, 136, 0, 128)));
        assert_eq!(color("#12345"), None);
    }

    #[test]
    fn rgb_in_legacy_and_modern_syntax() {
        assert_eq!(color("rgb(255, 0, 0)"), rgb(255, 0, 0));
        assert_eq!(color("rgb(255 0 0)"), rgb(255, 0, 0));
        assert_eq!(
            color("rgba(0, 0, 255, 0.5)"),
            Some(Color::Rgba(0, 0, 255, 128))
        );
        assert_eq!(
            color("rgb(0 0 255 / 50%)"),
            Some(Color::Rgba(0, 0, 255, 128))
        );
        assert_eq!(color("rgb(100% 0% 0%)"), rgb(255, 0, 0));
        assert_eq!(color("rgb(300 -5 0)"), rgb(255, 0, 0), "channels clamp");
        assert_eq!(color("rgb(0 0)"), None);
    }

    #[test]
    fn hsl_and_hwb() {
        assert_eq!(color("hsl(120, 100%, 50%)"), rgb(0, 255, 0));
        assert_eq!(
            color("hsl(0deg 100% 50% / 0.5)"),
            Some(Color::Rgba(255, 0, 0, 128))
        );
        assert_eq!(color("hsl(0.5turn 100% 50%)"), rgb(0, 255, 255));
        assert_eq!(color("hwb(0 0% 0%)"), rgb(255, 0, 0));
        assert_eq!(color("hwb(0 100% 0%)"), rgb(255, 255, 255));
    }

    #[test]
    fn oklab_and_oklch_match_known_conversions() {
        near("oklab(1 0 0)", 255, 255, 255, 255);
        near("oklab(0.5 0 0)", 99, 99, 99, 255);
        near("oklch(62.8% 0.2577 29.23)", 255, 0, 0, 255);
        near("oklch(0.5 0 0 / 50%)", 99, 99, 99, 128);
        near("oklch(0% 0 0)", 0, 0, 0, 255);
    }

    #[test]
    fn color_function_and_light_dark() {
        assert_eq!(color("color(srgb 1 0 0)"), rgb(255, 0, 0));
        assert_eq!(
            color("color(srgb 100% 0% 0% / 0.5)"),
            Some(Color::Rgba(255, 0, 0, 128))
        );
        assert_eq!(color("light-dark(red, blue)"), rgb(255, 0, 0));
        assert_eq!(color("color(display-p3 1 0 0)"), None);
    }

    #[test]
    fn color_mix_blends_by_percentage_and_keeps_transparency() {
        near("color-mix(in srgb, red, blue)", 128, 0, 128, 255);
        near("color-mix(in srgb, red 25%, blue)", 64, 0, 191, 255);
        near("color-mix(in oklab, white 50%, black)", 99, 99, 99, 255);
        near("color-mix(in oklab, red 40%, transparent)", 255, 0, 0, 102);
        near("color-mix(in oklab, red 100%, transparent)", 255, 0, 0, 255);
        assert_eq!(color("color-mix(in srgb, red)"), None);
        assert_eq!(color("color-mix(red, blue)"), None);
    }
}
