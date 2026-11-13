// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `@font-feature-values`: the names a page gives to the font-specific alternates of a family
//! (`stylistic`, `styleset`, `character-variant`, `swash`, `ornaments`, `annotation`), and the
//! resolution of `font-variant-alternates` names against them into OpenType feature settings.

use crate::cascade::ComputedStyle;
use crate::tokenizer::{tokenize, Token};
use crate::value::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Stylistic,
    Styleset,
    CharacterVariant,
    Swash,
    Ornaments,
    Annotation,
}

/// The feature values a page defines, by family (lowercased), kind and name (case-sensitive).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureValues {
    families: HashMap<String, HashMap<(Kind, String), Vec<u32>>>,
}

impl FeatureValues {
    pub fn is_empty(&self) -> bool {
        self.families.is_empty()
    }

    /// Adds `other`'s definitions: for a name defined in both, `other`'s wins, as later rules do.
    pub fn merge(&mut self, other: FeatureValues) {
        for (family, defs) in other.families {
            self.families.entry(family).or_default().extend(defs);
        }
    }

    /// Resolves the names of `style`'s `font-variant-alternates` against the values the page
    /// defines for the first of its families that defines any, and stores the feature settings
    /// they come to for the shaper (`-blueice-alternates`). A name nobody defined contributes
    /// nothing.
    pub fn resolve(&self, style: &mut ComputedStyle) {
        let Some(Value::Unparsed(tokens)) = style.other.get("font-variant-alternates").cloned() else {
            style.other.remove("-blueice-alternates");
            return;
        };
        let families = style.font_family.clone().unwrap_or_default();
        let defs = families
            .split(',')
            .map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_ascii_lowercase())
            .find_map(|f| self.families.get(&f));
        let Some(defs) = defs else {
            style.other.remove("-blueice-alternates");
            return;
        };
        let mut settings: Vec<([u8; 4], u32)> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let Token::Ident(function) = &tokens[i] else {
                i += 1;
                continue;
            };
            if !matches!(tokens.get(i + 1), Some(Token::LeftParen)) {
                i += 1;
                continue;
            }
            let end = tokens[i + 2..]
                .iter()
                .position(|t| matches!(t, Token::RightParen))
                .map_or(tokens.len(), |p| i + 2 + p);
            let kind = match function.to_ascii_lowercase().as_str() {
                "stylistic" => Some(Kind::Stylistic),
                "styleset" => Some(Kind::Styleset),
                "character-variant" => Some(Kind::CharacterVariant),
                "swash" => Some(Kind::Swash),
                "ornaments" => Some(Kind::Ornaments),
                "annotation" => Some(Kind::Annotation),
                _ => None,
            };
            if let Some(kind) = kind {
                for token in &tokens[i + 2..end] {
                    let Token::Ident(name) = token else { continue };
                    let Some(values) = defs.get(&(kind, name.clone())) else { continue };
                    push_settings(kind, values, &mut settings);
                }
            }
            i = end + 1;
        }
        if settings.is_empty() {
            style.other.remove("-blueice-alternates");
            return;
        }
        // The shaper reads them as `font-feature-settings` text.
        let mut text = String::new();
        for (tag, value) in &settings {
            if !text.is_empty() {
                text.push(',');
            }
            text.push_str(&format!("\"{}\" {value}", String::from_utf8_lossy(tag)));
        }
        style.other.insert("-blueice-alternates".to_string(), Value::Unparsed(tokenize(&text)));
    }
}

/// The OpenType settings one defined name stands for (CSS Fonts 4, 6.9.2).
fn push_settings(kind: Kind, values: &[u32], out: &mut Vec<([u8; 4], u32)>) {
    let tag = |s: String| -> [u8; 4] {
        let mut t = *b"    ";
        for (slot, byte) in t.iter_mut().zip(s.bytes()) {
            *slot = byte;
        }
        t
    };
    match kind {
        Kind::Stylistic => {
            if let Some(&n) = values.first() {
                out.push((*b"salt", n));
            }
        }
        Kind::Swash => {
            if let Some(&n) = values.first() {
                out.push((*b"swsh", n));
                out.push((*b"cswh", n));
            }
        }
        Kind::Ornaments => {
            if let Some(&n) = values.first() {
                out.push((*b"ornm", n));
            }
        }
        Kind::Annotation => {
            if let Some(&n) = values.first() {
                out.push((*b"nalt", n));
            }
        }
        // Values 1 to 99 turn on ss01 to ss99; others do nothing.
        Kind::Styleset => {
            for &n in values {
                if (1..=99).contains(&n) {
                    out.push((tag(format!("ss{n:02}")), 1));
                }
            }
        }
        // One value is the feature, two are the feature and the value it is given.
        Kind::CharacterVariant => {
            if let Some(&n) = values.first() {
                if (1..=99).contains(&n) {
                    out.push((tag(format!("cv{n:02}")), values.get(1).copied().unwrap_or(1)));
                }
            }
        }
    }
}

