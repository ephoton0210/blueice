// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Custom properties (`--name`) and `var()` substitution. A custom property's
//! value is kept as tokens; an element's set is its parent's plus its own
//! (custom properties inherit), each own value resolved against that set. A
//! declaration that uses `var()` is substituted at computed-value time and
//! only then parsed, which is why `Value::Unparsed` exists.

use crate::tokenizer::Token;
use std::collections::{HashMap, HashSet};

pub(crate) type CustomProperties = HashMap<String, Vec<Token>>;

/// Substitution may nest `var()` this deep before it gives up (a guard
/// against pathological chains on top of the explicit cycle check).
const MAX_DEPTH: usize = 32;

/// Whether `tokens` contain a `var(` call anywhere.
pub(crate) fn contains_var(tokens: &[Token]) -> bool {
    tokens.windows(2).any(|pair| {
        matches!(&pair[0], Token::Ident(name) if name.eq_ignore_ascii_case("var"))
            && pair[1] == Token::LeftParen
    })
}

/// Index of the `)` matching the `(` at `open`.
fn matching_paren(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::LeftParen => depth += 1,
            Token::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn trim(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|t| *t != Token::Whitespace)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|t| *t != Token::Whitespace)
        .map_or(start, |i| i + 1);
    &tokens[start..end]
}

/// `tokens` with every `var(--name[, fallback])` replaced by the named
/// property's value, or by the fallback when the property is not set.
/// `None` when a reference has neither (the declaration is then invalid at
/// computed-value time).
pub(crate) fn substitute(tokens: &[Token], props: &CustomProperties) -> Option<Vec<Token>> {
    substitute_at(tokens, props, 0)
}

fn substitute_at(tokens: &[Token], props: &CustomProperties, depth: usize) -> Option<Vec<Token>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let is_var = matches!(&tokens[i], Token::Ident(name) if name.eq_ignore_ascii_case("var"))
            && tokens.get(i + 1) == Some(&Token::LeftParen);
        if !is_var {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }
        let close = matching_paren(tokens, i + 1)?;
        let inner = &tokens[i + 2..close];
        let comma = inner.iter().position(|t| *t == Token::Comma);
        let (name_tokens, fallback) = match comma {
            Some(c) => (&inner[..c], Some(&inner[c + 1..])),
            None => (inner, None),
        };
        let Some(Token::Ident(name)) = trim(name_tokens).first() else {
            return None;
        };
        let replacement = match props.get(name) {
            Some(value) => substitute_at(value, props, depth + 1)?,
            None => substitute_at(trim(fallback?), props, depth + 1)?,
        };
        out.extend(replacement);
        i = close + 1;
    }
    Some(out)
}

/// An element's custom properties: `inherited` (already resolved) overlaid
/// with `own` (raw values, in cascade order of precedence). A value that
/// refers to itself, directly or through others, is invalid and removes the
/// property -- including an inherited one of that name.
pub(crate) fn resolve(
    inherited: &CustomProperties,
    own: HashMap<String, Vec<Token>>,
) -> CustomProperties {
    if own.is_empty() {
        return inherited.clone();
    }
    let mut result = inherited.clone();
    let mut resolved: HashMap<String, Option<Vec<Token>>> = HashMap::new();
    // In name order: which chains are too deep to trust must not depend on the
    // map's iteration order.
    let mut names: Vec<&String> = own.keys().collect();
    names.sort();
    for name in names {
        resolve_one(name, &own, inherited, &mut resolved, &mut HashSet::new());
    }
    for (name, value) in resolved {
        match value {
            Some(value) => {
                result.insert(name, value);
            }
            None => {
                result.remove(&name);
            }
        }
    }
    result
}

