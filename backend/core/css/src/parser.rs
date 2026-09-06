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
use crate::value::{parse_value, zero_as_length, Value};

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

fn split_on(tokens: &[Token], is_sep: impl Fn(&Token) -> bool) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, t) in tokens.iter().enumerate() {
        if is_sep(t) {
            parts.push(&tokens[start..i]);
            start = i + 1;
        }
    }
    parts.push(&tokens[start..]);
    parts
}

fn trim_ws(tokens: &[Token]) -> &[Token] {
    let start = tokens.iter().position(|t| *t != Token::Whitespace).unwrap_or(tokens.len());
    let end = tokens.iter().rposition(|t| *t != Token::Whitespace).map(|i| i + 1).unwrap_or(0);
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

fn expand_property(property: &str, value_tokens: &[Token]) -> Vec<(String, Value)> {
    match property {
        "margin" | "padding" => {
            let components: Option<Vec<Value>> = split_on(value_tokens, |t| *t == Token::Whitespace)
                .into_iter()
                .map(trim_ws)
                .filter(|c| !c.is_empty())
                .map(|c| parse_component_value(&format!("{property}-top"), c))
                .collect();
            let Some(components) = components else { return vec![] };
            let Some([top, right, bottom, left]) = expand_box_shorthand(components) else { return vec![] };
            vec![
                (format!("{property}-top"), top),
                (format!("{property}-right"), right),
                (format!("{property}-bottom"), bottom),
                (format!("{property}-left"), left),
            ]
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
        _ => match parse_component_value(property, value_tokens) {
            Some(v) => vec![(property.to_string(), v)],
            None => vec![],
        },
    }
}

/// Parses one declaration block's tokens (the contents between `{` and
/// `}`, not including the braces) into [`Declaration`]s. Declarations
/// that fail to parse (unknown property syntax, unsupported value) are
/// dropped individually rather than failing the whole block.
pub fn parse_declarations(tokens: &[Token]) -> Vec<Declaration> {
    split_on(tokens, |t| *t == Token::Semicolon)
        .into_iter()
        .filter_map(|decl_tokens| {
            let decl_tokens = trim_ws(decl_tokens);
            if decl_tokens.is_empty() {
                return None;
            }
            let Token::Ident(prop) = &decl_tokens[0] else { return None };
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
            expand_property(&property, value_tokens)
                .into_iter()
                .map(move |(property, value)| Declaration { property, value, important })
        })
        .collect()
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
    let mut rules = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Whitespace => i += 1,
            Token::AtKeyword(_) => i = skip_at_rule(tokens, i + 1),
            _ => {
                let Some(brace) = tokens[i..].iter().position(|t| *t == Token::LeftBrace) else {
                    break; // trailing garbage with no block -- nothing more to parse
                };
                let selector_tokens = &tokens[i..i + brace];
                let body_start = i + brace + 1;
                let Some(close_offset) = tokens[body_start..].iter().position(|t| *t == Token::RightBrace) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::SimpleSelector;
    use crate::value::{Color, Length};

    #[test]
    fn single_rule_single_declaration() {
        let rules = parse("p { color: red; }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selectors[0].compounds[0].simple_selectors, vec![SimpleSelector::Type("p".to_string())]);
        assert_eq!(
            rules[0].declarations,
            vec![Declaration { property: "color".to_string(), value: Value::Color(Color::Rgba(255, 0, 0, 255)), important: false }]
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
        assert_eq!(rules[0].declarations[0].value, Value::Color(Color::Rgba(255, 0, 0, 255)));
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
        assert_eq!(rules[0].declarations[0].value, Value::Color(Color::Rgba(0, 0, 255, 255)));
    }

    #[test]
    fn unknown_at_rule_without_block_is_skipped() {
        let rules = parse("@import url(foo.css); p { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn rule_with_invalid_selector_is_dropped() {
        let rules = parse("a:hover { color: red; } p { color: blue; }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selectors[0].compounds[0].simple_selectors, vec![SimpleSelector::Type("p".to_string())]);
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
            let d = decls.iter().find(|d| d.property == format!("margin-{side}")).unwrap();
            assert_eq!(d.value, Value::Length(Length::Px(10.0)));
        }
    }

    #[test]
    fn margin_shorthand_two_values() {
        let rules = parse("p { margin: 10px 20px; }");
        let get = |side: &str| rules[0].declarations.iter().find(|d| d.property == format!("margin-{side}")).unwrap().value.clone();
        assert_eq!(get("top"), Value::Length(Length::Px(10.0)));
        assert_eq!(get("bottom"), Value::Length(Length::Px(10.0)));
        assert_eq!(get("right"), Value::Length(Length::Px(20.0)));
        assert_eq!(get("left"), Value::Length(Length::Px(20.0)));
    }

    #[test]
    fn margin_shorthand_three_values() {
        let rules = parse("p { margin: 1px 2px 3px; }");
        let get = |side: &str| rules[0].declarations.iter().find(|d| d.property == format!("margin-{side}")).unwrap().value.clone();
        assert_eq!(get("top"), Value::Length(Length::Px(1.0)));
        assert_eq!(get("right"), Value::Length(Length::Px(2.0)));
        assert_eq!(get("left"), Value::Length(Length::Px(2.0)));
        assert_eq!(get("bottom"), Value::Length(Length::Px(3.0)));
    }

    #[test]
    fn margin_shorthand_four_values() {
        let rules = parse("p { margin: 1px 2px 3px 4px; }");
        let get = |side: &str| rules[0].declarations.iter().find(|d| d.property == format!("margin-{side}")).unwrap().value.clone();
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
        assert_eq!(rules[0].declarations[0].value, Value::Keyword("helvetica neue".to_string()));
    }

    #[test]
    fn whitespace_and_comments_around_rules_are_ignored() {
        let rules = parse("/* comment */ p /* c */ { /* c */ color: red; /* c */ } /* c */");
        assert_eq!(rules.len(), 1);
    }
}
