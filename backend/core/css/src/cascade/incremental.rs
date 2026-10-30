// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Restyling only what a set of document changes can have affected.
//!
//! A full cascade recomputes every element; on a large page that is hundreds of
//! milliseconds, and a script that animates one element pays it per frame. Given the
//! document's own record of what changed ([`blueice_dom::Changes`]) and the styles from
//! the cascade before it, this recomputes just the elements whose style can differ:
//!
//! - an element whose only change is its `style` attribute, and then its descendants
//!   only if its computed style really changed (they inherit from it);
//! - an element with any other attribute changed, or one that was inserted: its whole
//!   subtree, because descendant combinators look at ancestors' attributes;
//! - the siblings a structural selector (`:first-child`, `:nth-child`, `+`, `~`,
//!   `:empty`) can see an insertion, removal or class change through -- only if the
//!   rules actually use such selectors, and only the adjacent ones when that is enough.
//!
//! Anything it cannot reason about (a journal that overflowed, a root font-size change
//! that every `rem` depends on) makes it report `false` and leave the styles untouched,
//! so the caller does a full cascade. The result is checked against a full cascade by a
//! differential test over random trees and edits.

use super::*;
use crate::selector::{Combinator, ComplexSelector, PseudoClass, SimpleSelector};
use blueice_dom::{Before, Changes};
use std::collections::HashSet;

/// How the rules look at a node's place among its siblings.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct StructureUse {
    /// `+` or `~`.
    sibling: bool,
    /// `:nth-*`, `:*-of-type`: any sibling's position can depend on any other's.
    positional_all: bool,
    /// `:first-child`, `:last-child`, `:only-child`: only the neighbours matter.
    positional_adjacent: bool,
    /// `:empty`: the parent's own style depends on whether it has children.
    empty: bool,
}

