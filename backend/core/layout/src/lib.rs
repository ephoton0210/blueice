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
mod cache;
mod ctx;
mod flex;
mod float;
mod fragment;
mod grid;
mod inline;
mod intrinsic;
mod native_text;
mod table;
mod text;
mod transform;

pub use block::update_select_option_bounds;
pub use cache::LayoutCache;
pub use fragment::{Constraints, Fragment, FragmentKind, NativeForm, SelectOption};
pub use native_text::{
    input_button_label, input_range_values, select_display_size, select_options,
};
pub use transform::Translation;

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
    layout_inner(doc, root, styles, constraints, None)
}

/// [`layout`] that keeps what it computes in `cache` and reuses what an earlier call left
/// there. Before each call after the document or styles changed, `cache` must have been
/// told what changed ([`LayoutCache::invalidate`]) or been cleared; the result is then
/// exactly what [`layout`] returns.
pub fn layout_with_cache(
    doc: &Document,
    root: NodeId,
    styles: &HashMap<NodeId, ComputedStyle>,
    constraints: Constraints,
    cache: &mut LayoutCache,
) -> Fragment {
    layout_inner(doc, root, styles, constraints, Some(cache))
}

fn layout_inner(
    doc: &Document,
    root: NodeId,
    styles: &HashMap<NodeId, ComputedStyle>,
    constraints: Constraints,
    cache: Option<&mut LayoutCache>,
) -> Fragment {
    let effective_root = if styles.contains_key(&root) {
        Some(root)
    } else {
        doc.children(root).find(|c| styles.contains_key(c))
    };
    let Some(node) = effective_root else {
        if let Some(cache) = cache {
            cache.clear();
        }
        return Fragment::empty_block();
    };
    let (width, height) = (constraints.available_width, constraints.viewport_height);
    let (mut fragment, layouts) = match cache {
        Some(cache) => {
            let epoch = [
                width.to_bits(),
                height.to_bits(),
                styles.get(&node).map_or(16.0, |s| s.font_size_px).to_bits(),
            ];
            if cache.epoch != Some(epoch) {
                cache.layouts.clear();
                cache.epoch = Some(epoch);
            }
            let (fragment, layouts) = block::layout_root_with(
                doc,
                node,
                styles,
                width,
                height,
                std::mem::take(&mut cache.layouts),
            );
            cache.layouts = layouts;
            (fragment, HashMap::new())
        }
        None => block::layout_root_with(doc, node, styles, width, height, HashMap::new()),
    };
    drop(layouts);
    transform::apply(&mut fragment);
    fragment
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
                viewport_height: 600.0,
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
                viewport_height: 600.0,
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
                viewport_height: 600.0,
            },
        );
        assert_eq!(fragment.node, None);
        assert_eq!(fragment.width, 0.0);
    }
}
