// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selector parsing, specificity, and DOM matching, per
//! `phase-2-mvp-scope/PLAN.md`'s "MVP CSS scope": type/class/ID/
//! universal simple selectors, descendant and child combinators, and
//! attribute presence/equality selectors.
//!
//! Specificity is the textbook (id, class-like, type) triple -- both
//! Stylo and Blink pack this into an integer for speed
//! (`research/css-cascade.md` §1/§2), but a plain tuple already gets
//! correct lexicographic ordering for free from `#[derive(Ord)]` with
//! no packing/unpacking code to get wrong, and MVP has no performance
//! reason yet to trade that clarity away.

use crate::tokenizer::Token;
use blueice_dom::{Document, NodeData, NodeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Universal,
    Type(String),
    Class(String),
    Id(String),
    AttrPresence(String),
    AttrEquals(String, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
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

/// (id-count, class-like-count, type-count). `Ord`'s derived
/// lexicographic comparison is exactly CSS specificity comparison.
pub type Specificity = (u32, u32, u32);

pub fn specificity(selector: &ComplexSelector) -> Specificity {
    let mut id = 0u32;
    let mut class_like = 0u32;
    let mut ty = 0u32;
    for compound in &selector.compounds {
        for s in &compound.simple_selectors {
            match s {
                SimpleSelector::Id(_) => id += 1,
                SimpleSelector::Class(_) | SimpleSelector::AttrPresence(_) | SimpleSelector::AttrEquals(_, _) => {
                    class_like += 1
                }
                SimpleSelector::Type(_) => ty += 1,
                SimpleSelector::Universal => {}
            }
        }
    }
    (id, class_like, ty)
}

fn trim_whitespace(tokens: &[Token]) -> &[Token] {
    let start = tokens.iter().position(|t| *t != Token::Whitespace).unwrap_or(tokens.len());
    let end = tokens.iter().rposition(|t| *t != Token::Whitespace).map(|i| i + 1).unwrap_or(0);
    if start >= end {
        &[]
    } else {
        &tokens[start..end]
    }
}

/// Parses a comma-separated selector list (the tokens before a rule's
/// `{`). A component that fails to parse (an unsupported selector
/// feature, per the MVP cut list) is dropped rather than failing the
/// whole list -- matching real engines silently ignoring an invalid
/// selector in a list rather than rejecting sibling ones.
pub fn parse_selector_list(tokens: &[Token]) -> Vec<ComplexSelector> {
    tokens.split(|t| *t == Token::Comma).filter_map(parse_complex_selector).collect()
}

fn parse_complex_selector(tokens: &[Token]) -> Option<ComplexSelector> {
    let tokens = trim_whitespace(tokens);
    if tokens.is_empty() {
        return None;
    }

    let mut raw_compounds: Vec<Vec<Token>> = vec![Vec::new()];
    let mut combinators: Vec<Combinator> = Vec::new();
    let mut pending_whitespace = false;

    for tok in tokens {
        match tok {
            Token::Whitespace => pending_whitespace = true,
            Token::Delim('>') => {
                combinators.push(Combinator::Child);
                raw_compounds.push(Vec::new());
                pending_whitespace = false;
            }
            other => {
                if pending_whitespace && !raw_compounds.last().unwrap().is_empty() {
                    combinators.push(Combinator::Descendant);
                    raw_compounds.push(Vec::new());
                }
                pending_whitespace = false;
                raw_compounds.last_mut().unwrap().push(other.clone());
            }
        }
    }

    let compounds: Option<Vec<Compound>> = raw_compounds
        .iter()
        .map(|toks| parse_simple_selectors(toks).map(|simple_selectors| Compound { simple_selectors }))
        .collect();
    let compounds = compounds?;

    Some(ComplexSelector { compounds, combinators })
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
                let Some(Token::Ident(name)) = tokens.get(i + 1) else { return None };
                result.push(SimpleSelector::Class(name.clone()));
                i += 2;
            }
            Token::Hash(name) => {
                result.push(SimpleSelector::Id(name.clone()));
                i += 1;
            }
            Token::LeftBracket => {
                let Some(Token::Ident(attr)) = tokens.get(i + 1) else { return None };
                let attr = attr.to_ascii_lowercase();
                match tokens.get(i + 2) {
                    Some(Token::RightBracket) => {
                        result.push(SimpleSelector::AttrPresence(attr));
                        i += 3;
                    }
                    Some(Token::Delim('=')) => {
                        let value = match tokens.get(i + 3) {
                            Some(Token::QuotedString(s)) => s.clone(),
                            Some(Token::Ident(s)) => s.clone(),
                            _ => return None,
                        };
                        if tokens.get(i + 4) != Some(&Token::RightBracket) {
                            return None;
                        }
                        result.push(SimpleSelector::AttrEquals(attr, value));
                        i += 5;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    Some(result)
}

fn compound_matches(doc: &Document, node: NodeId, compound: &Compound) -> bool {
    let NodeData::Element { tag_name, attributes } = doc.data(node) else {
        return false;
    };
    compound.simple_selectors.iter().all(|s| match s {
        SimpleSelector::Universal => true,
        SimpleSelector::Type(t) => tag_name == t,
        SimpleSelector::Class(c) => attributes.iter().any(|(k, v)| k == "class" && v.split_whitespace().any(|cls| cls == c)),
        SimpleSelector::Id(id) => attributes.iter().any(|(k, v)| k == "id" && v == id),
        SimpleSelector::AttrPresence(a) => attributes.iter().any(|(k, _)| k == a),
        SimpleSelector::AttrEquals(a, v) => attributes.iter().any(|(k, val)| k == a && val == v),
    })
}

/// Whether `node` matches `selector`, walking up `doc`'s tree for
/// descendant/child combinators.
pub fn matches(doc: &Document, node: NodeId, selector: &ComplexSelector) -> bool {
    let n = selector.compounds.len();
    if n == 0 {
        return false;
    }
    if !compound_matches(doc, node, &selector.compounds[n - 1]) {
        return false;
    }

    let mut current = node;
    for i in (0..n - 1).rev() {
        match selector.combinators[i] {
            Combinator::Child => {
                let Some(parent) = doc.parent(current) else { return false };
                if !compound_matches(doc, parent, &selector.compounds[i]) {
                    return false;
                }
                current = parent;
            }
            Combinator::Descendant => {
                let mut ancestor = doc.parent(current);
                let found = loop {
                    match ancestor {
                        None => break None,
                        Some(a) if compound_matches(doc, a, &selector.compounds[i]) => break Some(a),
                        Some(a) => ancestor = doc.parent(a),
                    }
                };
                let Some(a) = found else { return false };
                current = a;
            }
        }
    }
    true
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
        assert_eq!(one("div").compounds, vec![Compound { simple_selectors: vec![SimpleSelector::Type("div".to_string())] }]);
        assert_eq!(one(".foo").compounds, vec![Compound { simple_selectors: vec![SimpleSelector::Class("foo".to_string())] }]);
        assert_eq!(one("#foo").compounds, vec![Compound { simple_selectors: vec![SimpleSelector::Id("foo".to_string())] }]);
        assert_eq!(one("*").compounds, vec![Compound { simple_selectors: vec![SimpleSelector::Universal] }]);
    }

    #[test]
    fn type_names_are_case_insensitive_class_and_id_are_not() {
        assert_eq!(one("DIV").compounds[0].simple_selectors, vec![SimpleSelector::Type("div".to_string())]);
        assert_eq!(one(".Foo").compounds[0].simple_selectors, vec![SimpleSelector::Class("Foo".to_string())]);
        assert_eq!(one("#Foo").compounds[0].simple_selectors, vec![SimpleSelector::Id("Foo".to_string())]);
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
            vec![SimpleSelector::AttrEquals("type".to_string(), "text".to_string())]
        );
        assert_eq!(
            one("[type=text]").compounds[0].simple_selectors,
            vec![SimpleSelector::AttrEquals("type".to_string(), "text".to_string())],
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
        assert_eq!(sel.combinators, vec![Combinator::Child, Combinator::Descendant]);
        assert_eq!(sel.compounds.len(), 3);
    }

    #[test]
    fn comma_separated_selector_list() {
        let list = selectors("h1, h2, .title");
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn unsupported_selector_in_a_list_is_dropped_not_fatal() {
        // :hover is not in the MVP selector scope; the sibling `p`
        // selector must still parse.
        let list = selectors("p, a:hover");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].compounds[0].simple_selectors, vec![SimpleSelector::Type("p".to_string())]);
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
            attributes: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
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

        assert!(matches(&doc, a, &one("ul a")), "descendant, not just direct child");
        assert!(matches(&doc, li, &one("ul li")));
        assert!(!matches(&doc, a, &one("ul > a")), "a is not a direct child of ul");
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
    fn no_match_when_ancestor_chain_does_not_satisfy_selector() {
        let mut doc = Document::new();
        let root = doc.root();
        let section = elem(&mut doc, "section", &[]);
        let p = elem(&mut doc, "p", &[]);
        doc.append_child(root, section);
        doc.append_child(section, p);

        assert!(!matches(&doc, p, &one("div p")), "p's ancestor is section, not div");
    }

    #[test]
    fn text_node_never_matches_any_selector() {
        let mut doc = Document::new();
        let root = doc.root();
        let text = doc.create_node(NodeData::Text { data: "hi".to_string() });
        doc.append_child(root, text);
        assert!(!matches(&doc, text, &one("*")));
    }
}