impl StructureUse {
    pub(super) fn scan(&mut self, selector: &ComplexSelector) {
        if selector
            .combinators
            .iter()
            .any(|c| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling))
        {
            self.sibling = true;
        }
        for compound in &selector.compounds {
            for simple in &compound.simple_selectors {
                let SimpleSelector::Pseudo(pseudo) = simple else {
                    continue;
                };
                match pseudo {
                    PseudoClass::Nth { .. }
                    | PseudoClass::FirstOfType
                    | PseudoClass::LastOfType
                    | PseudoClass::OnlyOfType => self.positional_all = true,
                    PseudoClass::FirstChild | PseudoClass::LastChild | PseudoClass::OnlyChild => {
                        self.positional_adjacent = true
                    }
                    PseudoClass::Empty => self.empty = true,
                    PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
                        for inner in list {
                            self.scan(inner);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn in_tree(doc: &Document, mut node: NodeId) -> bool {
    if !doc.contains(node) {
        return false;
    }
    while let Some(parent) = doc.parent(node) {
        node = parent;
    }
    node == doc.root()
}

fn depth(doc: &Document, mut node: NodeId) -> usize {
    let mut d = 0;
    while let Some(parent) = doc.parent(node) {
        node = parent;
        d += 1;
    }
    d
}

fn is_element(doc: &Document, node: NodeId) -> bool {
    matches!(doc.data(node), NodeData::Element { .. })
}

/// The nearest element at or after `from` going `forward` (else backward) among siblings.
fn element_from(doc: &Document, from: Option<NodeId>, forward: bool) -> Option<NodeId> {
    let mut at = from;
    while let Some(n) = at {
        if is_element(doc, n) {
            return Some(n);
        }
        at = if forward {
            doc.next_sibling(n)
        } else {
            doc.prev_sibling(n)
        };
    }
    None
}

/// The styles an element and its `::before`/`::after` get, against `parent_style`.
fn compute_with_pseudo(
    doc: &Document,
    node: NodeId,
    index: &RuleIndex<'_>,
    environment: &Environment,
    parent_style: Option<&ComputedStyle>,
    root_font_size: f64,
) -> (ComputedStyle, [Option<ComputedStyle>; 2]) {
    let style = compute_style_for(
        doc,
        node,
        index,
        environment,
        parent_style,
        root_font_size,
        None,
    )
    .expect("an element always has a style");
    let pseudo = [PseudoElement::Before, PseudoElement::After].map(|which| {
        compute_style_for(
            doc,
            node,
            index,
            environment,
            Some(&style),
            root_font_size,
            Some(which),
        )
        .filter(|p| matches!(p.other.get("content"), Some(Value::Unparsed(_))))
    });
    (style, pseudo)
}

struct Restyle<'a> {
    doc: &'a Document,
    index: &'a RuleIndex<'a>,
    environment: &'a Environment,
    styles: &'a HashMap<NodeId, ComputedStyle>,
    /// New styles; `None` removes the entry.
    overlay: HashMap<NodeId, Option<ComputedStyle>>,
    visited: HashSet<NodeId>,
    forced: HashSet<NodeId>,
    /// A change the incremental path will not take on (the root's font size).
    bail: bool,
}

impl Restyle<'_> {
    fn style(&self, node: NodeId) -> Option<&ComputedStyle> {
        match self.overlay.get(&node) {
            Some(entry) => entry.as_ref(),
            None => self.styles.get(&node),
        }
    }

    /// The font size `rem` resolves against for `node`: its top-level ancestor's.
    fn root_font_size(&self, node: NodeId) -> f64 {
        let mut top = node;
        while let Some(parent) = self.doc.parent(top) {
            if parent == self.doc.root() {
                break;
            }
            top = parent;
        }
        if top == node {
            16.0
        } else {
            self.style(top).map_or(16.0, |s| s.font_size_px)
        }
    }

    fn visit(&mut self, node: NodeId, force: bool) {
        if self.bail
            || self.forced.contains(&node)
            || (!force && self.visited.contains(&node))
            || !is_element(self.doc, node)
        {
            return;
        }
        self.visited.insert(node);
        if force {
            self.forced.insert(node);
        }
        let parent = self.doc.parent(node);
        let parent_style = match parent {
            Some(p) if p != self.doc.root() => self.style(p).cloned(),
            _ => None,
        };
        let (style, pseudo) = compute_with_pseudo(
            self.doc,
            node,
            self.index,
            self.environment,
            parent_style.as_ref(),
            self.root_font_size(node),
        );
        let mut changed = self.style(node) != Some(&style);
        for (which, new) in [PseudoElement::Before, PseudoElement::After]
            .into_iter()
            .zip(pseudo)
        {
            let id = pseudo_node_id(node, which);
            if self.style(id) != new.as_ref() {
                changed = true;
                self.overlay.insert(id, new);
            }
        }
        if parent == Some(self.doc.root())
            && self.styles.get(&node).map(|s| s.font_size_px) != Some(style.font_size_px)
        {
            self.bail = true;
            return;
        }
        if changed {
            self.overlay.insert(node, Some(style));
        }
        if force || changed {
            let children: Vec<NodeId> = self.doc.children(node).collect();
            for child in children {
                self.visit(child, force);
            }
        }
    }
}

/// Brings `styles` up to date with `changes` without a full cascade. Returns `false`
/// (leaving `styles` as it was) when a full cascade is needed instead. The rules and
/// the viewport must be the ones `styles` was computed with.
pub fn cascade_incremental(
    doc: &Document,
    stylesheets: &[(Origin, &[Rule])],
    viewport_width: f64,
    viewport_height: f64,
    styles: &mut HashMap<NodeId, ComputedStyle>,
    changes: &Changes,
) -> bool {
    if changes.overflow {
        return false;
    }
    if changes.is_empty() {
        return true;
    }
    let index = RuleIndex::build(stylesheets);
    let uses = index.structure;
    let environment = Environment {
        viewport_width,
        viewport_height,
    };

    // (node, force the whole subtree)
    let mut roots: Vec<(NodeId, bool)> = Vec::new();

    // The siblings of `around` whose style can depend on it being there or not.
    let neighbours =
        |parent: NodeId, near: [Option<NodeId>; 2], roots: &mut Vec<(NodeId, bool)>| {
            if uses.positional_all || uses.sibling {
                for child in doc.children(parent) {
                    if is_element(doc, child) {
                        roots.push((child, true));
                    }
                }
            } else if uses.positional_adjacent {
                for (sibling, forward) in [(near[0], false), (near[1], true)] {
                    if let Some(e) = element_from(doc, sibling, forward) {
                        if doc.parent(e) == Some(parent) {
                            roots.push((e, true));
                            continue;
                        }
                    }
                    // A neighbour that moved on: any child might be next to the gap.
                    if sibling.is_some_and(|s| doc.parent(s) != Some(parent)) {
                        for child in doc.children(parent) {
                            if is_element(doc, child) {
                                roots.push((child, true));
                            }
                        }
                    }
                }
            }
            if uses.empty && is_element(doc, parent) {
                roots.push((parent, false));
            }
        };

    for (&id, before) in &changes.touched {
        if !in_tree(doc, id) {
            continue;
        }
        let (
            Before::Element {
                tag_name: old_tag,
                attributes: old,
            },
            NodeData::Element {
                tag_name,
                attributes: new,
            },
        ) = (before, doc.data(id))
        else {
            continue;
        };
        let differs = |name: &str| {
            let find = |list: &Vec<(String, String)>| {
                list.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
            };
            find(old) != find(new)
        };
        let mut names: Vec<&str> = old
            .iter()
            .chain(new.iter())
            .map(|(k, _)| k.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        let changed: Vec<&str> = names.into_iter().filter(|n| differs(n)).collect();
        if old_tag == tag_name && changed.is_empty() {
            continue;
        }
        if old_tag == tag_name && changed == ["style"] {
            roots.push((id, false));
            continue;
        }
        roots.push((id, true));
        if uses.sibling {
            let mut next = element_from(doc, doc.next_sibling(id), true);
            while let Some(sibling) = next {
                roots.push((sibling, true));
                next = element_from(doc, doc.next_sibling(sibling), true);
            }
        }
    }
    for &(parent, child) in &changes.attached {
        if doc.contains(child) && doc.parent(child) == Some(parent) && in_tree(doc, parent) {
            roots.push((child, true));
            neighbours(
                parent,
                [doc.prev_sibling(child), doc.next_sibling(child)],
                &mut roots,
            );
        }
    }
    let mut pruned = false;
    for d in &changes.detached {
        pruned = true;
        if in_tree(doc, d.parent) {
            neighbours(d.parent, [d.prev, d.next], &mut roots);
        }
    }

    roots.sort_by_key(|(node, _)| depth(doc, *node));
    let mut restyle = Restyle {
        doc,
        index: &index,
        environment: &environment,
        styles,
        overlay: HashMap::new(),
        visited: HashSet::new(),
        forced: HashSet::new(),
        bail: false,
    };
    for (node, force) in roots {
        restyle.visit(node, force);
    }
    if restyle.bail {
        return false;
    }
    let overlay = restyle.overlay;
    for (id, style) in overlay {
        match style {
            Some(style) => {
                styles.insert(id, style);
            }
            None => {
                styles.remove(&id);
            }
        }
    }
    if pruned {
        // Nodes that left the tree (or were freed) take their styles with them.
        let mut reachable = HashSet::new();
        let mut stack = vec![doc.root()];
        while let Some(n) = stack.pop() {
            reachable.insert(n);
            stack.extend(doc.children(n));
        }
        styles.retain(|id, _| {
            let owner = if is_pseudo_node(*id) {
                pseudo_owner(*id)
            } else {
                *id
            };
            reachable.contains(&owner)
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    /// A small deterministic generator, so a failure names the seed that reproduces it.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.below(items.len())]
        }
    }

    const TAGS: &[&str] = &["div", "span", "p", "ul", "li", "section", "b"];
    const CLASSES: &[&str] = &["a", "b", "c", "d"];
    const STYLES: &[&str] = &[
        "color: red",
        "width: 10px",
        "display: none",
        "font-size: 20px",
        "margin-top: 3px",
        "--x: 9px",
    ];

    /// Rules that look at classes, ids, ancestors, parents, siblings, positions,
    /// emptiness, inheritance, custom properties, generated content and `rem`.
    const SHEET: &str = "\
        div { color: blue; --x: 1px } span { width: var(--x) } p { font-size: 2rem }\
        .a { margin-top: 5px } .a .b { color: green } .a > .c { width: 40px } \
        .a + .b { margin-top: 9px } .b ~ .c { display: block } \
        li:first-child { color: red } li:last-child { margin-top: 1px } \
        li:nth-child(2n+1) { width: 7px } :not(.a) > .d { height: 3px } \
        .c:only-child { color: black } .a:empty { width: 1px } \
        div:first-of-type { margin-top: 2px } p::before { content: 'x'; color: red } \
        #i1 { width: 55px } ul > li + li { margin-top: 6px } section { font-size: 18px }";

    fn random_element(doc: &mut Document, rng: &mut Rng, ids: &mut u32) -> NodeId {
        let mut attributes = Vec::new();
        if rng.below(3) > 0 {
            let n = 1 + rng.below(2);
            let classes: Vec<&str> = (0..n).map(|_| *rng.pick(CLASSES)).collect();
            attributes.push(("class".to_string(), classes.join(" ")));
        }
        if rng.below(8) == 0 {
            *ids += 1;
            attributes.push(("id".to_string(), format!("i{ids}")));
        }
        if rng.below(5) == 0 {
            attributes.push(("style".to_string(), rng.pick(STYLES).to_string()));
        }
        doc.create_node(NodeData::Element {
            tag_name: rng.pick(TAGS).to_string(),
            attributes,
        })
    }

    fn grow(doc: &mut Document, rng: &mut Rng, parent: NodeId, depth: usize, ids: &mut u32) {
        for _ in 0..rng.below(4) {
            if rng.below(5) == 0 {
                let t = doc.create_node(NodeData::Text {
                    data: "t".to_string(),
                });
                doc.append_child(parent, t);
            }
            let child = random_element(doc, rng, ids);
            doc.append_child(parent, child);
            if depth < 4 {
                grow(doc, rng, child, depth + 1, ids);
            }
        }
    }

    fn elements(doc: &Document) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![doc.root()];
        while let Some(n) = stack.pop() {
            if is_element(doc, n) {
                out.push(n);
            }
            stack.extend(doc.children(n));
        }
        out.sort_by_key(|n| n.as_u64());
        out
    }

    fn set_attribute(doc: &mut Document, node: NodeId, name: &str, value: Option<String>) {
        if let NodeData::Element { attributes, .. } = doc.data_mut(node) {
            attributes.retain(|(k, _)| k != name);
            if let Some(value) = value {
                attributes.push((name.to_string(), value));
            }
        }
    }

    /// One random edit.
    fn mutate(doc: &mut Document, rng: &mut Rng, ids: &mut u32) {
        let all = elements(doc);
        let target = *rng.pick(&all);
        let is_root = doc.parent(target) == Some(doc.root());
        match rng.below(8) {
            0 => set_attribute(doc, target, "class", Some(rng.pick(CLASSES).to_string())),
            1 => set_attribute(doc, target, "class", None),
            2 => set_attribute(doc, target, "style", Some(rng.pick(STYLES).to_string())),
            3 => set_attribute(doc, target, "style", None),
            4 => {
                *ids += 1;
                set_attribute(doc, target, "id", Some(format!("i{}", rng.below(3) + 1)));
            }
            5 => {
                let child = random_element(doc, rng, ids);
                grow(doc, rng, child, 3, ids);
                let children: Vec<NodeId> = doc.children(target).collect();
                let reference = if children.is_empty() || rng.below(2) == 0 {
                    None
                } else {
                    Some(*rng.pick(&children))
                };
                doc.insert_before(target, child, reference);
            }
            6 if !is_root => {
                if rng.below(2) == 0 {
                    doc.remove_subtree(target);
                } else {
                    doc.detach(target);
                }
            }
            7 if !is_root => {
                // Move it under another element that is not inside it.
                let mut inside = HashSet::new();
                let mut stack = vec![target];
                while let Some(n) = stack.pop() {
                    inside.insert(n);
                    stack.extend(doc.children(n));
                }
                let candidates: Vec<NodeId> =
                    all.into_iter().filter(|n| !inside.contains(n)).collect();
                if !candidates.is_empty() {
                    let new_parent = *rng.pick(&candidates);
                    doc.detach(target);
                    doc.append_child(new_parent, target);
                }
            }
            _ => {}
        }
    }

    fn full(doc: &Document, rules: &[(Origin, &[Rule])]) -> HashMap<NodeId, ComputedStyle> {
        cascade_with_viewport(doc, rules, 1000.0, 500.0)
    }

    #[test]
    fn incremental_restyling_equals_a_full_cascade_after_random_edits() {
        let ua = ua_stylesheet();
        let author = parse(SHEET);
        let rules: Vec<(Origin, &[Rule])> = vec![(Origin::Ua, &ua), (Origin::Author, &author)];
        let (mut incremental_runs, mut fallbacks) = (0, 0);
        for seed in 1..=60u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut doc = blueice_html::parse("<html><body></body></html>");
            let body = elements(&doc)
                .into_iter()
                .find(|n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == "body"))
                .unwrap();
            let mut ids = 0;
            grow(&mut doc, &mut rng, body, 0, &mut ids);
            let mut styles = full(&doc, &rules);
            doc.take_changes();
            for step in 0..40 {
                for _ in 0..1 + rng.below(3) {
                    mutate(&mut doc, &mut rng, &mut ids);
                }
                let changes = doc.take_changes();
                let expected = full(&doc, &rules);
                if cascade_incremental(&doc, &rules, 1000.0, 500.0, &mut styles, &changes) {
                    incremental_runs += 1;
                } else {
                    fallbacks += 1;
                    styles = expected.clone();
                }
                assert!(
                    styles == expected,
                    "seed {seed} step {step}: incremental styles differ from a full cascade ({} vs {} entries, first difference: {:?})",
                    styles.len(),
                    expected.len(),
                    expected
                        .iter()
                        .find(|(id, style)| styles.get(id) != Some(style))
                        .map(|(id, _)| (id.as_u64(), doc.contains(*id)))
                        .or_else(|| styles.keys().find(|id| !expected.contains_key(id)).map(|id| (id.as_u64(), true)))
                );
            }
        }
        assert!(
            incremental_runs > 4 * fallbacks,
            "the incremental path must actually be taken ({incremental_runs} incremental, {fallbacks} fallbacks)"
        );
    }

    #[test]
    fn a_style_attribute_change_restyles_only_that_element() {
        let ua = ua_stylesheet();
        let author = parse("p { color: blue }");
        let rules: Vec<(Origin, &[Rule])> = vec![(Origin::Ua, &ua), (Origin::Author, &author)];
        let mut doc = blueice_html::parse("<div><p id=a>x</p><p id=b>y</p></div>");
        let mut styles = full(&doc, &rules);
        doc.take_changes();
        let a = elements(&doc)
            .into_iter()
            .find(|n| matches!(doc.data(*n), NodeData::Element { attributes, .. } if attributes.iter().any(|(k, v)| k == "id" && v == "a")))
            .unwrap();
        set_attribute(&mut doc, a, "style", Some("width: 33px".to_string()));
        let changes = doc.take_changes();
        let before = styles.clone();
        assert!(cascade_incremental(
            &doc,
            &rules,
            1000.0,
            500.0,
            &mut styles,
            &changes
        ));
        assert_eq!(styles, full(&doc, &rules));
        let different: Vec<NodeId> = styles
            .iter()
            .filter(|(id, s)| before.get(id) != Some(s))
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(different, vec![a], "nothing else changed");
    }

    #[test]
    fn a_change_to_the_root_font_size_falls_back_to_a_full_cascade() {
        let ua = ua_stylesheet();
        let author = parse("p { font-size: 2rem }");
        let rules: Vec<(Origin, &[Rule])> = vec![(Origin::Ua, &ua), (Origin::Author, &author)];
        let mut doc = blueice_html::parse("<p>x</p>");
        let mut styles = full(&doc, &rules);
        doc.take_changes();
        let html = elements(&doc)
            .into_iter()
            .find(|n| doc.parent(*n) == Some(doc.root()))
            .unwrap();
        set_attribute(&mut doc, html, "style", Some("font-size: 30px".to_string()));
        let changes = doc.take_changes();
        let before = styles.clone();
        assert!(!cascade_incremental(
            &doc,
            &rules,
            1000.0,
            500.0,
            &mut styles,
            &changes
        ));
        assert_eq!(
            styles, before,
            "left untouched for the caller's full cascade"
        );
    }

    #[test]
    fn an_overflowed_journal_and_a_quiet_one() {
        let ua = ua_stylesheet();
        let rules: Vec<(Origin, &[Rule])> = vec![(Origin::Ua, &ua)];
        let doc = blueice_html::parse("<p>x</p>");
        let mut styles = full(&doc, &rules);
        assert!(cascade_incremental(
            &doc,
            &rules,
            1000.0,
            500.0,
            &mut styles,
            &Changes::default()
        ));
        let overflow = Changes {
            overflow: true,
            ..Changes::default()
        };
        assert!(!cascade_incremental(
            &doc,
            &rules,
            1000.0,
            500.0,
            &mut styles,
            &overflow
        ));
    }
}
