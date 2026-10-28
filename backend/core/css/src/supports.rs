// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `@supports` condition evaluation. A condition is true only for what this
//! engine genuinely implements, so a page that tests for a feature gets its
//! fallback styling rather than rules the layout cannot honour. Growing
//! `declaration_supported` is part of adding a property or value.

use crate::selector::parse_selector_list;
use crate::tokenizer::Token;
use crate::value::parse_value;

/// Properties the cascade and layout honour, with the keyword values
/// accepted for the ones where that is not "any parsable value".
const SUPPORTED_PROPERTIES: &[&str] = &[
    "background-color",
    "border-bottom-width",
    "border-color",
    "border-left-color",
    "border-left-width",
    "border-right-width",
    "border-style",
    "border-top-style",
    "border-top-width",
    "border-width",
    "box-sizing",
    "color",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "height",
    "line-height",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "opacity",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "text-align",
    "visibility",
    "width",
];

/// Display keywords layout currently generates boxes for.
const SUPPORTED_DISPLAY: &[&str] = &["block", "inline", "none"];

fn declaration_supported(property: &str, value: &[Token]) -> bool {
    let property = property.to_ascii_lowercase();
    if property.starts_with("--") {
        return true; // any custom property is valid, whatever its value
    }
    let value: Vec<Token> = value
        .iter()
        .filter(|t| !matches!(t, Token::Whitespace | Token::Eof))
        .cloned()
        .collect();
    if value.is_empty() {
        return false;
    }
    if property == "display" {
        return matches!(value.as_slice(), [Token::Ident(word)]
            if SUPPORTED_DISPLAY.iter().any(|d| d.eq_ignore_ascii_case(word)));
    }
    SUPPORTED_PROPERTIES.contains(&property.as_str()) && parse_value(&value).is_some()
}

/// Whether the `@supports` prelude `tokens` (everything before the block) holds.
pub(crate) fn condition_holds(tokens: &[Token]) -> bool {
    let tokens: Vec<Token> = tokens
        .iter()
        .filter(|t| !matches!(t, Token::Whitespace | Token::Eof))
        .cloned()
        .collect();
    matches!(condition(&tokens), Some((true, used)) if used == tokens.len())
}

fn is_word(token: Option<&Token>, word: &str) -> bool {
    matches!(token, Some(Token::Ident(w)) if w.eq_ignore_ascii_case(word))
}

/// `<supports-condition>`: `not <in-parens>` or `<in-parens> [and|or <in-parens>]*`
/// where one combinator may not be mixed with the other without parentheses.
fn condition(tokens: &[Token]) -> Option<(bool, usize)> {
    if is_word(tokens.first(), "not") {
        let (value, used) = in_parens(&tokens[1..])?;
        return Some((!value, used + 1));
    }
    let (mut value, mut used) = in_parens(tokens)?;
    let combinator = match tokens.get(used) {
        Some(t) if is_word(Some(t), "and") => Some("and"),
        Some(t) if is_word(Some(t), "or") => Some("or"),
        _ => None,
    };
    let Some(combinator) = combinator else {
        return Some((value, used));
    };
    while is_word(tokens.get(used), combinator) {
        let (next, consumed) = in_parens(&tokens[used + 1..])?;
        value = if combinator == "and" {
            value && next
        } else {
            value || next
        };
        used += 1 + consumed;
    }
    if is_word(tokens.get(used), "and") || is_word(tokens.get(used), "or") {
        return None; // `and` mixed with `or` needs parentheses
    }
    Some((value, used))
}

/// A parenthesised condition or declaration, `selector(...)`, or a
/// general-enclosed form the engine does not understand (false).
fn in_parens(tokens: &[Token]) -> Option<(bool, usize)> {
    let (function, open) = match tokens.first()? {
        Token::LeftParen => (None, 0),
        Token::Ident(name) if matches!(tokens.get(1), Some(Token::LeftParen)) => {
            (Some(name.to_ascii_lowercase()), 1)
        }
        _ => return None,
    };
    let mut depth = 0usize;
    let mut close = None;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::LeftParen => depth += 1,
            Token::RightParen => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let inner = &tokens[open + 1..close];
    let value = match function.as_deref() {
        Some("selector") => !parse_selector_list(inner).is_empty(),
        Some(_) => false, // font-tech(), font-format(), ...: not supported
        None => match inner {
            [Token::LeftParen, ..] => {
                matches!(condition(inner), Some((true, n)) if n == inner.len())
            }
            [Token::Ident(word), ..] if word.eq_ignore_ascii_case("not") => {
                matches!(condition(inner), Some((true, n)) if n == inner.len())
            }
            [Token::Ident(property), Token::Colon, rest @ ..] => {
                declaration_supported(property, rest)
            }
            _ => false,
        },
    };
    Some((value, close + 1))
}
