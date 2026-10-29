// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rule/declaration parsing: [`Token`] stream -> [`Rule`]s, tying
//! `tokenizer`, `selector`, and `value` together into a [`Stylesheet`].
//!
//! Per `phase-2-mvp-scope/PLAN.md`'s "MVP CSS scope": unknown at-rules
//! (`@media`, `@font-face`, `@import`, ...) are recognized and skipped
//! (consumed to their matching block or `;`) rather than corrupting the
//! rest of the stylesheet or aborting the parse -- the same
//! "ignore, don't crash" error recovery the HTML parser uses for
//! malformed markup. `margin`/`padding`'s 1-to-4-value shorthand syntax
//! is expanded into the four longhands at parse time (matching how a
//! real cascade only ever operates on longhands), since it's common
//! enough in real authored CSS that skipping it would fail ordinary
//! pages, not just edge cases.

use crate::selector::{parse_selector_list, ComplexSelector};
use crate::tokenizer::{tokenize, Token};
use crate::value::{parse_value, zero_as_length, Length, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub property: String,
    pub value: Value,
    pub important: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
    pub declarations: Vec<Declaration>,
}

/// Properties whose value is a length/percentage/keyword, for which a
/// bare `0` (tokenized as a unitless [`Token::Number`]) means the same
/// thing as `0px` -- as opposed to properties like `flex-grow` or
/// `line-height` where a bare number is a meaningful unitless value in
/// its own right, not shorthand for a length.
const LENGTH_VALUED_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "top",
    "right",
    "bottom",
    "left",
    "flex-basis",
    "font-size",
];

/// Parses one component's tokens into a [`Value`], normalizing a bare
/// `0` to [`Length::Zero`] for properties where that's meaningful.
fn parse_component_value(property: &str, tokens: &[Token]) -> Option<Value> {
    let value = parse_value(tokens)?;
    if LENGTH_VALUED_PROPERTIES.contains(&property) {
        if let Some(len) = zero_as_length(&value) {
            return Some(Value::Length(len));
        }
    }
    Some(value)
}

/// Splits `tokens` at every separator that is not inside parentheses,
/// brackets or braces, so `calc(1px + 2px)` stays one component and a `;`
/// inside `url(data:...;base64,...)` does not end a declaration.
fn split_on(tokens: &[Token], is_sep: impl Fn(&Token) -> bool) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.saturating_sub(1)
            }
            _ if depth == 0 && is_sep(t) => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

fn trim_ws(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|t| *t != Token::Whitespace)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|t| *t != Token::Whitespace)
        .map(|i| i + 1)
        .unwrap_or(0);
    if start >= end {
        &[]
    } else {
        &tokens[start..end]
    }
}

/// Expands the CSS box-shorthand 1-to-4-value syntax
/// (top/right/bottom/left) per the standard rule: 1 value sets all
/// four; 2 sets (top/bottom, right/left); 3 sets (top, right/left,
/// bottom); 4 sets each explicitly.
fn expand_box_shorthand(values: Vec<Value>) -> Option<[Value; 4]> {
    match values.len() {
        1 => {
            let [a]: [Value; 1] = values.try_into().ok()?;
            Some([a.clone(), a.clone(), a.clone(), a])
        }
        2 => {
            let [a, b]: [Value; 2] = values.try_into().ok()?;
            Some([a.clone(), b.clone(), a, b])
        }
        3 => {
            let [a, b, c]: [Value; 3] = values.try_into().ok()?;
            Some([a, b.clone(), c, b])
        }
        4 => values.try_into().ok(),
        _ => None,
    }
}

/// Border-style keywords (per `phase-2-mvp-scope/PLAN.md`'s "MVP CSS
/// scope" -- MVP treats every non-`none` style as an ordinary solid
/// line, but still needs to *recognize* the full keyword set so the
/// combined `border` shorthand below can tell a style component apart
/// from a color or a width by trying each in turn).
const BORDER_STYLE_KEYWORDS: &[&str] = &[
    "none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset",
];

/// `inset` and friends: a 1-to-4-value box shorthand onto four named longhands.
fn expand_box_shorthand_property_named(
    names: [&str; 4],
    value_tokens: &[Token],
) -> Vec<(String, Value)> {
    let values: Option<Vec<Value>> = components(value_tokens)
        .iter()
        .map(|c| parse_component_value("top", c))
        .collect();
    let Some(values) = values else { return vec![] };
    let Some(expanded) = expand_box_shorthand(values) else {
        return vec![];
    };
    names
        .iter()
        .zip(expanded)
        .map(|(name, value)| (name.to_string(), value))
        .collect()
}

/// Expands a 1-to-4-value box shorthand (`margin`, `padding`,
/// `border-width`, `border-style`, `border-color`) into its four
/// per-side longhands, named `{prefix}-top{suffix}` etc. `is_length`
/// controls whether a bare `0` component normalizes to
/// [`Length::Zero`] (true for `margin`/`padding`/`border-width`; false
/// for `border-style`/`border-color`, which have no numeric form at
/// all).
fn expand_box_shorthand_property(
    prefix: &str,
    suffix: &str,
    value_tokens: &[Token],
    is_length: bool,
) -> Vec<(String, Value)> {
    let probe_property = format!("{prefix}-top{suffix}");
    let components: Option<Vec<Value>> = split_on(value_tokens, |t| *t == Token::Whitespace)
        .into_iter()
        .map(trim_ws)
        .filter(|c| !c.is_empty())
        .map(|c| {
            if is_length {
                parse_component_value(&probe_property, c)
            } else {
                parse_value(c)
            }
        })
        .collect();
    let Some(components) = components else {
        return vec![];
    };
    let Some([top, right, bottom, left]) = expand_box_shorthand(components) else {
        return vec![];
    };
    vec![
        (format!("{prefix}-top{suffix}"), top),
        (format!("{prefix}-right{suffix}"), right),
        (format!("{prefix}-bottom{suffix}"), bottom),
        (format!("{prefix}-left{suffix}"), left),
    ]
}

/// The whitespace-separated components of a value, none empty.
fn components(tokens: &[Token]) -> Vec<&[Token]> {
    split_on(tokens, |t| *t == Token::Whitespace)
        .into_iter()
        .map(trim_ws)
        .filter(|c| !c.is_empty())
        .collect()
}

fn keyword(value: &str) -> Value {
    Value::Keyword(value.to_string())
}