/// The `@font-feature-values` rules in `css`. A rule with a generic family name or a malformed
/// prelude is ignored whole; a bad declaration is ignored alone.
pub fn font_feature_values(css: &str) -> FeatureValues {
    let tokens = tokenize(css);
    let mut out = FeatureValues::default();
    let mut i = 0;
    while i < tokens.len() {
        let Token::AtKeyword(name) = &tokens[i] else {
            i += 1;
            continue;
        };
        if !name.eq_ignore_ascii_case("font-feature-values") {
            i += 1;
            continue;
        }
        let Some(open) = tokens[i..].iter().position(|t| matches!(t, Token::LeftBrace)) else {
            break;
        };
        let open = i + open;
        let close = matching_brace(&tokens, open);
        let families = prelude_families(&tokens[i + 1..open]);
        if let Some(families) = families {
            let defs = block_definitions(&tokens[open + 1..close]);
            for family in families {
                out.families.entry(family).or_default().extend(defs.clone());
            }
        }
        i = close + 1;
    }
    out
}

/// Index of the `}` that closes the `{` at `open`, or the end of `tokens`.
fn matching_brace(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0;
    for (i, t) in tokens.iter().enumerate().skip(open) {
        match t {
            Token::LeftBrace => depth += 1,
            Token::RightBrace => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// The lowercased family names of a prelude: comma-separated, each a string or a run of
/// identifiers. `None` for a generic family or an empty one.
fn prelude_families(tokens: &[Token]) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for family in tokens.split(|t| matches!(t, Token::Comma)) {
        let mut name = String::new();
        for t in family {
            match t {
                Token::QuotedString(s) => name = s.clone(),
                Token::Ident(w) => {
                    if !name.is_empty() {
                        name.push(' ');
                    }
                    name.push_str(w);
                }
                Token::Whitespace => {}
                _ => return None,
            }
        }
        let lower = name.to_ascii_lowercase();
        if lower.is_empty()
            || matches!(
                lower.as_str(),
                "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui" | "emoji" | "math"
                    | "fangsong" | "ui-serif" | "ui-sans-serif" | "ui-monospace" | "ui-rounded"
            )
        {
            return None;
        }
        out.push(lower);
    }
    (!out.is_empty()).then_some(out)
}

/// The definitions in a rule's block: nested at-rules of each kind holding `name: n n;`.
fn block_definitions(tokens: &[Token]) -> HashMap<(Kind, String), Vec<u32>> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        let Token::AtKeyword(name) = &tokens[i] else {
            i += 1;
            continue;
        };
        let Some(open) = tokens[i..].iter().position(|t| matches!(t, Token::LeftBrace)) else {
            break;
        };
        let open = i + open;
        let close = matching_brace(tokens, open);
        let kind = match name.to_ascii_lowercase().as_str() {
            "stylistic" => Some(Kind::Stylistic),
            "styleset" => Some(Kind::Styleset),
            "character-variant" => Some(Kind::CharacterVariant),
            "swash" => Some(Kind::Swash),
            "ornaments" => Some(Kind::Ornaments),
            "annotation" => Some(Kind::Annotation),
            _ => None,
        };
        if let Some(kind) = kind {
            let body = &tokens[open + 1..close.min(tokens.len())];
            for declaration in body.split(|t| matches!(t, Token::Semicolon)) {
                let Some(colon) = declaration.iter().position(|t| matches!(t, Token::Colon)) else {
                    continue;
                };
                let names: Vec<&Token> = declaration[..colon].iter().filter(|t| !matches!(t, Token::Whitespace)).collect();
                let [Token::Ident(feature_name)] = names.as_slice() else { continue };
                let mut numbers = Vec::new();
                let mut valid = true;
                for t in &declaration[colon + 1..] {
                    match t {
                        Token::Number(n) if *n >= 0.0 && n.fract() == 0.0 => numbers.push(*n as u32),
                        Token::Whitespace => {}
                        _ => valid = false,
                    }
                }
                let fits = match kind {
                    Kind::CharacterVariant => (1..=2).contains(&numbers.len()),
                    Kind::Styleset => !numbers.is_empty(),
                    _ => numbers.len() == 1,
                };
                if valid && fits {
                    out.insert((kind, feature_name.clone()), numbers);
                }
            }
        }
        i = close + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(family: &str, alternates: &str) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.font_family = Some(family.to_string());
        style.other.insert("font-variant-alternates".into(), Value::Unparsed(tokenize(alternates)));
        style
    }

    fn resolved(css: &str, family: &str, alternates: &str) -> Vec<(String, u32)> {
        let mut s = style(family, alternates);
        font_feature_values(css).resolve(&mut s);
        crate::font_features(&s)
            .into_iter()
            .map(|(t, v)| (String::from_utf8(t.to_vec()).unwrap().trim().to_string(), v))
            .collect()
    }

    const CSS: &str = "@font-feature-values Noble Script { \
        @swash { swishy: 1; flowing: 2; } \
        @stylistic { fancy: 3; } \
        @styleset { nice-style: 12; double: 1 2 3; } \
        @character-variant { alt-a: 1; alt-b: 2 3; } \
        @ornaments { bullet: 4; } @annotation { circled: 1; } }";

    #[test]
    fn names_resolve_to_the_features_the_spec_gives_them() {
        assert_eq!(resolved(CSS, "Noble Script", "swash(flowing)"), [("swsh".to_string(), 2), ("cswh".to_string(), 2)]);
        assert_eq!(resolved(CSS, "Noble Script", "stylistic(fancy)"), [("salt".to_string(), 3)]);
        assert_eq!(resolved(CSS, "Noble Script", "styleset(nice-style)"), [("ss12".to_string(), 1)]);
        assert_eq!(
            resolved(CSS, "Noble Script", "styleset(double, nice-style)"),
            [("ss01".to_string(), 1), ("ss02".to_string(), 1), ("ss03".to_string(), 1), ("ss12".to_string(), 1)]
        );
        // one value is the feature; two are the feature and its value
        assert_eq!(resolved(CSS, "Noble Script", "character-variant(alt-a)"), [("cv01".to_string(), 1)]);
        assert_eq!(resolved(CSS, "Noble Script", "character-variant(alt-b)"), [("cv02".to_string(), 3)]);
        assert_eq!(resolved(CSS, "Noble Script", "ornaments(bullet) annotation(circled)"), [("ornm".to_string(), 4), ("nalt".to_string(), 1)]);
        // historical-forms needs no name
        assert_eq!(resolved(CSS, "Noble Script", "historical-forms"), [("hist".to_string(), 1)]);
    }

    #[test]
    fn a_name_nobody_defined_a_family_that_has_none_and_wrong_case_do_nothing() {
        assert!(resolved(CSS, "Noble Script", "swash(unknown)").is_empty());
        assert!(resolved(CSS, "Noble Script", "swash(FLOWING)").is_empty(), "names are case-sensitive");
        assert!(resolved(CSS, "Other Family", "swash(flowing)").is_empty());
        // the family is matched without regard to case, and the first family that has values is used
        assert_eq!(resolved(CSS, "Fallback, \"noble script\"", "stylistic(fancy)"), [("salt".to_string(), 3)]);
        // a number is not a name
        assert!(resolved(CSS, "Noble Script", "stylistic(2)").is_empty());
    }

    #[test]
    fn rules_for_the_same_family_merge_and_the_last_definition_wins() {
        let css = "@font-feature-values A { @swash { s: 1; } } @font-feature-values a, B { @swash { s: 2; t: 5; } }";
        assert_eq!(resolved(css, "A", "swash(s)"), [("swsh".to_string(), 2), ("cswh".to_string(), 2)]);
        assert_eq!(resolved(css, "b", "swash(t)"), [("swsh".to_string(), 5), ("cswh".to_string(), 5)]);
    }

    #[test]
    fn bad_declarations_and_rules_are_ignored_alone() {
        // a generic family makes the whole rule invalid
        assert!(font_feature_values("@font-feature-values serif { @swash { s: 1; } }").is_empty());
        // a declaration with too many values, a negative or fractional one, and an unknown block
        let css = "@font-feature-values F { @character-variant { a: 1 2 3; b: 2; } @swash { c: -1; d: 1.5; e: 2; } @bogus { f: 1; } }";
        assert_eq!(resolved(css, "F", "character-variant(a)"), Vec::<(String, u32)>::new());
        assert_eq!(resolved(css, "F", "character-variant(b)"), [("cv02".to_string(), 1)]);
        assert!(resolved(css, "F", "swash(c)").is_empty() && resolved(css, "F", "swash(d)").is_empty());
        assert_eq!(resolved(css, "F", "swash(e)").len(), 2);
    }

    #[test]
    fn out_of_range_styleset_values_enable_nothing() {
        let css = "@font-feature-values F { @styleset { big: 100; zero: 0; ok: 99; } }";
        assert!(resolved(css, "F", "styleset(big)").is_empty());
        assert!(resolved(css, "F", "styleset(zero)").is_empty());
        assert_eq!(resolved(css, "F", "styleset(ok)"), [("ss99".to_string(), 1)]);
    }
}
