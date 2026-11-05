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

    /// The boxes of the element named `tag` with the given `id` (searching the whole fragment tree).
    fn find_by_id<'a>(
        doc: &blueice_dom::Document,
        f: &'a Fragment,
        id: &str,
    ) -> Option<&'a Fragment> {
        if let Some(node) = f.node {
            if let NodeData::Element { attributes, .. } = doc.data(node) {
                if attributes.iter().any(|(k, v)| k == "id" && v == id) {
                    return Some(f);
                }
            }
        }
        f.children.iter().find_map(|c| find_by_id(doc, c, id))
    }

    /// The width of the box with `id` in `html`, laid out in a 1280 px window.
    fn width_of(html: &str, id: &str) -> f64 {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        let fragment = layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: 1280.0,
                viewport_height: 900.0,
            },
        );
        find_by_id(&doc, &fragment, id)
            .unwrap_or_else(|| panic!("no box with id {id}"))
            .width
    }

    #[test]
    fn a_centred_item_of_a_column_flex_box_is_as_wide_as_its_content() {
        // `flex-direction: column; align-items: center` shrink-wraps each item across; only
        // `stretch` (the default) fills the width. A flex container item is no exception.
        let content = width_of(
            "<body style='margin:0'><div style='display:flex;width:1000px'>\
               <a id=b style='display:flex;padding:0 12px'>Start</a></div></body>",
            "b",
        );
        for (label, item) in [
            ("a block", "<a id=b style='display:block;padding:0 12px'>Start</a>"),
            ("an inline-flex box", "<a id=b style='display:inline-flex;padding:0 12px;justify-content:center;align-items:center'>Start</a>"),
            ("a flex box", "<a id=b style='display:flex;padding:0 12px;justify-content:center'>Start</a>"),
            ("a flex box in a box", "<div id=b style='display:flex;padding:0 12px'><span>Start</span></div>"),
        ] {
            let html = format!(
                "<body style='margin:0'><div style='display:flex;flex-direction:column;align-items:center;width:1000px'>{item}</div></body>"
            );
            let width = width_of(&html, "b");
            assert!((width - content).abs() < 1.0, "{label}: {width} px, content is {content}");
        }
        // Stretch still fills the width.
        let stretched = width_of(
            "<body style='margin:0'><div style='display:flex;flex-direction:column;width:1000px'>\
               <a id=b style='display:flex;padding:0 12px'>Start</a></div></body>",
            "b",
        );
        assert!((stretched - 1000.0).abs() < 1.0, "{stretched}");
    }

    #[test]
    fn a_flex_button_is_as_wide_as_its_content_however_the_content_is_aligned() {
        // A button that is itself a flex container with its content centred (or pushed to an
        // end, or spread), and does not shrink, takes its width from its content. Measured at a
        // stand-in "unlimited" width, where aligned content sits far from the start, the
        // alignment must not be counted as part of the content.
        let plain = width_of(
            "<body style='margin:0'><div style='display:flex;width:1000px'>\
               <a id=b style='display:flex;flex-shrink:0;padding:0 12px'>Login</a></div></body>",
            "b",
        );
        for (label, extra) in [
            ("centred", "justify-content:center"),
            ("at the end", "justify-content:flex-end"),
            ("spread", "justify-content:space-between"),
            ("space around", "justify-content:space-around"),
            ("space evenly", "justify-content:space-evenly"),
            ("centred, items centred", "justify-content:center;align-items:center"),
        ] {
            let html = format!(
                "<body style='margin:0'><div style='display:flex;justify-content:flex-end;width:1000px'>\
                   <a id=b style='display:flex;flex-shrink:0;padding:0 12px;{extra}'>Login</a></div></body>"
            );
            let width = width_of(&html, "b");
            assert!(
                (width - plain).abs() < 1.0,
                "{label}: {width} px wide, but the same button without alignment is {plain}"
            );
        }
        // The same with several items and a gap: the width is their sum and the gaps.
        let two = width_of(
            "<body style='margin:0'><div style='display:flex;justify-content:flex-end;width:1000px'>\
               <a id=b style='display:flex;flex-shrink:0;justify-content:center;gap:10px'>\
                 <span style='display:block;width:30px'>a</span><span style='display:block;width:40px'>b</span></a></div></body>",
            "b",
        );
        assert!((two - 80.0).abs() < 1.0, "30 + 10 + 40 is 80, got {two}");
        // A column flex container is as wide as its widest item, wherever they are aligned.
        let column = width_of(
            "<body style='margin:0'><div style='display:flex;justify-content:flex-end;width:1000px'>\
               <a id=b style='display:flex;flex-direction:column;flex-shrink:0;align-items:center'>\
                 <span style='display:block;width:30px'>a</span><span style='display:block;width:70px'>b</span></a></div></body>",
            "b",
        );
        assert!((column - 70.0).abs() < 1.0, "the widest item is 70, got {column}");
    }

    #[test]
    fn a_box_laid_over_a_shrink_wrapped_button_adds_nothing_to_its_width() {
        // A button with a background layer pinned over it (`position:absolute; inset:-1px`, as
        // utility CSS builds them) is as wide as its label, wherever it sits. Measured for its
        // preferred width at a stand-in "unlimited" size, the layer must not stretch it.
        let layer = "<span style='position:absolute;top:-1px;left:-1px;right:-1px;bottom:-1px'></span>";
        let cases = [
            (
                "a flex item that does not shrink",
                format!(
                    "<body style='margin:0'><div style='display:flex;justify-content:flex-end;width:1000px'>\
                       <a id=b style='display:flex;flex-shrink:0;position:relative;padding:0 8px'>Login{layer}</a></div></body>"
                ),
            ),
            (
                "an inline flex box in a flex row",
                format!(
                    "<body style='margin:0'><div style='display:flex;justify-content:flex-end;width:1000px'>\
                       <a id=b style='display:inline-flex;position:relative;padding:0 8px'>Login{layer}</a></div></body>"
                ),
            ),
            (
                "an absolutely positioned shrink-to-fit box",
                format!(
                    "<body style='margin:0'><div style='position:relative;width:1000px;height:50px'>\
                       <a id=b style='position:absolute;top:0;right:0;display:block;padding:0 8px'>Login{layer}</a></div></body>"
                ),
            ),
        ];
        for (label, html) in cases {
            let width = width_of(&html, "b");
            assert!(
                (20.0..120.0).contains(&width),
                "{label}: the button is {width} px wide, not about its label's width"
            );
        }
    }

    #[test]
    fn a_fixed_box_inside_an_absolutely_positioned_box_is_still_laid_out() {
        // A sticky sidebar: `position: fixed` set on a descendant of an absolutely positioned
        // column. The fixed box belongs to the viewport, so it must survive its parent's layout.
        for (label, fixed_style) in [
            ("sized", "position:fixed;top:10px;width:306px"),
            ("shrink-wrapped", "position:fixed;top:10px"),
            ("pinned right", "position:fixed;top:10px;right:20px;width:306px"),
        ] {
            let html = format!(
                "<body style='margin:0'><div style='position:relative;width:1200px'>\
                   <div style='position:absolute;top:0;right:0;width:306px'>\
                     <div id=fx style='{fixed_style}'><section style='padding:4px'><p>hello world</p></section></div>\
                   </div></div></body>"
            );
            let doc = blueice_html::parse(&html);
            let ua = ua_stylesheet();
            let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
            let fragment = layout(
                &doc,
                doc.root(),
                &styles,
                Constraints {
                    available_width: 1280.0,
                    viewport_height: 900.0,
                },
            );
            let fixed = find_by_id(&doc, &fragment, "fx")
                .unwrap_or_else(|| panic!("{label}: the fixed box vanished"));
            assert!(fixed.fixed, "{label}: it keeps its fixed flag");
            assert!(
                fixed.width > 50.0 && fixed.height > 10.0,
                "{label}: laid out with its content: {}x{}",
                fixed.width,
                fixed.height
            );
            assert_eq!(fixed.y, 10.0, "{label}: placed against the viewport, not the column");
        }
    }

    #[test]
    fn a_fixed_box_in_an_ordinary_flow_is_unchanged() {
        let doc = blueice_html::parse(
            "<body style='margin:0'><div><div id=fx style='position:fixed;top:5px;left:7px;width:100px'><p>x</p></div></div></body>",
        );
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        let fragment = layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: 800.0,
                viewport_height: 600.0,
            },
        );
        let fixed = find_by_id(&doc, &fragment, "fx").expect("laid out");
        assert_eq!((fixed.x, fixed.y, fixed.width), (7.0, 5.0, 100.0));
    }
}