/// `flex: <grow> [<shrink>] [<basis>]` and its keyword forms.
fn expand_flex(tokens: &[Token]) -> Vec<(String, Value)> {
    let parts = components(tokens);
    let single_keyword = match parts.as_slice() {
        [one] => match parse_value(one) {
            Some(Value::Keyword(k)) => Some(k),
            _ => None,
        },
        _ => None,
    };
    let (grow, shrink, basis) = match single_keyword.as_deref() {
        Some("none") => (Value::Number(0.0), Value::Number(0.0), keyword("auto")),
        Some("auto") => (Value::Number(1.0), Value::Number(1.0), keyword("auto")),
        Some("initial") => (Value::Number(0.0), Value::Number(1.0), keyword("auto")),
        Some(_) => return vec![],
        None => {
            let values: Option<Vec<Value>> = parts.iter().map(|p| parse_value(p)).collect();
            let Some(values) = values else { return vec![] };
            let number = |v: &Value| matches!(v, Value::Number(n) if *n >= 0.0);
            let basis_of = |v: &Value| match v {
                Value::Number(n) if *n == 0.0 => Some(Value::Length(Length::Zero)),
                Value::Number(_) | Value::Color(_) | Value::List(_) | Value::Unparsed(_) => None,
                other => Some(other.clone()),
            };
            match values.as_slice() {
                [grow] if number(grow) => {
                    (grow.clone(), Value::Number(1.0), Value::Percentage(0.0))
                }
                [basis] => (
                    Value::Number(1.0),
                    Value::Number(1.0),
                    match basis_of(basis) {
                        Some(basis) => basis,
                        None => return vec![],
                    },
                ),
                [grow, shrink] if number(grow) && number(shrink) => {
                    (grow.clone(), shrink.clone(), Value::Percentage(0.0))
                }
                [grow, basis] if number(grow) => (
                    grow.clone(),
                    Value::Number(1.0),
                    match basis_of(basis) {
                        Some(basis) => basis,
                        None => return vec![],
                    },
                ),
                [grow, shrink, basis] if number(grow) && number(shrink) => (
                    grow.clone(),
                    shrink.clone(),
                    match basis_of(basis) {
                        Some(basis) => basis,
                        None => return vec![],
                    },
                ),
                _ => return vec![],
            }
        }
    };
    vec![
        ("flex-grow".to_string(), grow),
        ("flex-shrink".to_string(), shrink),
        ("flex-basis".to_string(), basis),
    ]
}

/// Two-value shorthands (`gap`, `overflow`, `flex-flow`, `place-*`): one
/// value sets both longhands, two set them in order.
fn expand_pair(first: &str, second: &str, tokens: &[Token]) -> Vec<(String, Value)> {
    let values: Option<Vec<Value>> = components(tokens)
        .iter()
        .map(|c| parse_component_value(first, c))
        .collect();
    match values.as_deref() {
        Some([a]) => vec![
            (first.to_string(), a.clone()),
            (second.to_string(), a.clone()),
        ],
        Some([a, b]) => vec![
            (first.to_string(), a.clone()),
            (second.to_string(), b.clone()),
        ],
        _ => vec![],
    }
}

fn expand_flex_flow(tokens: &[Token]) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for part in components(tokens) {
        match parse_value(part) {
            Some(Value::Keyword(k))
                if ["row", "row-reverse", "column", "column-reverse"].contains(&k.as_str()) =>
            {
                out.push(("flex-direction".to_string(), Value::Keyword(k)))
            }
            Some(Value::Keyword(k)) if ["nowrap", "wrap", "wrap-reverse"].contains(&k.as_str()) => {
                out.push(("flex-wrap".to_string(), Value::Keyword(k)))
            }
            _ => return vec![],
        }
    }
    out
}

