// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `@font-face`: the fonts a stylesheet brings with it.
//!
//! The rest of the cascade skips at-rules; this reads the font ones. It works on the text rather
//! than on tokens because `unicode-range: U+0131` is hexadecimal and the tokenizer would read it
//! as the number 131. A rule with no family or no source it can use is dropped; the others do not
//! depend on it.

/// One file a face can be loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSource {
    /// As written in the stylesheet (relative to it); empty for a `local()` source.
    pub url: String,
    /// The name inside `local(...)`, unquoted: a font installed on the system, not a file.
    pub local: Option<String>,
    /// The `format()` hint, lowercased (`woff2`, `woff`, `truetype`, `opentype`...).
    pub format: Option<String>,
}

/// One `@font-face` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFaceRule {
    /// The family name, lowercased.
    pub family: String,
    pub sources: Vec<FontSource>,
    /// The weights it covers (`font-weight: 300 900` is a range; one weight is both ends).
    pub weight_min: u16,
    pub weight_max: u16,
    pub italic: bool,
    /// The widths it covers, as a percentage in tenths (`font-stretch: 75% 125%` is
    /// `750..=1250`; the initial `normal` is 100%, `1000`).
    pub stretch_min: u16,
    pub stretch_max: u16,
    /// Code point ranges it covers; empty means all.
    pub unicode_ranges: Vec<(u32, u32)>,
}

/// The `@font-face` rules of `css`, in order, including those inside `@layer` and a `@supports`
/// the engine supports. Those inside `@media` and other conditional rules are not read.
pub fn font_faces(css: &str) -> Vec<FontFaceRule> {
    let mut out = Vec::new();
    scan(&strip_comments(css), &mut out, 0);
    out
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == q {
                quote = None;
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut last = ' ';
            for d in chars.by_ref() {
                if last == '*' && d == '/' {
                    break;
                }
                last = d;
            }
            out.push(' ');
        } else {
            if c == '"' || c == '\'' {
                quote = Some(c);
            }
            out.push(c);
        }
    }
    out
}

