// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selector parsing, specificity, and DOM matching: type/class/ID/universal
//! selectors, the descendant, child, next-sibling and subsequent-sibling
//! combinators, attribute selectors with every operator, and the structural
//! pseudo-classes (`:root`, `:empty`, `:first-child`, `:nth-child(an+b)`, the
//! `-of-type` forms, `:not()`, `:is()`, `:where()`) plus the static form and
//! link states. Interaction pseudo-classes (`:hover`, `:focus`, ...) are valid
//! but never match a static document. Pseudo-elements and `:has()` are not
//! supported yet, so a selector that uses them is dropped.
//!
//! Specificity is the textbook (id, class-like, type) triple -- both
//! Stylo and Blink pack this into an integer for speed
//! (`research/css-cascade.md` §1/§2), but a plain tuple already gets
//! correct lexicographic ordering for free from `#[derive(Ord)]` with
//! no packing/unpacking code to get wrong.

use crate::tokenizer::Token;
use blueice_dom::{Document, NodeData, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `[a=v i]`: equality that ignores ASCII case (plain `=` is `AttrEquals`).
    Equals,
    /// `[a~=v]`: v is one of the whitespace-separated words.
    Includes,
    /// `[a|=v]`: exactly v, or v followed by `-`.
    DashMatch,
    /// `[a^=v]`
    Prefix,
    /// `[a$=v]`
    Suffix,
    /// `[a*=v]`
    Substring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoClass {
    Root,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    /// `:nth-child(an+b)`, `:nth-last-child`, `:nth-of-type`, `:nth-last-of-type`.
    Nth {
        a: i64,
        b: i64,
        from_end: bool,
        of_type: bool,
    },
    Not(Vec<ComplexSelector>),
    Is(Vec<ComplexSelector>),
    /// Like `:is()` but contributes no specificity.
    Where(Vec<ComplexSelector>),
    /// `:link` / `:any-link`: an `a`/`area` with an `href`.
    Link,
    Checked,
    Disabled,
    Enabled,
    Required,
    Optional,
    /// A valid state that no static document is ever in (`:hover`, `:focus`,
    /// `:active`, `:visited`, `:target`, ...).
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Universal,
    Type(String),
    Class(String),
    Id(String),
    AttrPresence(String),
    AttrEquals(String, String),
    /// An attribute selector with an operator other than plain `=`; the flag
    /// is true for the ASCII case-insensitive `i` modifier.
    Attr(String, AttrOp, String, bool),
    Pseudo(PseudoClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    /// `a + b`: b immediately follows a.
    NextSibling,
    /// `a ~ b`: b follows a, not necessarily immediately.
    SubsequentSibling,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compound {
    pub simple_selectors: Vec<SimpleSelector>,
}

/// A full selector, stored left-to-right as written (`div.foo > p` =>
/// compounds `[div.foo, p]`, combinators `[Child]`). `combinators.len()
/// == compounds.len() - 1`; `combinators[i]` connects `compounds[i]` to
/// `compounds[i + 1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    pub compounds: Vec<Compound>,
    pub combinators: Vec<Combinator>,
}

/// The most selective thing the rightmost compound requires, used to find the
/// rules that can possibly match an element without testing every rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectorKey<'a> {
    Id(&'a str),
    Class(&'a str),
    Type(&'a str),
    Universal,
}

impl ComplexSelector {
    pub fn key(&self) -> SelectorKey<'_> {
        let simple = self
            .compounds
            .last()
            .map(|c| c.simple_selectors.as_slice())
            .unwrap_or(&[]);
        let find = |wanted: fn(&SimpleSelector) -> Option<&str>| simple.iter().find_map(wanted);
        if let Some(id) = find(|s| match s {
            SimpleSelector::Id(id) => Some(id.as_str()),
            _ => None,
        }) {
            return SelectorKey::Id(id);
        }
        if let Some(class) = find(|s| match s {
            SimpleSelector::Class(class) => Some(class.as_str()),
            _ => None,
        }) {
            return SelectorKey::Class(class);
        }
        if let Some(tag) = find(|s| match s {
            SimpleSelector::Type(tag) => Some(tag.as_str()),
            _ => None,
        }) {
            return SelectorKey::Type(tag);
        }
        SelectorKey::Universal
    }
}

/// (id-count, class-like-count, type-count). `Ord`'s derived
/// lexicographic comparison is exactly CSS specificity comparison.
pub type Specificity = (u32, u32, u32);

fn add(a: Specificity, b: Specificity) -> Specificity {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

pub fn specificity(selector: &ComplexSelector) -> Specificity {
    let mut total = (0, 0, 0);
    for compound in &selector.compounds {
        for s in &compound.simple_selectors {
            total = add(
                total,
                match s {
                    SimpleSelector::Id(_) => (1, 0, 0),
                    SimpleSelector::Class(_)
                    | SimpleSelector::AttrPresence(_)
                    | SimpleSelector::AttrEquals(_, _)
                    | SimpleSelector::Attr(..) => (0, 1, 0),
                    SimpleSelector::Type(_) => (0, 0, 1),
                    SimpleSelector::Universal => (0, 0, 0),
                    SimpleSelector::Pseudo(PseudoClass::Where(_)) => (0, 0, 0),
                    SimpleSelector::Pseudo(PseudoClass::Not(list) | PseudoClass::Is(list)) => {
                        list.iter().map(specificity).max().unwrap_or((0, 0, 0))
                    }
                    SimpleSelector::Pseudo(_) => (0, 1, 0),
                },
            );
        }
    }
    total
}

fn trim_whitespace(tokens: &[Token]) -> &[Token] {
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

/// Splits on commas that are not inside parentheses or brackets.
fn split_commas(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let (mut start, mut depth) = (0, 0usize);
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen | Token::LeftBracket => depth += 1,
            Token::RightParen | Token::RightBracket => depth = depth.saturating_sub(1),
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

/// Parses a comma-separated selector list (the tokens before a rule's
/// `{`). A component that fails to parse (a feature not supported yet)
/// is dropped rather than failing the whole list: such a selector is far more
/// often valid-but-unsupported than invalid, and the sibling selectors of a
/// valid list must still apply.
pub fn parse_selector_list(tokens: &[Token]) -> Vec<ComplexSelector> {
    split_commas(tokens)
        .into_iter()
        .filter_map(parse_complex_selector)
        .collect()
}

fn parse_complex_selector(tokens: &[Token]) -> Option<ComplexSelector> {
    let tokens = trim_whitespace(tokens);
    if tokens.is_empty() {
        return None;
    }

    let mut raw_compounds: Vec<Vec<Token>> = vec![Vec::new()];
    let mut combinators: Vec<Combinator> = Vec::new();
    let mut pending_whitespace = false;
    let mut depth = 0usize;

    for tok in tokens {
        match tok {
            Token::LeftParen | Token::LeftBracket => {
                depth += 1;
                pending_whitespace = false;
                raw_compounds.last_mut()?.push(tok.clone());
            }
            Token::RightParen | Token::RightBracket => {
                depth = depth.saturating_sub(1);
                raw_compounds.last_mut()?.push(tok.clone());
            }
            _ if depth > 0 => raw_compounds.last_mut()?.push(tok.clone()),
            Token::Whitespace => pending_whitespace = true,
            Token::Delim(c @ ('>' | '+' | '~')) => {
                // A combinator needs a compound on its left.
                if raw_compounds.last()?.is_empty() {
                    return None;
                }
                combinators.push(match c {
                    '>' => Combinator::Child,
                    '+' => Combinator::NextSibling,
                    _ => Combinator::SubsequentSibling,
                });
                raw_compounds.push(Vec::new());
                pending_whitespace = false;
            }
            other => {
                if pending_whitespace && !raw_compounds.last()?.is_empty() {
                    combinators.push(Combinator::Descendant);
                    raw_compounds.push(Vec::new());
                }
                pending_whitespace = false;
                raw_compounds.last_mut()?.push(other.clone());
            }
        }
    }

    let compounds: Option<Vec<Compound>> = raw_compounds
        .iter()
        .map(|toks| {
            parse_simple_selectors(toks).map(|simple_selectors| Compound { simple_selectors })
        })
        .collect();
    let compounds = compounds?;

    Some(ComplexSelector {
        compounds,
        combinators,
    })
}

/// The index of the `)`/`]` matching the opener at `open`.
fn closer(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::LeftParen | Token::LeftBracket => depth += 1,
            Token::RightParen | Token::RightBracket => {
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

fn parse_attribute(inner: &[Token]) -> Option<SimpleSelector> {
    let inner: Vec<&Token> = inner.iter().filter(|t| **t != Token::Whitespace).collect();
    let Some(Token::Ident(name)) = inner.first() else {
        return None;
    };
    let name = name.to_ascii_lowercase();
    if inner.len() == 1 {
        return Some(SimpleSelector::AttrPresence(name));
    }
    let (op, rest) = match (inner.get(1)?, inner.get(2)) {
        (Token::Delim('='), _) => (None, &inner[2..]),
        (Token::Delim(c @ ('~' | '|' | '^' | '$' | '*')), Some(Token::Delim('='))) => (
            Some(match c {
                '~' => AttrOp::Includes,
                '|' => AttrOp::DashMatch,
                '^' => AttrOp::Prefix,
                '$' => AttrOp::Suffix,
                _ => AttrOp::Substring,
            }),
            &inner[3..],
        ),
        _ => return None,
    };
    let value = match rest.first()? {
        Token::QuotedString(s) | Token::Ident(s) => s.clone(),
        Token::Number(n) => format!("{n}"),
        _ => return None,
    };
    let insensitive = match rest.get(1) {
        None => false,
        Some(Token::Ident(flag)) if flag.eq_ignore_ascii_case("i") => true,
        Some(Token::Ident(flag)) if flag.eq_ignore_ascii_case("s") => false,
        _ => return None,
    };
    if rest.len() > 2 {
        return None;
    }
    Some(match op {
        None if !insensitive => SimpleSelector::AttrEquals(name, value),
        None => SimpleSelector::Attr(name, AttrOp::Equals, value, true),
        Some(op) => SimpleSelector::Attr(name, op, value, insensitive),
    })
}

/// `an+b` from the tokens of `:nth-*()`'s argument.
fn parse_nth(tokens: &[Token]) -> Option<(i64, i64)> {
    let mut text = String::new();
    let mut previous_sign = false;
    for token in tokens.iter().filter(|t| **t != Token::Whitespace) {
        match token {
            Token::Ident(word) => text.push_str(&word.to_ascii_lowercase()),
            Token::Dimension(n, unit) => {
                if n.fract() != 0.0 {
                    return None;
                }
                text.push_str(&format!("{}{}", *n as i64, unit));
            }
            Token::Number(n) => {
                if n.fract() != 0.0 {
                    return None;
                }
                let n = *n as i64;
                if previous_sign || text.is_empty() || n < 0 {
                    text.push_str(&n.to_string());
                } else {
                    text.push_str(&format!("+{n}"));
                }
            }
            Token::Delim(c @ ('+' | '-')) => {
                text.push(*c);
                previous_sign = true;
                continue;
            }
            _ => return None,
        }
        previous_sign = false;
    }
    match text.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some(n_at) = text.find('n') {
        let coefficient = &text[..n_at];
        let a = match coefficient {
            "" | "+" => 1,
            "-" => -1,
            other => other.parse().ok()?,
        };
        let rest = &text[n_at + 1..];
        let b = if rest.is_empty() {
            0
        } else {
            let (sign, digits) = match rest.strip_prefix('-') {
                Some(d) => (-1, d),
                None => (1, rest.strip_prefix('+')?),
            };
            sign * digits.parse::<i64>().ok()?
        };
        Some((a, b))
    } else {
        text.parse().ok().map(|b| (0, b))
    }
}

fn parse_pseudo(name: &str, args: Option<&[Token]>) -> Option<PseudoClass> {
    let name = name.to_ascii_lowercase();
    let list = |args: &[Token]| {
        let parsed = parse_selector_list(args);
        (!parsed.is_empty()).then_some(parsed)
    };
    Some(match (name.as_str(), args) {
        ("root", None) => PseudoClass::Root,
        ("empty", None) => PseudoClass::Empty,
        ("first-child", None) => PseudoClass::FirstChild,
        ("last-child", None) => PseudoClass::LastChild,
        ("only-child", None) => PseudoClass::OnlyChild,
        ("first-of-type", None) => PseudoClass::FirstOfType,
        ("last-of-type", None) => PseudoClass::LastOfType,
        ("only-of-type", None) => PseudoClass::OnlyOfType,
        ("link" | "any-link", None) => PseudoClass::Link,
        ("checked", None) => PseudoClass::Checked,
        ("disabled", None) => PseudoClass::Disabled,
        ("enabled", None) => PseudoClass::Enabled,
        ("required", None) => PseudoClass::Required,
        ("optional", None) => PseudoClass::Optional,
        (
            "hover" | "active" | "focus" | "focus-visible" | "focus-within" | "visited" | "target"
            | "target-within" | "autofill" | "-webkit-autofill" | "indeterminate" | "invalid"
            | "valid" | "in-range" | "out-of-range" | "user-invalid" | "user-valid" | "fullscreen"
            | "modal" | "popover-open" | "playing" | "paused" | "placeholder-shown" | "read-only"
            | "read-write" | "default" | "local-link" | "current" | "past" | "future",
            None,
        ) => PseudoClass::Never,
        ("not", Some(args)) => PseudoClass::Not(list(args)?),
        ("is" | "matches" | "-webkit-any" | "-moz-any", Some(args)) => PseudoClass::Is(list(args)?),
        ("where", Some(args)) => PseudoClass::Where(list(args)?),
        (
            kind @ ("nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type"),
            Some(args),
        ) => {
            // `of <selector>` is not supported; reject rather than ignore it.
            let (a, b) = parse_nth(args)?;
            PseudoClass::Nth {
                a,
                b,
                from_end: kind.contains("last"),
                of_type: kind.ends_with("of-type"),
            }
        }
        ("lang" | "dir", Some(_)) => PseudoClass::Never,
        _ => return None,
    })
}

fn parse_simple_selectors(tokens: &[Token]) -> Option<Vec<SimpleSelector>> {
    if tokens.is_empty() {
        return None;
    }
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Ident(name) => {
                result.push(SimpleSelector::Type(name.to_ascii_lowercase()));
                i += 1;
            }
            Token::Delim('*') => {
                result.push(SimpleSelector::Universal);
                i += 1;
            }
            Token::Delim('.') => {
                let Some(Token::Ident(name)) = tokens.get(i + 1) else {
                    return None;
                };
                result.push(SimpleSelector::Class(name.clone()));
                i += 2;
            }
            Token::Hash(name) => {
                result.push(SimpleSelector::Id(name.clone()));
                i += 1;
            }
            Token::LeftBracket => {
                let end = closer(tokens, i)?;
                result.push(parse_attribute(&tokens[i + 1..end])?);
                i = end + 1;
            }
            Token::Colon => {
                // `::` introduces a pseudo-element, which is not supported.
                let Some(Token::Ident(name)) = tokens.get(i + 1) else {
                    return None;
                };
                if tokens.get(i + 2) == Some(&Token::LeftParen) {
                    let end = closer(tokens, i + 2)?;
                    result.push(SimpleSelector::Pseudo(parse_pseudo(
                        name,
                        Some(&tokens[i + 3..end]),
                    )?));
                    i = end + 1;
                } else {
                    result.push(SimpleSelector::Pseudo(parse_pseudo(name, None)?));
                    i += 2;
                }
            }
            _ => return None,
        }
    }
    Some(result)
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn element_children(doc: &Document, parent: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    doc.children(parent)
        .filter(|c| matches!(doc.data(*c), NodeData::Element { .. }))
}

fn tag_of(doc: &Document, node: NodeId) -> Option<&str> {
    match doc.data(node) {
        NodeData::Element { tag_name, .. } => Some(tag_name),
        _ => None,
    }
}

/// 1-based position of `node` among its element siblings, counted from the
/// start or the end, optionally among siblings of the same type only.
fn position(doc: &Document, node: NodeId, from_end: bool, of_type: bool) -> Option<i64> {
    let parent = doc.parent(node)?;
    let tag = tag_of(doc, node)?;
    let siblings: Vec<NodeId> = element_children(doc, parent)
        .filter(|s| !of_type || tag_of(doc, *s) == Some(tag))
        .collect();
    let index = siblings.iter().position(|s| *s == node)?;
    Some(if from_end {
        (siblings.len() - index) as i64
    } else {
        index as i64 + 1
    })
}

fn nth_matches(a: i64, b: i64, position: i64) -> bool {
    if a == 0 {
        position == b
    } else {
        let n = position - b;
        n % a == 0 && n / a >= 0
    }
}

fn pseudo_matches(doc: &Document, node: NodeId, pseudo: &PseudoClass) -> bool {
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    else {
        return false;
    };
    let has = |name: &str| attribute(attributes, name).is_some();
    let form_control = matches!(
        tag_name.as_str(),
        "input" | "button" | "select" | "textarea" | "fieldset" | "optgroup" | "option"
    );
    match pseudo {
        PseudoClass::Root => doc.parent(node) == Some(doc.root()),
        PseudoClass::Empty => doc.first_child(node).is_none(),
        PseudoClass::FirstChild => position(doc, node, false, false) == Some(1),
        PseudoClass::LastChild => position(doc, node, true, false) == Some(1),
        PseudoClass::OnlyChild => {
            position(doc, node, false, false) == Some(1)
                && position(doc, node, true, false) == Some(1)
        }
        PseudoClass::FirstOfType => position(doc, node, false, true) == Some(1),
        PseudoClass::LastOfType => position(doc, node, true, true) == Some(1),
        PseudoClass::OnlyOfType => {
            position(doc, node, false, true) == Some(1)
                && position(doc, node, true, true) == Some(1)
        }
        PseudoClass::Nth {
            a,
            b,
            from_end,
            of_type,
        } => position(doc, node, *from_end, *of_type).is_some_and(|p| nth_matches(*a, *b, p)),
        PseudoClass::Not(list) => !list.iter().any(|s| matches(doc, node, s)),
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            list.iter().any(|s| matches(doc, node, s))
        }
        PseudoClass::Link => matches!(tag_name.as_str(), "a" | "area") && has("href"),
        PseudoClass::Checked => {
            (tag_name == "input"
                && matches!(attribute(attributes, "type"), Some("checkbox" | "radio"))
                && has("checked"))
                || (tag_name == "option" && has("selected"))
        }
        PseudoClass::Disabled => form_control && has("disabled"),
        PseudoClass::Enabled => form_control && !has("disabled"),
        PseudoClass::Required => {
            matches!(tag_name.as_str(), "input" | "select" | "textarea") && has("required")
        }
        PseudoClass::Optional => {
            matches!(tag_name.as_str(), "input" | "select" | "textarea") && !has("required")
        }
        PseudoClass::Never => false,
    }
}

fn attr_matches(op: AttrOp, actual: &str, wanted: &str, insensitive: bool) -> bool {
    let (actual, wanted) = if insensitive {
        (actual.to_ascii_lowercase(), wanted.to_ascii_lowercase())
    } else {
        (actual.to_string(), wanted.to_string())
    };
    match op {
        AttrOp::Equals => actual == wanted,
        AttrOp::Includes => {
            !wanted.is_empty() && actual.split_ascii_whitespace().any(|w| w == wanted)
        }
        AttrOp::DashMatch => actual == wanted || actual.starts_with(&format!("{wanted}-")),
        AttrOp::Prefix => !wanted.is_empty() && actual.starts_with(&wanted),
        AttrOp::Suffix => !wanted.is_empty() && actual.ends_with(&wanted),
        AttrOp::Substring => !wanted.is_empty() && actual.contains(&wanted),
    }
}

fn compound_matches(doc: &Document, node: NodeId, compound: &Compound) -> bool {
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    else {
        return false;
    };
    compound.simple_selectors.iter().all(|s| match s {
        SimpleSelector::Universal => true,
        SimpleSelector::Type(t) => tag_name == t,
        SimpleSelector::Class(c) => attributes
            .iter()
            .any(|(k, v)| k == "class" && v.split_whitespace().any(|cls| cls == c)),
        SimpleSelector::Id(id) => attributes.iter().any(|(k, v)| k == "id" && v == id),
        SimpleSelector::AttrPresence(a) => attributes.iter().any(|(k, _)| k == a),
        SimpleSelector::AttrEquals(a, v) => attributes.iter().any(|(k, val)| k == a && val == v),
        SimpleSelector::Attr(a, op, v, insensitive) => attributes
            .iter()
            .any(|(k, val)| k == a && attr_matches(*op, val, v, *insensitive)),
        SimpleSelector::Pseudo(pseudo) => pseudo_matches(doc, node, pseudo),
    })
}

/// Whether `node` matches `selector`, walking up `doc`'s tree for
/// descendant/child combinators and across siblings for `+` and `~`.
pub fn matches(doc: &Document, node: NodeId, selector: &ComplexSelector) -> bool {
    let n = selector.compounds.len();
    if n == 0 {
        return false;
    }
    if !compound_matches(doc, node, &selector.compounds[n - 1]) {
        return false;
    }
    matches_from(doc, selector, n - 1, node)
}

fn previous_element(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut sibling = doc.prev_sibling(node);
    while let Some(s) = sibling {
        if matches!(doc.data(s), NodeData::Element { .. }) {
            return Some(s);
        }
        sibling = doc.prev_sibling(s);
    }
    None
}

/// Checks that `selector.compounds[0..=i]` all match walking up from
/// `node` (which the caller has already confirmed matches `compounds[i]`),
/// backtracking through alternate ancestors for `Descendant` combinators
/// instead of committing to the first (nearest) one that happens to
/// match the immediate compound. A greedy nearest-match can satisfy one
/// `Descendant` step yet fail a later (further left, stricter) step even
/// when a farther ancestor would have satisfied the whole chain --
/// e.g. `x > y z` against `<x><y class=outer><div><y class=inner><z/>
/// </y></div></y></x>`: the nearest "y" ancestor of `z` (`y.inner`) isn't
/// a child of `x` (its parent is `div`), but the farther one (`y.outer`)
/// is, so the selector should still match. `~` backtracks the same way
/// across earlier siblings.
fn matches_from(doc: &Document, selector: &ComplexSelector, i: usize, node: NodeId) -> bool {
    if i == 0 {
        return true;
    }
    let wanted = &selector.compounds[i - 1];
    match selector.combinators[i - 1] {
        Combinator::Child => {
            let Some(parent) = doc.parent(node) else {
                return false;
            };
            compound_matches(doc, parent, wanted) && matches_from(doc, selector, i - 1, parent)
        }
        Combinator::Descendant => {
            let mut ancestor = doc.parent(node);
            while let Some(a) = ancestor {
                if compound_matches(doc, a, wanted) && matches_from(doc, selector, i - 1, a) {
                    return true;
                }
                ancestor = doc.parent(a);
            }
            false
        }
        Combinator::NextSibling => previous_element(doc, node).is_some_and(|previous| {
            compound_matches(doc, previous, wanted) && matches_from(doc, selector, i - 1, previous)
        }),
        Combinator::SubsequentSibling => {
            let mut sibling = previous_element(doc, node);
            while let Some(s) = sibling {
                if compound_matches(doc, s, wanted) && matches_from(doc, selector, i - 1, s) {
                    return true;
                }
                sibling = previous_element(doc, s);
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn selectors(css: &str) -> Vec<ComplexSelector> {
        parse_selector_list(&tokenize(css))
    }

    fn one(css: &str) -> ComplexSelector {
        let mut list = selectors(css);
        assert_eq!(list.len(), 1, "expected exactly one selector from {css:?}");
        list.remove(0)
    }

    #[test]
    fn type_class_id_and_universal() {
        assert_eq!(
            one("div").compounds,
            vec![Compound {
                simple_selectors: vec![SimpleSelector::Type("div".to_string())]
            }]
        );
        assert_eq!(
            one(".foo").compounds,
            vec![Compound {
                simple_selectors: vec![SimpleSelector::Class("foo".to_string())]
            }]
        );
        assert_eq!(
            one("#foo").compounds,
            vec![Compound {
                simple_selectors: vec![SimpleSelector::Id("foo".to_string())]
            }]
        );
        assert_eq!(
            one("*").compounds,
            vec![Compound {
                simple_selectors: vec![SimpleSelector::Universal]
            }]
        );
    }

    #[test]
    fn type_names_are_case_insensitive_class_and_id_are_not() {
        assert_eq!(
            one("DIV").compounds[0].simple_selectors,
            vec![SimpleSelector::Type("div".to_string())]
        );
        assert_eq!(
            one(".Foo").compounds[0].simple_selectors,
            vec![SimpleSelector::Class("Foo".to_string())]
        );
        assert_eq!(
            one("#Foo").compounds[0].simple_selectors,
            vec![SimpleSelector::Id("Foo".to_string())]
        );
    }

    #[test]
    fn compound_selector_combines_simple_selectors_with_no_combinator() {
        let sel = one("div.foo#bar");
        assert_eq!(sel.compounds.len(), 1);
        assert_eq!(
            sel.compounds[0].simple_selectors,
            vec![
                SimpleSelector::Type("div".to_string()),
                SimpleSelector::Class("foo".to_string()),
                SimpleSelector::Id("bar".to_string()),
            ]
        );
    }

    #[test]
    fn attribute_selectors() {
        assert_eq!(
            one("[disabled]").compounds[0].simple_selectors,
            vec![SimpleSelector::AttrPresence("disabled".to_string())]
        );
        assert_eq!(
            one(r#"[type="text"]"#).compounds[0].simple_selectors,
            vec![SimpleSelector::AttrEquals(
                "type".to_string(),
                "text".to_string()
            )]
        );
        assert_eq!(
            one("[type=text]").compounds[0].simple_selectors,
            vec![SimpleSelector::AttrEquals(
                "type".to_string(),
                "text".to_string()
            )],
            "unquoted attribute values are also valid"
        );
    }

    #[test]
    fn descendant_combinator() {
        let sel = one("div p");
        assert_eq!(sel.combinators, vec![Combinator::Descendant]);
        assert_eq!(sel.compounds.len(), 2);
    }

    #[test]
    fn child_combinator() {
        let sel = one("div > p");
        assert_eq!(sel.combinators, vec![Combinator::Child]);
    }

    #[test]
    fn child_combinator_without_surrounding_whitespace() {
        let sel = one("div>p");
        assert_eq!(sel.combinators, vec![Combinator::Child]);
        assert_eq!(sel.compounds.len(), 2);
    }

    #[test]
    fn mixed_combinators_multi_level() {
        let sel = one("ul > li.item a");
        assert_eq!(
            sel.combinators,
            vec![Combinator::Child, Combinator::Descendant]
        );
        assert_eq!(sel.compounds.len(), 3);
    }

    #[test]
    fn comma_separated_selector_list() {
        let list = selectors("h1, h2, .title");
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn unsupported_selector_in_a_list_is_dropped_not_fatal() {
        // A pseudo-element is not supported yet; the sibling `p`
        // selector must still parse.
        let list = selectors("p, a::before");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].compounds[0].simple_selectors,
            vec![SimpleSelector::Type("p".to_string())]
        );
    }

    #[test]
    fn specificity_ordering() {
        assert_eq!(specificity(&one("div")), (0, 0, 1));
        assert_eq!(specificity(&one(".foo")), (0, 1, 0));
        assert_eq!(specificity(&one("#foo")), (1, 0, 0));
        assert_eq!(specificity(&one("div.foo")), (0, 1, 1));
        assert_eq!(specificity(&one("div p")), (0, 0, 2));
        assert_eq!(specificity(&one("*")), (0, 0, 0));
        assert!(specificity(&one("#foo")) > specificity(&one("div.foo.bar.baz")));
        assert!(specificity(&one(".a.b")) > specificity(&one("div.a")));
    }

    // ---- DOM matching ----

    fn elem(doc: &mut Document, tag: &str, attrs: &[(&str, &str)]) -> NodeId {
        doc.create_node(NodeData::Element {
            tag_name: tag.to_string(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        })
    }

    #[test]
    fn matches_type_class_and_id() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = elem(&mut doc, "div", &[("class", "foo bar"), ("id", "x")]);
        doc.append_child(root, div);

        assert!(matches(&doc, div, &one("div")));
        assert!(matches(&doc, div, &one(".foo")));
        assert!(matches(&doc, div, &one(".bar")));
        assert!(matches(&doc, div, &one("#x")));
        assert!(!matches(&doc, div, &one("span")));
        assert!(!matches(&doc, div, &one(".baz")));
        assert!(matches(&doc, div, &one("*")));
    }

    #[test]
    fn matches_attribute_selectors() {
        let mut doc = Document::new();
        let root = doc.root();
        let input = elem(&mut doc, "input", &[("type", "text"), ("disabled", "")]);
        doc.append_child(root, input);

        assert!(matches(&doc, input, &one("[disabled]")));
        assert!(matches(&doc, input, &one(r#"[type="text"]"#)));
        assert!(!matches(&doc, input, &one(r#"[type="checkbox"]"#)));
        assert!(!matches(&doc, input, &one("[checked]")));
    }

    #[test]
    fn matches_descendant_combinator_at_any_depth() {
        let mut doc = Document::new();
        let root = doc.root();
        let ul = elem(&mut doc, "ul", &[]);
        let li = elem(&mut doc, "li", &[]);
        let a = elem(&mut doc, "a", &[]);
        doc.append_child(root, ul);
        doc.append_child(ul, li);
        doc.append_child(li, a);

        assert!(
            matches(&doc, a, &one("ul a")),
            "descendant, not just direct child"
        );
        assert!(matches(&doc, li, &one("ul li")));
        assert!(
            !matches(&doc, a, &one("ul > a")),
            "a is not a direct child of ul"
        );
    }

    #[test]
    fn matches_child_combinator_only_direct_parent() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = elem(&mut doc, "div", &[]);
        let p = elem(&mut doc, "p", &[]);
        doc.append_child(root, div);
        doc.append_child(div, p);

        assert!(matches(&doc, p, &one("div > p")));
        assert!(matches(&doc, p, &one("div p")));
    }

    #[test]
    fn descendant_then_child_backtracks_past_a_nearer_non_matching_ancestor() {
        // `x > y z`: the *nearest* "y" ancestor of z (y.inner) isn't a
        // child of x (its parent is div), but a farther "y" ancestor
        // (y.outer) is -- the matcher must not give up after the first
        // (nearest) ancestor satisfying the descendant step fails the
        // stricter child step that follows it.
        let mut doc = Document::new();
        let root = doc.root();
        let x = elem(&mut doc, "x", &[]);
        let y_outer = elem(&mut doc, "y", &[("class", "outer")]);
        let div = elem(&mut doc, "div", &[]);
        let y_inner = elem(&mut doc, "y", &[("class", "inner")]);
        let z = elem(&mut doc, "z", &[]);
        doc.append_child(root, x);
        doc.append_child(x, y_outer);
        doc.append_child(y_outer, div);
        doc.append_child(div, y_inner);
        doc.append_child(y_inner, z);

        assert!(
            matches(&doc, z, &one("x > y z")),
            "must backtrack to y.outer, whose parent is x"
        );
    }

    #[test]
    fn no_match_when_ancestor_chain_does_not_satisfy_selector() {
        let mut doc = Document::new();
        let root = doc.root();
        let section = elem(&mut doc, "section", &[]);
        let p = elem(&mut doc, "p", &[]);
        doc.append_child(root, section);
        doc.append_child(section, p);

        assert!(
            !matches(&doc, p, &one("div p")),
            "p's ancestor is section, not div"
        );
    }

    #[test]
    fn text_node_never_matches_any_selector() {
        let mut doc = Document::new();
        let root = doc.root();
        let text = doc.create_node(NodeData::Text {
            data: "hi".to_string(),
        });
        doc.append_child(root, text);
        assert!(!matches(&doc, text, &one("*")));
    }

    // ---- extended selectors ----

    fn tree() -> (Document, Vec<NodeId>) {
        // <div id=root class="a b">
        //   <p>1</p><span class=x>2</span><p class=y data-k="Hello-World">3</p><em></em>
        // </div>
        let doc = blueice_html::parse(
            "<div id=root class=\"a b\"><p>1</p><span class=x>2</span>\
             <p class=y data-k=\"Hello-World\">3</p><em></em></div>",
        );
        let div = find(&doc, doc.root(), "div").unwrap();
        let kids: Vec<NodeId> = doc.children(div).collect();
        (doc, kids)
    }

    fn find(doc: &Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    fn hits(doc: &Document, kids: &[NodeId], css: &str) -> Vec<usize> {
        let list = selectors(css);
        assert!(!list.is_empty(), "{css:?} should parse");
        kids.iter()
            .enumerate()
            .filter(|(_, k)| list.iter().any(|s| matches(doc, **k, s)))
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn sibling_combinators() {
        let (doc, kids) = tree();
        assert_eq!(hits(&doc, &kids, "p + span"), vec![1]);
        assert_eq!(
            hits(&doc, &kids, "p + p"),
            Vec::<usize>::new(),
            "span sits between"
        );
        assert_eq!(hits(&doc, &kids, "p ~ p"), vec![2]);
        assert_eq!(hits(&doc, &kids, "span ~ em"), vec![3]);
        assert_eq!(hits(&doc, &kids, "p + span + p"), vec![2]);
        assert_eq!(hits(&doc, &kids, "span+p"), vec![2], "no spaces needed");
        assert!(selectors("+ p").is_empty() && selectors("p +").is_empty());
    }

    #[test]
    fn structural_pseudo_classes() {
        let (doc, kids) = tree();
        assert_eq!(hits(&doc, &kids, ":first-child"), vec![0]);
        assert_eq!(hits(&doc, &kids, ":last-child"), vec![3]);
        assert_eq!(hits(&doc, &kids, "p:first-of-type"), vec![0]);
        assert_eq!(hits(&doc, &kids, "p:last-of-type"), vec![2]);
        assert_eq!(hits(&doc, &kids, "span:only-of-type"), vec![1]);
        assert_eq!(hits(&doc, &kids, ":empty"), vec![3]);
        assert_eq!(hits(&doc, &kids, ":nth-child(2)"), vec![1]);
        assert_eq!(hits(&doc, &kids, ":nth-child(odd)"), vec![0, 2]);
        assert_eq!(hits(&doc, &kids, ":nth-child(even)"), vec![1, 3]);
        assert_eq!(hits(&doc, &kids, ":nth-child(n+3)"), vec![2, 3]);
        assert_eq!(hits(&doc, &kids, ":nth-child(-n+2)"), vec![0, 1]);
        assert_eq!(hits(&doc, &kids, ":nth-child(2n-1)"), vec![0, 2]);
        assert_eq!(hits(&doc, &kids, ":nth-child( 2n + 1 )"), vec![0, 2]);
        assert_eq!(hits(&doc, &kids, ":nth-last-child(1)"), vec![3]);
        assert_eq!(hits(&doc, &kids, "p:nth-of-type(2)"), vec![2]);
        assert_eq!(hits(&doc, &kids, "p:nth-last-of-type(2)"), vec![0]);
        assert!(selectors(":nth-child(foo)").is_empty());
    }

    #[test]
    fn root_matches_only_the_document_element() {
        let doc = blueice_html::parse("<p>x</p>");
        let html = find(&doc, doc.root(), "html").unwrap();
        let p = find(&doc, doc.root(), "p").unwrap();
        let root = one(":root");
        assert!(matches(&doc, html, &root));
        assert!(!matches(&doc, p, &root));
    }

    #[test]
    fn not_is_where_take_selector_lists() {
        let (doc, kids) = tree();
        assert_eq!(hits(&doc, &kids, ":not(p)"), vec![1, 3]);
        assert_eq!(hits(&doc, &kids, ":not(p, span)"), vec![3]);
        assert_eq!(hits(&doc, &kids, ":is(span, em)"), vec![1, 3]);
        assert_eq!(hits(&doc, &kids, ":where(.x, .y)"), vec![1, 2]);
        assert_eq!(hits(&doc, &kids, "div > :is(p, span):not(.x)"), vec![0, 2]);
        assert_eq!(hits(&doc, &kids, ":is(div p, em)"), vec![0, 2, 3]);
        assert!(selectors(":not()").is_empty());
    }

    #[test]
    fn specificity_of_functional_pseudo_classes() {
        assert_eq!(specificity(&one(":where(#a, .b)")), (0, 0, 0));
        assert_eq!(
            specificity(&one(":is(#a, .b)")),
            (1, 0, 0),
            "most specific argument"
        );
        assert_eq!(specificity(&one(":not(.a)")), (0, 1, 0));
        assert_eq!(specificity(&one("p:first-child")), (0, 1, 1));
        assert_eq!(specificity(&one(":nth-child(2n+1)")), (0, 1, 0));
        assert_eq!(specificity(&one("a:hover")), (0, 1, 1));
        assert_eq!(specificity(&one("[href^=http]")), (0, 1, 0));
    }

    #[test]
    fn attribute_operators() {
        let (doc, kids) = tree();
        assert_eq!(hits(&doc, &kids, "[data-k=\"Hello-World\"]"), vec![2]);
        assert_eq!(hits(&doc, &kids, "[data-k=\"hello-world\" i]"), vec![2]);
        assert_eq!(
            hits(&doc, &kids, "[data-k=\"hello-world\"]"),
            Vec::<usize>::new()
        );
        assert_eq!(hits(&doc, &kids, "[data-k^=Hello]"), vec![2]);
        assert_eq!(hits(&doc, &kids, "[data-k$=World]"), vec![2]);
        assert_eq!(hits(&doc, &kids, "[data-k*=lo-Wo]"), vec![2]);
        assert_eq!(hits(&doc, &kids, "[data-k|=Hello]"), vec![2]);
        assert_eq!(hits(&doc, &kids, "[class~=x]"), vec![1]);
        assert_eq!(hits(&doc, &kids, "[class~=\"\"]"), Vec::<usize>::new());
        assert_eq!(hits(&doc, &kids, "[data-k^=\"\"]"), Vec::<usize>::new());
        assert!(selectors("[a=]").is_empty() && selectors("[a ?= b]").is_empty());
    }

    #[test]
    fn interaction_states_parse_but_never_match_and_pseudo_elements_are_dropped() {
        let (doc, kids) = tree();
        assert_eq!(hits(&doc, &kids, "p:hover"), Vec::<usize>::new());
        assert_eq!(hits(&doc, &kids, ":focus-visible"), Vec::<usize>::new());
        assert!(selectors("p::before").is_empty());
        assert!(selectors("p:before-ish").is_empty(), "unknown pseudo-class");
        assert!(
            selectors("p:has(a)").is_empty(),
            ":has is not supported yet"
        );
    }

    #[test]
    fn form_and_link_states_follow_attributes() {
        let doc = blueice_html::parse(
            "<a href=x>l</a><a>n</a><input type=checkbox checked><input disabled required>",
        );
        let all: Vec<NodeId> = {
            fn walk(doc: &Document, n: NodeId, out: &mut Vec<NodeId>) {
                if matches!(doc.data(n), NodeData::Element { .. }) {
                    out.push(n);
                }
                for c in doc.children(n) {
                    walk(doc, c, out);
                }
            }
            let mut v = Vec::new();
            walk(&doc, doc.root(), &mut v);
            v
        };
        let count = |css: &str| {
            let s = one(css);
            all.iter().filter(|n| matches(&doc, **n, &s)).count()
        };
        assert_eq!(count(":link"), 1);
        assert_eq!(count("a:any-link"), 1);
        assert_eq!(count(":checked"), 1);
        assert_eq!(count(":disabled"), 1);
        assert_eq!(count("input:enabled"), 1);
        assert_eq!(count(":required"), 1);
        assert_eq!(count("input:optional"), 1);
    }

    #[test]
    fn escaped_class_names_match_their_literal_text() {
        let doc = blueice_html::parse("<p class=\"md:flex w-1/2 a.b\">x</p>");
        let p = find(&doc, doc.root(), "p").unwrap();
        for css in [r".md\:flex", r".w-1\/2", r".a\.b", r"p.md\:flex.w-1\/2"] {
            assert!(matches(&doc, p, &one(css)), "{css}");
        }
    }

    #[test]
    fn selector_keys_prefer_id_then_class_then_type() {
        assert_eq!(one("div p#x.y").key(), SelectorKey::Id("x"));
        assert_eq!(one("div p.y.z").key(), SelectorKey::Class("y"));
        assert_eq!(one("div > p").key(), SelectorKey::Type("p"));
        assert_eq!(one("div > *").key(), SelectorKey::Universal);
        assert_eq!(one(":is(a, b)").key(), SelectorKey::Universal);
        assert_eq!(one("[x]").key(), SelectorKey::Universal);
    }
}