fn resolve_one(
    name: &str,
    own: &HashMap<String, Vec<Token>>,
    inherited: &CustomProperties,
    resolved: &mut HashMap<String, Option<Vec<Token>>>,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Token>> {
    if let Some(done) = resolved.get(name) {
        return done.clone();
    }
    let Some(raw) = own.get(name) else {
        return inherited.get(name).cloned();
    };
    if visiting.len() >= MAX_DEPTH || !visiting.insert(name.to_string()) {
        return None; // a cycle, or a chain too deep to trust
    }
    // Make every property this value mentions available, resolved first.
    let mut view = inherited.clone();
    let mut valid = true;
    for token in raw {
        if let Token::Ident(mentioned) = token {
            if mentioned.starts_with("--") && mentioned != name {
                match resolve_one(mentioned, own, inherited, resolved, visiting) {
                    Some(value) => {
                        view.insert(mentioned.clone(), value);
                    }
                    // An invalid own property must not fall back to an
                    // inherited value of the same name.
                    None => {
                        view.remove(mentioned);
                    }
                }
            } else if mentioned == name {
                valid = false; // refers to itself
            }
        }
    }
    visiting.remove(name);
    let value = if valid { substitute(raw, &view) } else { None };
    resolved.insert(name.to_string(), value.clone());
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn props(pairs: &[(&str, &str)]) -> CustomProperties {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), tokenize(v)))
            .collect()
    }
    fn sub(css: &str, p: &CustomProperties) -> Option<String> {
        substitute(&tokenize(css), p).map(|t| {
            t.iter()
                .map(|t| match t {
                    Token::Ident(s) => s.clone(),
                    Token::Dimension(n, u) => format!("{n}{u}"),
                    Token::Number(n) => n.to_string(),
                    Token::Whitespace => " ".to_string(),
                    Token::Comma => ",".to_string(),
                    other => format!("{other:?}"),
                })
                .collect()
        })
    }

    #[test]
    fn detects_var_calls() {
        assert!(contains_var(&tokenize("calc(var(--a) * 2)")));
        assert!(contains_var(&tokenize("VAR(--a)")));
        assert!(!contains_var(&tokenize("variable(--a)")));
        assert!(!contains_var(&tokenize("1px")));
    }

    #[test]
    fn substitutes_values_and_fallbacks() {
        let p = props(&[("--a", "4px"), ("--b", "red")]);
        assert_eq!(sub("var(--a)", &p).as_deref(), Some("4px"));
        assert_eq!(sub("var(--missing, 7px)", &p).as_deref(), Some("7px"));
        assert_eq!(
            sub("var(--a, 7px)", &p).as_deref(),
            Some("4px"),
            "the fallback is unused"
        );
        assert_eq!(sub("0 var(--a) 0", &p).as_deref(), Some("0 4px 0"));
        assert_eq!(sub("var(--missing)", &p), None, "no value and no fallback");
        assert_eq!(
            sub("var(--missing, )", &p).as_deref(),
            Some(""),
            "an empty fallback is valid"
        );
        assert_eq!(sub("var()", &p), None);
    }

    #[test]
    fn fallbacks_may_nest_and_contain_commas() {
        let p = props(&[("--b", "9px")]);
        assert_eq!(sub("var(--a, var(--b))", &p).as_deref(), Some("9px"));
        assert_eq!(sub("var(--a, var(--c, 1px))", &p).as_deref(), Some("1px"));
        assert_eq!(
            sub("var(--a, 1px, 2px)", &p).as_deref(),
            Some("1px, 2px"),
            "all after the first comma"
        );
    }

    #[test]
    fn custom_properties_may_refer_to_each_other_and_inherit() {
        let parent = props(&[("--base", "10px")]);
        let own: HashMap<String, Vec<Token>> = props(&[
            ("--wide", "calc(var(--base) * 2)"),
            ("--base2", "var(--wide)"),
        ]);
        let resolved = resolve(&parent, own);
        assert_eq!(resolved.get("--base"), parent.get("--base"));
        let text = |name: &str| {
            resolved[name]
                .iter()
                .map(|t| format!("{t:?}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(
            text("--wide").contains("Dimension(10.0, \"px\")"),
            "{}",
            text("--wide")
        );
        assert!(
            text("--base2").contains("Dimension(10.0, \"px\")"),
            "chained"
        );
    }

    #[test]
    fn a_cycle_invalidates_the_property_and_shadows_an_inherited_one() {
        let parent = props(&[("--a", "1px"), ("--keep", "2px")]);
        let own = props(&[
            ("--a", "var(--b)"),
            ("--b", "var(--a)"),
            ("--self", "var(--self)"),
        ]);
        let resolved = resolve(&parent, own);
        assert!(
            !resolved.contains_key("--a"),
            "cyclic, so invalid even though inherited"
        );
        assert!(!resolved.contains_key("--b"));
        assert!(!resolved.contains_key("--self"));
        assert!(resolved.contains_key("--keep"));
    }

    #[test]
    fn a_runaway_chain_is_cut_off() {
        let mut own = HashMap::new();
        for i in 0..100 {
            own.insert(format!("--p{i}"), tokenize(&format!("var(--p{})", i + 1)));
        }
        own.insert("--p100".to_string(), tokenize("1px"));
        let resolved = resolve(&CustomProperties::new(), own);
        assert!(resolved.contains_key("--p100"));
        assert!(
            !resolved.contains_key("--p0"),
            "deeper than the substitution guard"
        );
    }
}