/// The top-level `prelude { body }` and `statement;` items of `text`: `(prelude, Some(body))` for
/// a block, `(statement, None)` for a statement.
fn items(text: &str) -> Vec<(&str, Option<&str>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut parens = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
        } else {
            match c {
                // An escaped character (`.w-\(x\)` in a class selector) is only that character.
                b'\\' => i += 1,
                b'"' | b'\'' => quote = Some(c),
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                b';' if parens == 0 => {
                    out.push((text[start..i].trim(), None));
                    start = i + 1;
                }
                b'{' if parens == 0 => {
                    let open = i;
                    let mut depth = 1usize;
                    let mut inner_quote: Option<u8> = None;
                    i += 1;
                    while i < bytes.len() && depth > 0 {
                        let d = bytes[i];
                        if let Some(q) = inner_quote {
                            if d == b'\\' {
                                i += 1;
                            } else if d == q {
                                inner_quote = None;
                            }
                        } else {
                            match d {
                                b'\\' => i += 1,
                                b'"' | b'\'' => inner_quote = Some(d),
                                b'{' => depth += 1,
                                b'}' => depth -= 1,
                                _ => {}
                            }
                        }
                        i += 1;
                    }
                    let close = if depth == 0 { i - 1 } else { bytes.len() };
                    out.push((text[start..open].trim(), Some(&text[open + 1..close])));
                    start = i.min(bytes.len());
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    out
}

fn scan(text: &str, out: &mut Vec<FontFaceRule>, depth: usize) {
    if depth > 16 {
        return;
    }
    for (prelude, body) in items(text) {
        let Some(body) = body else { continue };
        let Some(rest) = prelude.strip_prefix('@') else { continue };
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '(')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        match name.to_ascii_lowercase().as_str() {
            "font-face" => {
                if let Some(face) = parse_face(body) {
                    out.push(face);
                }
            }
            "layer" => scan(body, out, depth + 1),
            "supports" => {
                let mut tokens = crate::tokenizer::tokenize(rest[name.len()..].trim());
                if tokens.last() == Some(&crate::tokenizer::Token::Eof) {
                    tokens.pop();
                }
                if crate::supports::condition_holds(&tokens) {
                    scan(body, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.to_string();
        }
    }
    s.to_string()
}

/// Splits `text` on `separator` outside parentheses and quotes.
fn split_top(text: &str, separator: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut parens = 0usize;
    let mut quote: Option<char> = None;
    let mut skip = false;
    for (i, c) in text.char_indices() {
        if skip {
            skip = false;
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' {
                skip = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => parens += 1,
            ')' => parens = parens.saturating_sub(1),
            c if c == separator && parens == 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

/// The text inside `name( ... )` in `entry`, if there is such a call.
fn call_args<'a>(entry: &'a str, name: &str) -> Option<&'a str> {
    let lower = entry.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let start = from + at;
        let after = start + name.len();
        let boundary = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
        if boundary && lower[after..].starts_with('(') {
            let open = after;
            let mut depth = 0usize;
            let mut quote: Option<char> = None;
            for (i, c) in entry[open..].char_indices() {
                if let Some(q) = quote {
                    if c == q {
                        quote = None;
                    }
                    continue;
                }
                match c {
                    '"' | '\'' => quote = Some(c),
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(&entry[open + 1..open + i]);
                        }
                    }
                    _ => {}
                }
            }
            return None;
        }
        from = start + name.len();
    }
    None
}

fn parse_sources(value: &str) -> Vec<FontSource> {
    split_top(value, ',')
        .into_iter()
        .filter_map(|entry| {
            let Some(url) = call_args(entry, "url") else {
                let name = unquote(call_args(entry, "local")?);
                return (!name.is_empty()).then_some(FontSource { url: String::new(), local: Some(name), format: None });
            };
            Some(FontSource {
                url: unquote(url),
                local: None,
                format: call_args(entry, "format").map(|f| unquote(f).to_ascii_lowercase()),
            })
        })
        .collect()
}

/// The matching descriptors of a face a script made (`new FontFace(family, source, descriptors)`),
/// given as the text of each; an empty text is the descriptor's initial value.
pub struct FaceDescriptors {
    pub weight_min: u16,
    pub weight_max: u16,
    pub italic: bool,
    pub stretch_min: u16,
    pub stretch_max: u16,
    pub unicode_ranges: Vec<(u32, u32)>,
}

pub fn face_descriptors(weight: &str, style: &str, stretch: &str, unicode_range: &str) -> FaceDescriptors {
    let (weight_min, weight_max) = parse_weight(weight);
    let (stretch_min, stretch_max) = parse_stretch(stretch);
    let style = style.trim().to_ascii_lowercase();
    FaceDescriptors {
        weight_min,
        weight_max,
        italic: style.starts_with("italic") || style.starts_with("oblique"),
        stretch_min,
        stretch_max,
        unicode_ranges: parse_ranges(unicode_range),
    }
}

fn parse_weight(value: &str) -> (u16, u16) {
    let numbers: Vec<u16> = value
        .split_whitespace()
        .filter_map(|word| match word.to_ascii_lowercase().as_str() {
            "normal" => Some(400),
            "bold" => Some(700),
            other => other.parse::<f64>().ok().map(|n| n.clamp(1.0, 1000.0) as u16),
        })
        .collect();
    match numbers.as_slice() {
        [] => (400, 400),
        [one] => (*one, *one),
        [a, b, ..] => (*a.min(b), *a.max(b)),
    }
}

/// A `font-stretch` value or range in tenths of a percent.
fn parse_stretch(value: &str) -> (u16, u16) {
    let numbers: Vec<u16> = value
        .split_whitespace()
        .filter_map(|word| {
            let word = word.to_ascii_lowercase();
            let percent = match word.as_str() {
                "ultra-condensed" => 50.0,
                "extra-condensed" => 62.5,
                "condensed" => 75.0,
                "semi-condensed" => 87.5,
                "normal" => 100.0,
                "semi-expanded" => 112.5,
                "expanded" => 125.0,
                "extra-expanded" => 150.0,
                "ultra-expanded" => 200.0,
                other => other.strip_suffix('%')?.parse::<f64>().ok()?,
            };
            Some((percent * 10.0).round().clamp(10.0, 10000.0) as u16)
        })
        .collect();
    match numbers.as_slice() {
        [] => (1000, 1000),
        [one] => (*one, *one),
        [a, b, ..] => (*a.min(b), *a.max(b)),
    }
}

fn parse_ranges(value: &str) -> Vec<(u32, u32)> {
    split_top(value, ',')
        .into_iter()
        .filter_map(|item| {
            let item = item.trim();
            let hex = item.strip_prefix("U+").or_else(|| item.strip_prefix("u+"))?;
            if let Some((a, b)) = hex.split_once('-') {
                Some((u32::from_str_radix(a, 16).ok()?, u32::from_str_radix(b, 16).ok()?))
            } else if hex.contains('?') {
                let low = u32::from_str_radix(&hex.replace('?', "0"), 16).ok()?;
                let high = u32::from_str_radix(&hex.replace('?', "F"), 16).ok()?;
                Some((low, high))
            } else {
                let c = u32::from_str_radix(hex, 16).ok()?;
                Some((c, c))
            }
        })
        .collect()
}

fn parse_face(body: &str) -> Option<FontFaceRule> {
    let mut family = None;
    let mut sources = Vec::new();
    let mut weight = (400, 400);
    let mut italic = false;
    let mut stretch = (1000, 1000);
    let mut unicode_ranges = Vec::new();
    for declaration in split_top(body, ';') {
        let Some((name, value)) = declaration.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                let first = split_top(value, ',').into_iter().next().unwrap_or("");
                let words: Vec<String> = first.split_whitespace().map(str::to_string).collect();
                let name = if first.trim().starts_with(['"', '\'']) {
                    unquote(first)
                } else {
                    words.join(" ")
                };
                if !name.trim().is_empty() {
                    family = Some(name.trim().to_ascii_lowercase());
                }
            }
            "src" => sources = parse_sources(value),
            "font-weight" => weight = parse_weight(value),
            "font-style" => {
                let v = value.to_ascii_lowercase();
                italic = v.starts_with("italic") || v.starts_with("oblique");
            }
            "font-stretch" => stretch = parse_stretch(value),
            "unicode-range" => unicode_ranges = parse_ranges(value),
            _ => {}
        }
    }
    if sources.is_empty() {
        return None;
    }
    Some(FontFaceRule {
        family: family?,
        sources,
        weight_min: weight.0,
        weight_max: weight.1,
        italic,
        stretch_min: stretch.0,
        stretch_max: stretch.1,
        unicode_ranges,
    })
}
