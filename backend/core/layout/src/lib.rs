// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Layout: DOM + computed styles -> a box/fragment tree.
//!
//! See `development/browser_core/research/layout.md` for the target
//! architecture (ported here, not just referenced): a freshly-built
//! immutable [`Fragment`] tree per layout pass (Blink LayoutNG-style),
//! preferred over Gecko's mutable frame graph because the latter fights
//! Rust's ownership model; a single `match` on `display` dispatching to
//! per-algorithm implementations, with block/inline handled together
//! (they're not separable, per the research) and flex/grid as later
//! arms. Scope per `phase-2-mvp-scope/PLAN.md`: no floats, no CSS2
//! margin collapsing, no real text shaping (see `text.rs`), flexbox
//! generates an ordinary block box for now (no special flex
//! positioning yet).

mod block;
mod fragment;
mod text;

pub use fragment::{Constraints, Fragment, FragmentKind};

use blueice_css::ComputedStyle;
use blueice_dom::{Document, NodeId};
use std::collections::HashMap;

/// Lays out `root` (and its subtree) against `constraints`, using each
/// element's already-cascaded [`ComputedStyle`] from `styles` (the
/// output of `blueice_css::cascade`). If `root` itself has no computed
/// style (e.g. it's `doc.root()`, the `Document` node, which is never a
/// cascade target), its first styled child is laid out as the root box
/// instead.
pub fn layout(
    doc: &Document,
    root: NodeId,
    styles: &HashMap<NodeId, ComputedStyle>,
    constraints: Constraints,
) -> Fragment {
    let effective_root = if styles.contains_key(&root) {
        Some(root)
    } else {
        doc.children(root).find(|c| styles.contains_key(c))
    };
    match effective_root {
        Some(node) => block::layout_block(doc, node, styles, constraints.available_width),
        None => Fragment::empty_block(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::NodeData;

    #[test]
    fn layout_from_document_root_finds_the_html_element() {
        let doc = blueice_html::parse("<p>hi</p>");
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        let root = doc.root();
        let fragment = layout(
            &doc,
            root,
            &styles,
            Constraints {
                available_width: 800.0,
            },
        );
        assert_eq!(fragment.kind, FragmentKind::Block);
        assert!(
            matches!(doc.data(fragment.node.unwrap()), NodeData::Element { tag_name, .. } if tag_name == "html")
        );
    }

    #[test]
    fn layout_called_directly_on_a_styled_node_works_too() {
        let doc = blueice_html::parse("<p>hi</p>");
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        let p = *styles
            .keys()
            .find(|&&n| matches!(doc.data(n), NodeData::Element{tag_name, ..} if tag_name=="p"))
            .unwrap();
        let fragment = layout(
            &doc,
            p,
            &styles,
            Constraints {
                available_width: 800.0,
            },
        );
        assert_eq!(fragment.node, Some(p));
    }

    #[test]
    fn layout_on_a_root_with_no_styled_children_returns_an_empty_fragment() {
        let doc = Document::new();
        let styles = HashMap::new();
        let fragment = layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: 800.0,
            },
        );
        assert_eq!(fragment.node, None);
        assert_eq!(fragment.width, 0.0);
    }
}
