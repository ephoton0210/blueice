// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A layout cache that survives between passes.
//!
//! A block-level layout is a pure function of its node's subtree (the document and the
//! computed styles under it) and of the constraint it was laid out against, and within one
//! pass the engine already memoises on exactly that. Keeping the memo between passes makes
//! a pass after a small change cost the changed path instead of the whole page, provided the
//! entries whose inputs changed are dropped first ([`LayoutCache::invalidate`]).
//!
//! A node's layout reads more than its own subtree in two places only, and the
//! invalidation covers both: a list item's marker number (`block::list_ordinal`) reads its
//! parent's `start`/`reversed` and which siblings are list items, so every child of a parent
//! that anything changed under is dropped too; and a generated `::before`/`::after` box is
//! its owner's. Everything else a layout depends on (viewport, root font size) is part of
//! the cache's epoch, and the cache is emptied when it changes.

use crate::block::{LayoutKey, Placed};
use blueice_dom::{Changes, Document, NodeId};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct LayoutCache {
    pub(crate) layouts: HashMap<LayoutKey, Placed>,
    /// The viewport width and height and root font size (as bits) the entries were laid
    /// out for.
    pub(crate) epoch: Option<[u64; 3]>,
}

impl LayoutCache {
    pub fn clear(&mut self) {
        self.layouts.clear();
        self.epoch = None;
    }

    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    /// Drops every entry that something in `changes` (the document's own record of what was
    /// edited) or `style_changed` (the nodes whose computed style came to differ) may have
    /// changed the result of.
    pub fn invalidate(&mut self, doc: &Document, changes: &Changes, style_changed: &[NodeId]) {
        if changes.overflow {
            self.clear();
            return;
        }
        let mut dirty: HashSet<NodeId> = HashSet::new();
        // The node, and every ancestor: a subtree that changed changes the layout of each box
        // above it.
        let mark_path = |dirty: &mut HashSet<NodeId>, node: NodeId| {
            let mut current = Some(node);
            while let Some(n) = current {
                if !dirty.insert(n) {
                    break;
                }
                current = doc.parent(n);
            }
        };
        // Also every child of the node's parent: a list item's marker number depends on which
        // of its siblings are list items and on the parent's attributes.
        let mark_siblings = |dirty: &mut HashSet<NodeId>, node: NodeId| {
            if let Some(parent) = doc.parent(node) {
                dirty.extend(doc.children(parent));
            }
        };
        let owner_of = |node: NodeId| {
            if blueice_css::is_pseudo_node(node) {
                blueice_css::pseudo_owner(node)
            } else {
                node
            }
        };
        for &node in style_changed {
            let node = owner_of(node);
            if doc.contains(node) {
                mark_path(&mut dirty, node);
                mark_siblings(&mut dirty, node);
            }
        }
        for &node in changes.touched.keys() {
            if doc.contains(node) {
                mark_path(&mut dirty, node);
                mark_siblings(&mut dirty, node);
                // A parent's `start` or `reversed` is read by each of its children.
                dirty.extend(doc.children(node));
            }
        }
        for &(parent, child) in &changes.attached {
            if doc.contains(child) {
                mark_path(&mut dirty, child);
                mark_siblings(&mut dirty, child);
            }
            if doc.contains(parent) {
                mark_path(&mut dirty, parent);
                dirty.extend(doc.children(parent));
            }
        }
        for detached in &changes.detached {
            if doc.contains(detached.parent) {
                mark_path(&mut dirty, detached.parent);
                dirty.extend(doc.children(detached.parent));
            }
        }
        self.layouts.retain(|key, _| {
            let node = owner_of(key.node);
            doc.contains(node) && !dirty.contains(&node)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout, layout_with_cache, Constraints, Fragment};
    use blueice_css::{cascade, ua_stylesheet, Origin, Rule};
    use blueice_dom::NodeData;

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

    const TAGS: &[&str] = &[
        "div", "span", "p", "ul", "ol", "li", "li", "section", "b", "table", "tr", "td",
    ];
    const CLASSES: &[&str] = &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
    const STYLES: &[&str] = &[
        "width: 40px",
        "height: 12px",
        "display: none",
        "font-size: 20px",
        "margin-top: 3px",
        "padding: 4px",
        "display: flex",
        "float: right",
        "transform: translate(3px, 4px)",
    ];
    const WORDS: &[&str] = &[
        "alpha",
        "beta",
        "gamma",
        "delta",
        "epsilon zeta",
        "eta theta",
    ];

    /// Rules that exercise floats, inline-blocks, flex, positioning, percentages, clipping,
    /// transforms, tables and list markers.
    const SHEET: &str = "\
        .a { float: left; width: 30px; height: 10px } .b { display: inline-block; width: 20px } \
        .c { display: flex } .c > * { flex: 1 } .d { position: relative; top: 2px; left: 1px } \
        .e { position: absolute; left: 3px; top: 4px; width: 5px; height: 5px } \
        .f { padding: 3px; border: 1px solid red; margin: 4px } p { margin: 6px 0 } \
        li { margin-bottom: 2px } .g { width: 50% } .h { overflow: hidden; height: 20px } \
        .i { transform: translate(5px, 5px) } .j { display: none } \
        ul { padding-left: 20px } ol { padding-left: 24px } table { border-spacing: 2px } \
        td { padding: 1px } section { clear: both }";

    fn random_element(doc: &mut Document, rng: &mut Rng) -> NodeId {
        let mut attributes = Vec::new();
        if rng.below(3) > 0 {
            let n = 1 + rng.below(2);
            let classes: Vec<&str> = (0..n).map(|_| *rng.pick(CLASSES)).collect();
            attributes.push(("class".to_string(), classes.join(" ")));
        }
        if rng.below(6) == 0 {
            attributes.push(("style".to_string(), rng.pick(STYLES).to_string()));
        }
        let tag = *rng.pick(TAGS);
        if tag == "ol" && rng.below(2) == 0 {
            attributes.push(("start".to_string(), (1 + rng.below(5)).to_string()));
        }
        if tag == "li" && rng.below(5) == 0 {
            attributes.push(("value".to_string(), (1 + rng.below(9)).to_string()));
        }
        doc.create_node(NodeData::Element {
            tag_name: tag.to_string(),
            attributes,
        })
    }

    fn random_text(doc: &mut Document, rng: &mut Rng) -> NodeId {
        doc.create_node(NodeData::Text {
            data: rng.pick(WORDS).to_string(),
        })
    }

    fn grow(doc: &mut Document, rng: &mut Rng, parent: NodeId, depth: usize) {
        for _ in 0..rng.below(4) {
            if rng.below(3) == 0 {
                let text = random_text(doc, rng);
                doc.append_child(parent, text);
            }
            let child = random_element(doc, rng);
            doc.append_child(parent, child);
            if depth < 4 {
                grow(doc, rng, child, depth + 1);
            }
        }
    }

    fn all_nodes(doc: &Document) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![doc.root()];
        while let Some(n) = stack.pop() {
            out.push(n);
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

    fn mutate(doc: &mut Document, rng: &mut Rng) {
        let all = all_nodes(doc);
        let target = *rng.pick(&all);
        let is_element = matches!(doc.data(target), NodeData::Element { .. });
        let is_root_child = doc.parent(target) == Some(doc.root());
        match rng.below(11) {
            0 if is_element => {
                set_attribute(doc, target, "class", Some(rng.pick(CLASSES).to_string()))
            }
            1 if is_element => set_attribute(doc, target, "class", None),
            2 if is_element => {
                set_attribute(doc, target, "style", Some(rng.pick(STYLES).to_string()))
            }
            3 if is_element => set_attribute(doc, target, "style", None),
            4 if is_element => {
                set_attribute(doc, target, "start", Some((1 + rng.below(7)).to_string()))
            }
            5 if is_element => {
                set_attribute(doc, target, "value", Some((1 + rng.below(9)).to_string()))
            }
            6 if !is_element && !matches!(doc.data(target), NodeData::Document) => {
                if let NodeData::Text { data } = doc.data_mut(target) {
                    *data = rng.pick(WORDS).to_string();
                }
            }
            7 if is_element => {
                let child = random_element(doc, rng);
                grow(doc, rng, child, 3);
                let children: Vec<NodeId> = doc.children(target).collect();
                let reference = if children.is_empty() || rng.below(2) == 0 {
                    None
                } else {
                    Some(*rng.pick(&children))
                };
                doc.insert_before(target, child, reference);
            }
            8 if is_element => {
                let text = random_text(doc, rng);
                doc.append_child(target, text);
            }
            9 if !is_root_child && doc.parent(target).is_some() => {
                if rng.below(2) == 0 {
                    doc.remove_subtree(target);
                } else {
                    doc.detach(target);
                }
            }
            10 if !is_root_child && doc.parent(target).is_some() => {
                let mut inside = HashSet::new();
                let mut stack = vec![target];
                while let Some(n) = stack.pop() {
                    inside.insert(n);
                    stack.extend(doc.children(n));
                }
                let candidates: Vec<NodeId> = all
                    .into_iter()
                    .filter(|n| {
                        !inside.contains(n) && matches!(doc.data(*n), NodeData::Element { .. })
                    })
                    .collect();
                if !candidates.is_empty() {
                    let new_parent = *rng.pick(&candidates);
                    doc.detach(target);
                    doc.append_child(new_parent, target);
                }
            }
            _ => {}
        }
    }

    fn styles_of(
        doc: &Document,
        author: &[Rule],
    ) -> std::collections::HashMap<NodeId, blueice_css::ComputedStyle> {
        let ua = ua_stylesheet();
        cascade(doc, &[(Origin::Ua, &ua), (Origin::Author, author)])
    }

    fn constraints() -> Constraints {
        Constraints {
            available_width: 400.0,
            viewport_height: 300.0,
        }
    }

    fn changed_styles(
        before: &std::collections::HashMap<NodeId, blueice_css::ComputedStyle>,
        after: &std::collections::HashMap<NodeId, blueice_css::ComputedStyle>,
    ) -> Vec<NodeId> {
        before
            .keys()
            .chain(after.keys())
            .filter(|id| before.get(id) != after.get(id))
            .copied()
            .collect()
    }

    fn first_difference(a: &Fragment, b: &Fragment, path: &mut Vec<usize>) -> Option<String> {
        if a.node != b.node
            || a.kind != b.kind
            || a.x != b.x
            || a.y != b.y
            || a.width != b.width
            || a.height != b.height
        {
            return Some(format!(
                "at {path:?}: {:?} {} {} {} {} vs {:?} {} {} {} {}",
                a.node, a.x, a.y, a.width, a.height, b.node, b.x, b.y, b.width, b.height
            ));
        }
        if a.children.len() != b.children.len() {
            return Some(format!(
                "at {path:?}: {} children vs {}",
                a.children.len(),
                b.children.len()
            ));
        }
        for (i, (x, y)) in a.children.iter().zip(&b.children).enumerate() {
            path.push(i);
            if let Some(found) = first_difference(x, y, path) {
                return Some(found);
            }
            path.pop();
        }
        (a != b).then(|| format!("at {path:?}: only a field outside the geometry differs"))
    }

    #[test]
    fn a_cached_layout_equals_a_fresh_one_after_random_edits() {
        let author = blueice_css::parse(SHEET).rules;
        let (mut kept, mut possible) = (0usize, 0usize);
        for seed in 1..=60u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut doc = blueice_html::parse("<html><body></body></html>");
            let body = all_nodes(&doc)
                .into_iter()
                .find(|n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == "body"))
                .unwrap();
            grow(&mut doc, &mut rng, body, 0);
            let mut styles = styles_of(&doc, &author);
            let mut cache = LayoutCache::default();
            doc.take_changes();
            let first = layout_with_cache(&doc, doc.root(), &styles, constraints(), &mut cache);
            assert_eq!(
                first,
                layout(&doc, doc.root(), &styles, constraints()),
                "seed {seed}: the first pass"
            );
            for step in 0..40 {
                for _ in 0..1 + rng.below(3) {
                    mutate(&mut doc, &mut rng);
                }
                let changes = doc.take_changes();
                let new_styles = styles_of(&doc, &author);
                let changed = changed_styles(&styles, &new_styles);
                styles = new_styles;
                let before = cache.len();
                cache.invalidate(&doc, &changes, &changed);
                kept += cache.len();
                possible += before;
                let cached =
                    layout_with_cache(&doc, doc.root(), &styles, constraints(), &mut cache);
                let fresh = layout(&doc, doc.root(), &styles, constraints());
                assert!(
                    cached == fresh,
                    "seed {seed} step {step}: the cached layout differs from a fresh one, {:?}",
                    first_difference(&cached, &fresh, &mut Vec::new())
                );
            }
        }
        assert!(
            kept * 10 > possible * 3,
            "the cache must actually keep entries across edits ({kept} of {possible} kept)"
        );
    }

    /// A list's items number themselves from their siblings and their parent, so editing one
    /// item (or the list) changes the markers of items nothing happened to.
    #[test]
    fn list_numbers_stay_right_in_a_cached_layout() {
        let author = blueice_css::parse(
            "li { margin-bottom: 1px } .j { display: none } .k { list-style: upper-roman } \
             ol { padding-left: 30px }",
        )
        .rules;
        for seed in 1..=60u64 {
            let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
            let mut doc = blueice_html::parse(
                "<html><body><ol id=l><li>a</li><li>b</li><li>c</li><li>d</li><li>e</li><li>f</li></ol>\
                 <ul><li>x</li><li>y</li><li>z</li></ul></body></html>",
            );
            let mut styles = styles_of(&doc, &author);
            let mut cache = LayoutCache::default();
            doc.take_changes();
            layout_with_cache(&doc, doc.root(), &styles, constraints(), &mut cache);
            for step in 0..60 {
                let items: Vec<NodeId> = all_nodes(&doc)
                    .into_iter()
                    .filter(|n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == "li"))
                    .collect();
                let lists: Vec<NodeId> = all_nodes(&doc)
                    .into_iter()
                    .filter(|n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == "ol" || tag_name == "ul"))
                    .collect();
                match rng.below(8) {
                    0 if !items.is_empty() => {
                        let item = *rng.pick(&items);
                        set_attribute(&mut doc, item, "class", Some("j".to_string()));
                    }
                    1 if !items.is_empty() => {
                        let item = *rng.pick(&items);
                        set_attribute(&mut doc, item, "class", None);
                    }
                    2 if !items.is_empty() => {
                        let item = *rng.pick(&items);
                        set_attribute(
                            &mut doc,
                            item,
                            "value",
                            Some((1 + rng.below(9)).to_string()),
                        );
                    }
                    3 if !lists.is_empty() => {
                        let list = *rng.pick(&lists);
                        set_attribute(
                            &mut doc,
                            list,
                            "start",
                            Some((1 + rng.below(9)).to_string()),
                        );
                    }
                    4 if !lists.is_empty() => {
                        let list = *rng.pick(&lists);
                        let reversed = rng.below(2) == 0;
                        set_attribute(&mut doc, list, "reversed", reversed.then(String::new));
                    }
                    5 if !lists.is_empty() => {
                        let list = *rng.pick(&lists);
                        let item = doc.create_node(NodeData::Element {
                            tag_name: "li".to_string(),
                            attributes: Vec::new(),
                        });
                        let text = random_text(&mut doc, &mut rng);
                        doc.append_child(item, text);
                        let children: Vec<NodeId> = doc.children(list).collect();
                        let reference = (!children.is_empty()).then(|| *rng.pick(&children));
                        doc.insert_before(list, item, reference);
                    }
                    6 if items.len() > 1 => {
                        let item = *rng.pick(&items);
                        doc.remove_subtree(item);
                    }
                    7 if !lists.is_empty() => {
                        let list = *rng.pick(&lists);
                        set_attribute(&mut doc, list, "class", Some("k".to_string()));
                    }
                    _ => {}
                }
                let changes = doc.take_changes();
                let new_styles = styles_of(&doc, &author);
                let changed = changed_styles(&styles, &new_styles);
                styles = new_styles;
                cache.invalidate(&doc, &changes, &changed);
                let cached =
                    layout_with_cache(&doc, doc.root(), &styles, constraints(), &mut cache);
                let fresh = layout(&doc, doc.root(), &styles, constraints());
                assert!(
                    cached == fresh,
                    "seed {seed} step {step}: {:?}",
                    first_difference(&cached, &fresh, &mut Vec::new())
                );
            }
        }
    }
}