/// The colour of a `background` shorthand; a layer with none resets it.
fn expand_background(tokens: &[Token]) -> Vec<(String, Value)> {
    let mut out = vec![("background-image".to_string(), keyword("none"))];
    if let Some(image) = components(tokens).into_iter().find(|c| {
        matches!(c.first(), Some(Token::Url(_)))
            || matches!(c.first(), Some(Token::Ident(name))
                if matches!(c.get(1), Some(Token::LeftParen))
                    && (name.to_ascii_lowercase().ends_with("gradient")
                        || name.eq_ignore_ascii_case("url")))
    }) {
        out[0].1 = Value::Unparsed(image.to_vec());
    }
    let color = components(tokens)
        .into_iter()
        .find_map(|c| match parse_value(c) {
            Some(color @ Value::Color(_)) => Some(color),
            _ => None,
        })
        .unwrap_or(Value::Color(crate::value::Color::Rgba(0, 0, 0, 0)));
    out.push(("background-color".to_string(), color));

    // Repeat, position and size: whatever is left once colours and the image
    // are set aside, with the size after a `/`.
    let mut repeat: Vec<Value> = Vec::new();
    let mut position: Vec<Value> = Vec::new();
    let mut size: Vec<Value> = Vec::new();
    let mut after_slash = false;
    let mut depth = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::LeftParen => depth += 1,
            Token::RightParen => depth = depth.saturating_sub(1),
            Token::Delim('/') if depth == 0 => after_slash = true,
            Token::Ident(name) if matches!(tokens.get(i + 1), Some(Token::LeftParen)) => {
                // A function (the image, a colour): skip to its close.
                let mut d = 0usize;
                let mut j = i + 1;
                while j < tokens.len() {
                    match tokens[j] {
                        Token::LeftParen => d += 1,
                        Token::RightParen => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let _ = name;
                i = j;
            }
            token if depth == 0 => {
                if let Some(value) = parse_value(std::slice::from_ref(token)) {
                    match &value {
                        Value::Keyword(k)
                            if matches!(
                                k.as_str(),
                                "repeat"
                                    | "no-repeat"
                                    | "repeat-x"
                                    | "repeat-y"
                                    | "space"
                                    | "round"
                            ) =>
                        {
                            repeat.push(value)
                        }
                        Value::Keyword(k)
                            if after_slash
                                && matches!(k.as_str(), "cover" | "contain" | "auto") =>
                        {
                            size.push(value)
                        }
                        Value::Keyword(k)
                            if matches!(
                                k.as_str(),
                                "left" | "right" | "top" | "bottom" | "center"
                            ) =>
                        {
                            position.push(value)
                        }
                        Value::Length(_) | Value::Percentage(_) | Value::Calc(_) => {
                            if after_slash {
                                size.push(value)
                            } else {
                                position.push(value)
                            }
                        }
                        Value::Number(n) if *n == 0.0 => {
                            let zero = Value::Length(Length::Zero);
                            if after_slash {
                                size.push(zero)
                            } else {
                                position.push(zero)
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    let pack = |mut values: Vec<Value>| match values.len() {
        0 => None,
        1 => Some(values.remove(0)),
        _ => Some(Value::List(values)),
    };
    out.push((
        "background-repeat".to_string(),
        pack(repeat).unwrap_or_else(|| keyword("repeat")),
    ));
    out.push((
        "background-position".to_string(),
        pack(position).unwrap_or(Value::Percentage(0.0)),
    ));
    out.push((
        "background-size".to_string(),
        pack(size).unwrap_or_else(|| keyword("auto")),
    ));
    out
}

/// `list-style`: the marker keyword goes to `list-style-type`, `inside` or
/// `outside` to `list-style-position`; an image is ignored.
fn expand_list_style(tokens: &[Token]) -> Vec<(String, Value)> {
    let mut out = vec![("list-style-type".to_string(), keyword("disc"))];
    let mut explicit_type = false;
    for part in components(tokens) {
        if let Some(Value::Keyword(k)) = parse_value(part) {
            match k.as_str() {
                "inside" | "outside" => out.push(("list-style-position".to_string(), keyword(&k))),
                "none" if explicit_type => {}
                _ => {
                    out[0].1 = keyword(&k);
                    explicit_type = true;
                }
            }
        }
    }
    out
}

/// `border-radius`: one to four corner radii (the part after `/`, which makes
/// them elliptical, is read for its first value only) to the four longhands.
fn expand_border_radius(tokens: &[Token]) -> Vec<(String, Value)> {
    let horizontal: Vec<&Token> = tokens
        .iter()
        .take_while(|t| **t != Token::Delim('/'))
        .filter(|t| **t != Token::Whitespace)
        .collect();
    let values: Option<Vec<Value>> = horizontal
        .iter()
        .map(|t| parse_value(std::slice::from_ref(*t)))
        .collect();
    let Some(v) = values else { return vec![] };
    let [tl, tr, br, bl] = match v.as_slice() {
        [a] => [a, a, a, a],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return vec![],
    };
    let ok = |v: &Value| {
        matches!(
            v,
            Value::Length(_) | Value::Percentage(_) | Value::Number(_) | Value::Calc(_)
        )
    };
    if ![tl, tr, br, bl].iter().all(|v| ok(v)) {
        return vec![];
    }
    let fix = |v: &Value| match v {
        Value::Number(n) if *n == 0.0 => Value::Length(Length::Zero),
        other => other.clone(),
    };
    vec![
        ("border-top-left-radius".to_string(), fix(tl)),
        ("border-top-right-radius".to_string(), fix(tr)),
        ("border-bottom-right-radius".to_string(), fix(br)),
        ("border-bottom-left-radius".to_string(), fix(bl)),
    ]
}

/// `text-decoration`: the line keywords go to `text-decoration-line` (a single
/// keyword or a list) and a colour to `text-decoration-color`; the style and
/// thickness components are accepted and ignored.
fn expand_text_decoration(tokens: &[Token]) -> Vec<(String, Value)> {
    let mut lines: Vec<Value> = Vec::new();
    let mut color = None;
    for part in components(tokens) {
        match parse_value(part) {
            Some(Value::Keyword(k))
                if matches!(
                    k.as_str(),
                    "none" | "underline" | "overline" | "line-through" | "blink"
                ) =>
            {
                lines.push(Value::Keyword(k))
            }
            Some(Value::Keyword(k))
                if matches!(
                    k.as_str(),
                    "solid" | "double" | "dotted" | "dashed" | "wavy" | "from-font" | "auto"
                ) => {}
            Some(c @ Value::Color(_)) => color = Some(c),
            Some(Value::Length(_) | Value::Percentage(_) | Value::Calc(_)) => {}
            _ => return vec![],
        }
    }
    let mut out = Vec::new();
    match lines.len() {
        0 => out.push(("text-decoration-line".to_string(), keyword("none"))),
        1 => out.push(("text-decoration-line".to_string(), lines.remove(0))),
        _ => out.push(("text-decoration-line".to_string(), Value::List(lines))),
    }
    if let Some(color) = color {
        out.push(("text-decoration-color".to_string(), color));
    }
    out
}

/// `border`-style shorthands over the given sides.
fn expand_border_sides(sides: &[&str], value_tokens: &[Token]) -> Vec<(String, Value)> {
    // Order-independent: a width (a length, or bare `0`), a style keyword, and
    // a colour. Any component that is not one of the three (or is simply
    // omitted, as in `border: solid;`) is left unset rather than failing the
    // whole declaration -- matching the parser's "ignore, don't crash" posture.
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for component in components(value_tokens) {
        let Some(v) = parse_value(component) else {
            continue;
        };
        match v {
            Value::Color(_) => color = Some(v),
            Value::Keyword(ref k) if BORDER_STYLE_KEYWORDS.contains(&k.as_str()) => style = Some(v),
            Value::Keyword(ref k) if ["thin", "medium", "thick"].contains(&k.as_str()) => {
                width = Some(Value::Length(Length::Px(match k.as_str() {
                    "thin" => 1.0,
                    "medium" => 3.0,
                    _ => 5.0,
                })))
            }
            Value::Length(_) | Value::Calc(_) => width = Some(v),
            Value::Number(0.0) => width = Some(Value::Length(Length::Zero)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for side in sides {
        if let Some(w) = &width {
            out.push((format!("border-{side}-width"), w.clone()));
        }
        if let Some(s) = &style {
            out.push((format!("border-{side}-style"), s.clone()));
        }
        if let Some(c) = &color {
            out.push((format!("border-{side}-color"), c.clone()));
        }
    }
    out
}

/// Logical (flow-relative) properties, mapped for a left-to-right,
/// top-to-bottom document: inline is horizontal, block is vertical.
fn logical_longhands(property: &str) -> Option<&'static [&'static str]> {
    Some(match property {
        "margin-inline" => &["margin-left", "margin-right"],
        "margin-block" => &["margin-top", "margin-bottom"],
        "padding-inline" => &["padding-left", "padding-right"],
        "padding-block" => &["padding-top", "padding-bottom"],
        "inset-inline" => &["left", "right"],
        "inset-block" => &["top", "bottom"],
        "margin-inline-start" => &["margin-left"],
        "margin-inline-end" => &["margin-right"],
        "margin-block-start" => &["margin-top"],
        "margin-block-end" => &["margin-bottom"],
        "padding-inline-start" => &["padding-left"],
        "padding-inline-end" => &["padding-right"],
        "padding-block-start" => &["padding-top"],
        "padding-block-end" => &["padding-bottom"],
        "inset-inline-start" => &["left"],
        "inset-inline-end" => &["right"],
        "inset-block-start" => &["top"],
        "inset-block-end" => &["bottom"],
        "inline-size" => &["width"],
        "block-size" => &["height"],
        "min-inline-size" => &["min-width"],
        "min-block-size" => &["min-height"],
        "max-inline-size" => &["max-width"],
        "max-block-size" => &["max-height"],
        _ => return None,
    })
}

pub(crate) fn expand_property(property: &str, value_tokens: &[Token]) -> Vec<(String, Value)> {
    if let Some(longhands) = logical_longhands(property) {
        let values: Option<Vec<Value>> = components(value_tokens)
            .iter()
            .map(|c| parse_component_value(longhands[0], c))
            .collect();
        return match (values.as_deref(), longhands) {
            (Some([v]), longhands) => longhands
                .iter()
                .map(|l| (l.to_string(), v.clone()))
                .collect(),
            (Some([a, b]), [first, second]) => {
                vec![
                    (first.to_string(), a.clone()),
                    (second.to_string(), b.clone()),
                ]
            }
            _ => vec![],
        };
    }
    // Grid track lists, lines and areas are interpreted by layout, which
    // needs their tokens (`repeat()`, `minmax()`, `1fr`, `span 2`).
    if matches!(
        property,
        "grid-template-columns"
            | "grid-template-rows"
            | "grid-template-areas"
            | "grid-auto-columns"
            | "grid-auto-rows"
            | "grid-column"
            | "grid-row"
            | "grid-area"
            | "box-shadow"
            | "background-image"
            | "content"
    ) {
        let trimmed = trim_ws(value_tokens);
        if !matches!(trimmed, [Token::Ident(_)]) || property.ends_with("areas") {
            return if trimmed.is_empty() {
                vec![]
            } else {
                vec![(property.to_string(), Value::Unparsed(trimmed.to_vec()))]
            };
        }
    }
    match property {
        "margin" | "padding" => expand_box_shorthand_property(property, "", value_tokens, true),
        "border-width" => expand_box_shorthand_property("border", "-width", value_tokens, true),
        "border-style" => expand_box_shorthand_property("border", "-style", value_tokens, false),
        "border-color" => expand_box_shorthand_property("border", "-color", value_tokens, false),
        "border" => expand_border_sides(&["top", "right", "bottom", "left"], value_tokens),
        "border-top" => expand_border_sides(&["top"], value_tokens),
        "border-right" => expand_border_sides(&["right"], value_tokens),
        "border-bottom" => expand_border_sides(&["bottom"], value_tokens),
        "border-left" => expand_border_sides(&["left"], value_tokens),
        "flex" => expand_flex(value_tokens),
        "flex-flow" => expand_flex_flow(value_tokens),
        "gap" | "grid-gap" => expand_pair("row-gap", "column-gap", value_tokens),
        "overflow" => expand_pair("overflow-x", "overflow-y", value_tokens),
        "place-items" => expand_pair("align-items", "justify-items", value_tokens),
        "place-content" => expand_pair("align-content", "justify-content", value_tokens),
        "place-self" => expand_pair("align-self", "justify-self", value_tokens),
        "background" => expand_background(value_tokens),
        "border-radius" => expand_border_radius(value_tokens),
        "list-style" => expand_list_style(value_tokens),
        "text-decoration" => expand_text_decoration(value_tokens),
        "inset" => {
            expand_box_shorthand_property_named(["top", "right", "bottom", "left"], value_tokens)
        }
        "font-family" => {
            // MVP doesn't do font fallback matching -- take the first
            // named font in the comma list and store it as-is.
            let first = trim_ws(value_tokens);
            let name = first.iter().find_map(|t| match t {
                Token::Ident(s) => Some(s.to_ascii_lowercase()),
                Token::QuotedString(s) => Some(s.to_ascii_lowercase()),
                _ => None,
            });
            match name {
                Some(n) => vec![("font-family".to_string(), Value::Keyword(n))],
                None => vec![],
            }
        }
        _ => {
            if let Some(v) = parse_component_value(property, value_tokens) {
                return vec![(property.to_string(), v)];
            }
            let parts = components(value_tokens);
            if parts.len() < 2 {
                return vec![];
            }
            let list: Option<Vec<Value>> = parts
                .iter()
                .map(|p| parse_component_value(property, p))
                .collect();
            match list {
                Some(list) => vec![(property.to_string(), Value::List(list))],
                None => vec![],
            }
        }
    }
}

/// The `;`-separated declarations of a block, skipping nested rules
/// (`&:hover { ... }`, whose own parsing is not supported yet) so that the
/// declarations after one are not swallowed with it. A custom property's value
/// may itself contain braces, so those chunks are read to the next `;`.
fn declaration_chunks(tokens: &[Token]) -> Vec<&[Token]> {
    let mut chunks = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        while matches!(tokens.get(i), Some(Token::Whitespace | Token::Semicolon)) {
            i += 1;
        }
        let start = i;
        let custom = matches!(tokens.get(i), Some(Token::Ident(name)) if name.starts_with("--"));
        let mut depth = 0usize;
        let mut nested_rule = false;
        while i < tokens.len() {
            match &tokens[i] {
                Token::LeftParen | Token::LeftBracket => depth += 1,
                Token::RightParen | Token::RightBracket => depth = depth.saturating_sub(1),
                Token::LeftBrace if custom => depth += 1,
                Token::RightBrace if custom => depth = depth.saturating_sub(1),
                Token::LeftBrace if depth == 0 => {
                    nested_rule = true;
                    break;
                }
                Token::Semicolon if depth == 0 => break,
                _ => {}
            }
            i += 1;
        }
        if nested_rule {
            // Skip the whole `{ ... }` block.
            let mut braces = 0usize;
            while i < tokens.len() {
                match tokens[i] {
                    Token::LeftBrace => braces += 1,
                    Token::RightBrace => {
                        braces -= 1;
                        if braces == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        if start < i {
            chunks.push(&tokens[start..i]);
        }
    }
    chunks
}

/// Parses one declaration block's tokens (the contents between `{` and
/// `}`, not including the braces) into [`Declaration`]s. Declarations
/// that fail to parse (unknown property syntax, unsupported value) are
/// dropped individually rather than failing the whole block.
pub fn parse_declarations(tokens: &[Token]) -> Vec<Declaration> {
    declaration_chunks(tokens)
        .into_iter()
        .filter_map(|decl_tokens| {
            let decl_tokens = trim_ws(decl_tokens);
            if decl_tokens.is_empty() {
                return None;
            }
            let Token::Ident(prop) = &decl_tokens[0] else {
                return None;
            };
            let property = prop.to_ascii_lowercase();
            if decl_tokens.get(1) != Some(&Token::Colon) {
                return None;
            }
            let mut value_tokens = trim_ws(&decl_tokens[2..]);

            let mut important = false;
            if let [rest @ .., Token::Ident(word)] = value_tokens {
                if word.eq_ignore_ascii_case("important") {
                    let rest = trim_ws(rest);
                    if let [before @ .., Token::Delim('!')] = rest {
                        value_tokens = trim_ws(before);
                        important = true;
                    }
                }
            }

            Some((property, value_tokens, important))
        })
        .flat_map(|(property, value_tokens, important)| {
            // Custom properties and anything using `var()` cannot be parsed
            // until the cascade knows the element's custom properties.
            let expanded = if property.starts_with("--") || crate::vars::contains_var(value_tokens)
            {
                vec![(property, Value::Unparsed(value_tokens.to_vec()))]
            } else {
                expand_property(&property, value_tokens)
            };
            expanded
                .into_iter()
                .map(move |(property, value)| Declaration {
                    property,
                    value,
                    important,
                })
        })
        .collect()
}

/// Offset of the `}` that closes a block whose `{` precedes `tokens`,
/// skipping over nested `{ ... }` pairs (a custom property's braces, or a
/// nested rule whose own parsing is not supported yet).
fn matching_brace(tokens: &[Token]) -> Option<usize> {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::LeftBrace => depth += 1,
            Token::RightBrace if depth == 0 => return Some(i),
            Token::RightBrace => depth -= 1,
            _ => {}
        }
    }
    None
}

fn skip_at_rule(tokens: &[Token], start: usize) -> usize {
    let mut i = start;
    let mut depth = 0usize;
    while i < tokens.len() {
        match tokens[i] {
            Token::LeftBrace => depth += 1,
            Token::RightBrace => {
                if depth == 0 {
                    return i + 1;
                }
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            Token::Semicolon if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    i
}

/// Parses a full stylesheet's tokens into [`Rule`]s.
pub fn parse_rules(tokens: &[Token]) -> Vec<Rule> {
    parse_rules_in(tokens, None, 0)
}

fn parse_rules_in(
    tokens: &[Token],
    environment: Option<&crate::MediaEnvironment>,
    depth: usize,
) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Whitespace => i += 1,
            Token::AtKeyword(name)
                if name.eq_ignore_ascii_case("media") && environment.is_some() =>
            {
                let end = skip_at_rule(tokens, i + 1);
                let Some(open) = tokens[i + 1..end]
                    .iter()
                    .position(|t| *t == Token::LeftBrace)
                    .map(|v| i + 1 + v)
                else {
                    i = end;
                    continue;
                };
                if depth < 64
                    && tokens.get(end.saturating_sub(1)) == Some(&Token::RightBrace)
                    && crate::media::matches_tokens(&tokens[i + 1..open], environment.unwrap())
                {
                    rules.extend(parse_rules_in(
                        &tokens[open + 1..end - 1],
                        environment,
                        depth + 1,
                    ));
                }
                i = end;
            }
            Token::AtKeyword(name)
                if name.eq_ignore_ascii_case("layer") || name.eq_ignore_ascii_case("supports") =>
            {
                // `@layer` has no ordering effect yet: its rules join the
                // sheet in source order. `@supports` includes its block only
                // when the engine really supports the condition.
                let end = skip_at_rule(tokens, i + 1);
                let open = tokens[i + 1..end]
                    .iter()
                    .position(|t| *t == Token::LeftBrace)
                    .map(|v| i + 1 + v);
                if let Some(open) = open {
                    let wanted = name.eq_ignore_ascii_case("layer")
                        || crate::supports::condition_holds(&tokens[i + 1..open]);
                    if wanted
                        && depth < 64
                        && tokens.get(end.saturating_sub(1)) == Some(&Token::RightBrace)
                    {
                        rules.extend(parse_rules_in(
                            &tokens[open + 1..end - 1],
                            environment,
                            depth + 1,
                        ));
                    }
                }
                i = end;
            }
            Token::AtKeyword(_) => i = skip_at_rule(tokens, i + 1),
            _ => {
                let Some(brace) = tokens[i..].iter().position(|t| *t == Token::LeftBrace) else {
                    break; // trailing garbage with no block -- nothing more to parse
                };
                let selector_tokens = &tokens[i..i + brace];
                let body_start = i + brace + 1;
                let Some(close_offset) = matching_brace(&tokens[body_start..]) else {
                    break; // unterminated block
                };
                let body_tokens = &tokens[body_start..body_start + close_offset];

                let selectors = parse_selector_list(selector_tokens);
                if !selectors.is_empty() {
                    rules.push(Rule {
                        selectors,
                        declarations: parse_declarations(body_tokens),
                    });
                }
                i = body_start + close_offset + 1;
            }
        }
    }
    rules
}

pub fn parse(input: &str) -> Vec<Rule> {
    parse_rules(&tokenize(input))
}

pub fn parse_with_environment(input: &str, environment: &crate::MediaEnvironment) -> Vec<Rule> {
    parse_rules_in(&tokenize(input), Some(environment), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::SimpleSelector;
    use crate::value::{Color, Length};

    #[test]
    fn single_rule_single_declaration() {
        let rules = parse("p { color: red; }");
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].selectors[0].compounds[0].simple_selectors,
            vec![SimpleSelector::Type("p".to_string())]
        );
        assert_eq!(
            rules[0].declarations,
            vec![Declaration {
                property: "color".to_string(),
                value: Value::Color(Color::Rgba(255, 0, 0, 255)),
                important: false
            }]
        );
    }

    #[test]
    fn multiple_declarations() {
        let rules = parse("p { color: red; font-size: 12px; }");
        assert_eq!(rules[0].declarations.len(), 2);
        assert_eq!(rules[0].declarations[1].property, "font-size");
    }

    #[test]
    fn trailing_semicolon_is_optional() {
        let rules = parse("p { color: red }");
        assert_eq!(rules[0].declarations.len(), 1);
    }

    #[test]
    fn important_declaration() {
        let rules = parse("p { color: red !important; }");
        assert!(rules[0].declarations[0].important);
    }

    #[test]
    fn important_is_case_insensitive_and_tolerates_whitespace() {
        let rules = parse("p { color: red ! IMPORTANT; }");
        assert!(rules[0].declarations[0].important);
        assert_eq!(
            rules[0].declarations[0].value,
            Value::Color(Color::Rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn multiple_selectors_and_multiple_rules() {
        let rules = parse("h1, h2 { color: red; } p { color: blue; }");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].selectors.len(), 2);
    }

    #[test]
    fn unknown_at_rule_with_block_is_skipped_not_fatal() {
        let rules = parse("@media (min-width: 100px) { p { color: red; } } p { color: blue; }");
        // the whole @media block (including its nested rule) is skipped
        // wholesale -- media queries themselves are out of MVP scope --
        // but the sheet keeps parsing afterward.
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].declarations[0].value,
            Value::Color(Color::Rgba(0, 0, 255, 255))
        );
    }

    #[test]
    fn unknown_at_rule_without_block_is_skipped() {
        let rules = parse("@import url(foo.css); p { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn rule_with_invalid_selector_is_dropped() {
        let rules = parse("a::first-line { color: red; } p { color: blue; }");
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].selectors[0].compounds[0].simple_selectors,
            vec![SimpleSelector::Type("p".to_string())]
        );
    }

    #[test]
    fn invalid_declaration_is_dropped_but_siblings_survive() {
        let rules = parse("p { color:; font-size: 12px; }");
        assert_eq!(rules[0].declarations.len(), 1);
        assert_eq!(rules[0].declarations[0].property, "font-size");
    }

    #[test]
    fn margin_shorthand_one_value() {
        let rules = parse("p { margin: 10px; }");
        let decls = &rules[0].declarations;
        assert_eq!(decls.len(), 4);
        for side in ["top", "right", "bottom", "left"] {
            let d = decls
                .iter()
                .find(|d| d.property == format!("margin-{side}"))
                .unwrap();
            assert_eq!(d.value, Value::Length(Length::Px(10.0)));
        }
    }

    #[test]
    fn margin_shorthand_two_values() {
        let rules = parse("p { margin: 10px 20px; }");
        let get = |side: &str| {
            rules[0]
                .declarations
                .iter()
                .find(|d| d.property == format!("margin-{side}"))
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(get("top"), Value::Length(Length::Px(10.0)));
        assert_eq!(get("bottom"), Value::Length(Length::Px(10.0)));
        assert_eq!(get("right"), Value::Length(Length::Px(20.0)));
        assert_eq!(get("left"), Value::Length(Length::Px(20.0)));
    }

    #[test]
    fn margin_shorthand_three_values() {
        let rules = parse("p { margin: 1px 2px 3px; }");
        let get = |side: &str| {
            rules[0]
                .declarations
                .iter()
                .find(|d| d.property == format!("margin-{side}"))
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(get("top"), Value::Length(Length::Px(1.0)));
        assert_eq!(get("right"), Value::Length(Length::Px(2.0)));
        assert_eq!(get("left"), Value::Length(Length::Px(2.0)));
        assert_eq!(get("bottom"), Value::Length(Length::Px(3.0)));
    }

    #[test]
    fn margin_shorthand_four_values() {
        let rules = parse("p { margin: 1px 2px 3px 4px; }");
        let get = |side: &str| {
            rules[0]
                .declarations
                .iter()
                .find(|d| d.property == format!("margin-{side}"))
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(get("top"), Value::Length(Length::Px(1.0)));
        assert_eq!(get("right"), Value::Length(Length::Px(2.0)));
        assert_eq!(get("bottom"), Value::Length(Length::Px(3.0)));
        assert_eq!(get("left"), Value::Length(Length::Px(4.0)));
    }

    #[test]
    fn padding_shorthand_with_bare_zero_normalizes_to_length() {
        let rules = parse("p { padding: 0; }");
        assert_eq!(rules[0].declarations[0].value, Value::Length(Length::Zero));
    }

    #[test]
    fn border_width_shorthand_expands_to_four_sides() {
        let rules = parse("p { border-width: 2px; }");
        let decls = &rules[0].declarations;
        assert_eq!(decls.len(), 4);
        for side in ["top", "right", "bottom", "left"] {
            let d = decls
                .iter()
                .find(|d| d.property == format!("border-{side}-width"))
                .unwrap();
            assert_eq!(d.value, Value::Length(Length::Px(2.0)));
        }
    }

    #[test]
    fn border_style_and_border_color_shorthands_expand_to_four_sides() {
        let rules = parse("p { border-style: dashed; border-color: red; }");
        let decls = &rules[0].declarations;
        let style_top = decls
            .iter()
            .find(|d| d.property == "border-top-style")
            .unwrap();
        assert_eq!(style_top.value, Value::Keyword("dashed".to_string()));
        let color_left = decls
            .iter()
            .find(|d| d.property == "border-left-color")
            .unwrap();
        assert_eq!(color_left.value, Value::Color(Color::Rgba(255, 0, 0, 255)));
    }

    #[test]
    fn combined_border_shorthand_sets_width_style_and_color_on_all_sides() {
        let rules = parse("p { border: 1px solid black; }");
        let decls = &rules[0].declarations;
        assert_eq!(decls.len(), 12, "width+style+color x 4 sides");
        for side in ["top", "right", "bottom", "left"] {
            assert_eq!(
                decls
                    .iter()
                    .find(|d| d.property == format!("border-{side}-width"))
                    .unwrap()
                    .value,
                Value::Length(Length::Px(1.0))
            );
            assert_eq!(
                decls
                    .iter()
                    .find(|d| d.property == format!("border-{side}-style"))
                    .unwrap()
                    .value,
                Value::Keyword("solid".to_string())
            );
            assert_eq!(
                decls
                    .iter()
                    .find(|d| d.property == format!("border-{side}-color"))
                    .unwrap()
                    .value,
                Value::Color(Color::Rgba(0, 0, 0, 255))
            );
        }
    }

    #[test]
    fn combined_border_shorthand_components_are_order_independent() {
        let a = parse("p { border: 1px solid black; }");
        let b = parse("p { border: black 1px solid; }");
        let c = parse("p { border: solid black 1px; }");
        for rules in [&a, &b, &c] {
            assert_eq!(rules[0].declarations.len(), 12);
        }
    }

    #[test]
    fn combined_border_shorthand_tolerates_missing_components() {
        let rules = parse("p { border: solid; }");
        let decls = &rules[0].declarations;
        assert_eq!(decls.len(), 4, "only the style component was present");
        assert_eq!(decls[0].value, Value::Keyword("solid".to_string()));
    }

    #[test]
    fn combined_border_shorthand_accepts_bare_zero_width() {
        let rules = parse("p { border: 0; }");
        let decls = &rules[0].declarations;
        assert_eq!(decls.len(), 4);
        assert_eq!(decls[0].value, Value::Length(Length::Zero));
    }

    #[test]
    fn bare_zero_normalizes_for_length_valued_longhands() {
        let rules = parse("p { width: 0; }");
        assert_eq!(rules[0].declarations[0].value, Value::Length(Length::Zero));
    }

    #[test]
    fn bare_number_stays_a_number_for_non_length_properties() {
        let rules = parse("p { flex-grow: 0; }");
        assert_eq!(rules[0].declarations[0].value, Value::Number(0.0));
    }

    #[test]
    fn font_family_takes_the_first_name_in_the_list() {
        let rules = parse(r#"p { font-family: "Helvetica Neue", Arial, sans-serif; }"#);
        assert_eq!(
            rules[0].declarations[0].value,
            Value::Keyword("helvetica neue".to_string())
        );
    }

    #[test]
    fn whitespace_and_comments_around_rules_are_ignored() {
        let rules = parse("/* comment */ p /* c */ { /* c */ color: red; /* c */ } /* c */");
        assert_eq!(rules.len(), 1);
    }

    fn selectors_of(css: &str) -> usize {
        parse(css).len()
    }

    #[test]
    fn layer_blocks_are_flattened_and_a_bare_layer_statement_is_ignored() {
        let rules = parse(
            "@layer base, utilities; @layer base { p { color: red; } @layer inner { b { color: blue; } } } i { color: green; }",
        );
        assert_eq!(rules.len(), 3, "p, b (nested layer) and i");
    }

    #[test]
    fn supports_includes_its_block_only_when_the_condition_holds() {
        assert_eq!(
            selectors_of("@supports (color: red) { p { color: red; } }"),
            1
        );
        assert_eq!(
            selectors_of("@supports (display: block) { p { color: red; } }"),
            1
        );
        assert_eq!(
            selectors_of("@supports (display: flex) { p { color: red; } }"),
            0,
            "flex is not laid out, so a page must get its fallback"
        );
        assert_eq!(
            selectors_of("@supports (made-up-property: 1) { p { color: red; } }"),
            0
        );
        assert_eq!(
            selectors_of("@supports (--custom: anything) { p { color: red; } }"),
            1
        );
    }

    #[test]
    fn supports_combinators_and_not() {
        for (condition, included) in [
            ("not (display: flex)", true),
            ("not (display: block)", false),
            ("(display: block) and (color: red)", true),
            ("(display: block) and (display: grid)", false),
            ("(display: grid) or (color: red)", true),
            ("(display: grid) or (display: flex)", false),
            ("((display: block))", true),
            (
                "(display: grid) or ((color: red) and (not (display: flex)))",
                true,
            ),
            ("selector(a > b)", true),
            ("selector(", false),
            ("font-tech(color-COLRv1)", false),
        ] {
            let css = format!("@supports {condition} {{ p {{ color: red; }} }}");
            assert_eq!(
                selectors_of(&css),
                usize::from(included),
                "@supports {condition}"
            );
        }
    }

    #[test]
    fn rules_after_a_skipped_supports_block_still_parse() {
        let rules = parse("@supports (display: grid) { p { color: red; } } b { color: blue; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn container_and_property_at_rules_are_skipped() {
        let rules = parse(
            "@container (min-width: 10px) { p { color: red; } } @property --x { syntax: '*'; inherits: false; } b { color: blue; }",
        );
        assert_eq!(rules.len(), 1);
    }

    // ---- custom properties, var(), multi-value properties and shorthands ----

    fn decls(css: &str) -> Vec<(String, Value)> {
        parse(&format!("p {{ {css} }}"))
            .remove(0)
            .declarations
            .into_iter()
            .map(|d| (d.property, d.value))
            .collect()
    }
    fn value_of(css: &str, property: &str) -> Option<Value> {
        decls(css)
            .into_iter()
            .find(|(p, _)| p == property)
            .map(|(_, v)| v)
    }
    fn px(n: f64) -> Value {
        Value::Length(Length::Px(n))
    }

    #[test]
    fn custom_properties_and_var_declarations_stay_unparsed() {
        let d = decls("--gap: 4px 8px; margin: var(--gap); color: red;");
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].0, "--gap");
        assert!(
            matches!(&d[0].1, Value::Unparsed(t) if t.contains(&Token::Dimension(8.0, "px".to_string())))
        );
        assert_eq!(
            d[1].0, "margin",
            "a shorthand is expanded after substitution"
        );
        assert!(matches!(d[1].1, Value::Unparsed(_)));
        assert_eq!(
            d[2],
            (
                "color".to_string(),
                Value::Color(crate::value::Color::Rgba(255, 0, 0, 255))
            )
        );
    }

    #[test]
    fn a_custom_property_value_may_hold_semicolons_in_parentheses_and_braces() {
        let d = decls("--bg: url(data:image/png;base64,AAAA); --obj: {a:b;c:d}; color: red;");
        assert_eq!(d.len(), 3, "{d:?}");
    }

    #[test]
    fn important_applies_to_custom_properties() {
        let rule = parse("p { --x: 1 !important; }").remove(0);
        assert!(rule.declarations[0].important);
    }

    #[test]
    fn function_values_survive_space_splitting() {
        assert_eq!(
            value_of("margin: calc(1px + 2px) 4px;", "margin-top"),
            Some(Value::Calc(crate::Calc::Sum(
                Box::new(crate::Calc::Length(Length::Px(1.0))),
                Box::new(crate::Calc::Length(Length::Px(2.0)))
            )))
        );
        assert_eq!(
            value_of("margin: calc(1px + 2px) 4px;", "margin-left"),
            Some(px(4.0))
        );
        assert_eq!(
            value_of("border: 1px solid rgb(0 0 0 / 50%);", "border-top-color"),
            Some(Value::Color(crate::value::Color::Rgba(0, 0, 0, 128)))
        );
    }

    #[test]
    fn unknown_multi_component_values_become_lists() {
        assert_eq!(
            value_of("aspect-ratio: 16 9;", "aspect-ratio"),
            Some(Value::List(vec![Value::Number(16.0), Value::Number(9.0)]))
        );
        assert_eq!(
            value_of("margin-trim: block inline;", "margin-trim"),
            Some(Value::List(vec![
                Value::Keyword("block".to_string()),
                Value::Keyword("inline".to_string())
            ]))
        );
        assert_eq!(
            value_of("transition: opacity 0.3s;", "transition"),
            None,
            "a component the engine cannot represent drops the declaration"
        );
    }

    #[test]
    fn flex_shorthand_expands_to_its_three_longhands() {
        let flex = |css: &str| {
            let d = decls(css);
            let get = |name: &str| d.iter().find(|(p, _)| p == name).map(|(_, v)| v.clone());
            (get("flex-grow"), get("flex-shrink"), get("flex-basis"))
        };
        let n = Value::Number;
        let auto = || Value::Keyword("auto".to_string());
        assert_eq!(
            flex("flex: none;"),
            (Some(n(0.0)), Some(n(0.0)), Some(auto()))
        );
        assert_eq!(
            flex("flex: auto;"),
            (Some(n(1.0)), Some(n(1.0)), Some(auto()))
        );
        assert_eq!(
            flex("flex: initial;"),
            (Some(n(0.0)), Some(n(1.0)), Some(auto()))
        );
        assert_eq!(
            flex("flex: 2;"),
            (Some(n(2.0)), Some(n(1.0)), Some(Value::Percentage(0.0)))
        );
        assert_eq!(
            flex("flex: 2 3;"),
            (Some(n(2.0)), Some(n(3.0)), Some(Value::Percentage(0.0)))
        );
        assert_eq!(
            flex("flex: 1 0 10px;"),
            (Some(n(1.0)), Some(n(0.0)), Some(px(10.0)))
        );
        assert_eq!(
            flex("flex: 10px;"),
            (Some(n(1.0)), Some(n(1.0)), Some(px(10.0)))
        );
        assert_eq!(
            flex("flex: 1 1 0;"),
            (
                Some(n(1.0)),
                Some(n(1.0)),
                Some(Value::Length(Length::Zero))
            )
        );
        assert_eq!(
            flex("flex: 1 1 0%;"),
            (Some(n(1.0)), Some(n(1.0)), Some(Value::Percentage(0.0)))
        );
        assert_eq!(flex("flex: 1 2 3 4;"), (None, None, None));
    }

    #[test]
    fn flex_flow_gap_inset_and_overflow_shorthands() {
        assert_eq!(
            value_of("flex-flow: column wrap;", "flex-direction"),
            Some(Value::Keyword("column".to_string()))
        );
        assert_eq!(
            value_of("flex-flow: column wrap;", "flex-wrap"),
            Some(Value::Keyword("wrap".to_string()))
        );
        assert_eq!(
            value_of("flex-flow: wrap-reverse;", "flex-wrap"),
            Some(Value::Keyword("wrap-reverse".to_string()))
        );
        assert_eq!(value_of("gap: 4px 8px;", "row-gap"), Some(px(4.0)));
        assert_eq!(value_of("gap: 4px 8px;", "column-gap"), Some(px(8.0)));
        assert_eq!(value_of("gap: 4px;", "column-gap"), Some(px(4.0)));
        assert_eq!(value_of("grid-gap: 6px;", "row-gap"), Some(px(6.0)));
        assert_eq!(value_of("inset: 1px 2px;", "top"), Some(px(1.0)));
        assert_eq!(value_of("inset: 1px 2px;", "right"), Some(px(2.0)));
        assert_eq!(
            value_of("inset: 0;", "left"),
            Some(Value::Length(Length::Zero))
        );
        assert_eq!(
            value_of("overflow: hidden auto;", "overflow-x"),
            Some(Value::Keyword("hidden".to_string()))
        );
        assert_eq!(
            value_of("overflow: hidden auto;", "overflow-y"),
            Some(Value::Keyword("auto".to_string()))
        );
        assert_eq!(
            value_of("overflow: clip;", "overflow-y"),
            Some(Value::Keyword("clip".to_string()))
        );
    }

    #[test]
    fn background_shorthand_keeps_only_the_colour() {
        assert_eq!(
            value_of("background: url(x.png) no-repeat #fff;", "background-color"),
            Some(Value::Color(crate::value::Color::Rgba(255, 255, 255, 255)))
        );
        assert_eq!(
            value_of("background: rgb(1 2 3) url(x.png);", "background-color"),
            Some(Value::Color(crate::value::Color::Rgba(1, 2, 3, 255)))
        );
        assert_eq!(
            value_of("background: url(x.png);", "background-color"),
            Some(Value::Color(crate::value::Color::Rgba(0, 0, 0, 0))),
            "a layer without a colour resets it"
        );
        assert_eq!(
            value_of("background: none;", "background-color"),
            Some(Value::Color(crate::value::Color::Rgba(0, 0, 0, 0)))
        );
    }

    #[test]
    fn per_side_border_shorthands_and_logical_properties() {
        assert_eq!(
            value_of("border-bottom: 2px solid red;", "border-bottom-width"),
            Some(px(2.0))
        );
        assert_eq!(
            value_of("border-bottom: 2px solid red;", "border-bottom-style"),
            Some(Value::Keyword("solid".to_string()))
        );
        assert_eq!(
            value_of("border-bottom: 2px solid red;", "border-top-width"),
            None
        );
        assert_eq!(
            value_of("margin-inline: 4px 8px;", "margin-left"),
            Some(px(4.0))
        );
        assert_eq!(
            value_of("margin-inline: 4px 8px;", "margin-right"),
            Some(px(8.0))
        );
        assert_eq!(
            value_of("margin-block: 1px;", "margin-bottom"),
            Some(px(1.0))
        );
        assert_eq!(
            value_of("padding-inline-start: 3px;", "padding-left"),
            Some(px(3.0))
        );
        assert_eq!(
            value_of("padding-block-end: 3px;", "padding-bottom"),
            Some(px(3.0))
        );
        assert_eq!(
            value_of("inset-inline: 0;", "right"),
            Some(Value::Length(Length::Zero))
        );
        assert_eq!(value_of("inset-block-start: 5px;", "top"), Some(px(5.0)));
        assert_eq!(
            value_of("margin-inline-end: auto;", "margin-right"),
            Some(Value::Keyword("auto".to_string()))
        );
    }

    #[test]
    fn a_nested_rule_inside_a_body_does_not_corrupt_the_sheet() {
        let rules =
            parse("a { color: red; &:hover { color: blue; } margin: 1px; } b { color: green; }");
        assert_eq!(rules.len(), 2, "a and b");
        let props: Vec<_> = rules[0]
            .declarations
            .iter()
            .map(|d| d.property.as_str())
            .collect();
        assert!(
            props.contains(&"color") && props.contains(&"margin-top"),
            "{props:?}"
        );
    }

    #[test]
    fn text_decoration_shorthand_splits_into_line_and_colour() {
        assert_eq!(
            value_of("text-decoration: underline;", "text-decoration-line"),
            Some(Value::Keyword("underline".to_string()))
        );
        assert_eq!(
            value_of(
                "text-decoration: underline line-through red;",
                "text-decoration-line"
            ),
            Some(Value::List(vec![
                Value::Keyword("underline".to_string()),
                Value::Keyword("line-through".to_string())
            ]))
        );
        assert_eq!(
            value_of("text-decoration: none;", "text-decoration-line"),
            Some(Value::Keyword("none".to_string()))
        );
        assert_eq!(
            value_of(
                "text-decoration: underline dotted #00f 2px;",
                "text-decoration-color"
            ),
            Some(Value::Color(crate::value::Color::Rgba(0, 0, 255, 255)))
        );
        assert_eq!(
            value_of("text-decoration: sparkly;", "text-decoration-line"),
            None
        );
        assert_eq!(
            value_of("text-decoration-line: underline;", "text-decoration-line"),
            Some(Value::Keyword("underline".to_string())),
            "the longhand passes through"
        );
    }

    #[test]
    fn border_radius_expands_to_the_four_corners() {
        let d = expand_property("border-radius", &tokenize("4px 8px"));
        let get = |name: &str| d.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
        assert_eq!(get("border-top-left-radius"), Some(px(4.0)));
        assert_eq!(get("border-top-right-radius"), Some(px(8.0)));
        assert_eq!(get("border-bottom-right-radius"), Some(px(4.0)));
        assert_eq!(get("border-bottom-left-radius"), Some(px(8.0)));
        let d = expand_property("border-radius", &tokenize("50%"));
        assert_eq!(d.len(), 4);
        assert!(matches!(d[0].1, Value::Percentage(p) if p == 50.0));
        assert!(expand_property("border-radius", &tokenize("red")).is_empty());
    }

    #[test]
    fn a_background_shorthand_keeps_its_gradient_and_resets_the_image() {
        let d = expand_property(
            "background",
            &tokenize("linear-gradient(red, blue) no-repeat #fff"),
        );
        assert!(d
            .iter()
            .any(|(n, v)| n == "background-image" && matches!(v, Value::Unparsed(_))));
        let d = expand_property("background", &tokenize("#fff"));
        assert!(d
            .iter()
            .any(|(n, v)| n == "background-image" && *v == Value::Keyword("none".into())));
    }
}
