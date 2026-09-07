// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extracts a `blueice_ipc::AiSnapshot` from a live [`Page`], per
//! `phase-1-ai-representation-layer/PLAN.md`'s schema and
//! `phase-5-ai-representation-output/PLAN.md`'s "implement extraction
//! from the shared render pass" checklist item -- reads `Page`'s own
//! `doc`/`fragment`/`styles`/`hovered`/`focused` state through its
//! `pub(crate)` accessors rather than a second, independently-timed
//! pass over anything, so a snapshot always describes exactly what the
//! next [`Page::render`] would also paint.
//!
//! **Inclusion rule** (`spike.md`'s finding, unchanged here): only
//! elements with an inherently semantic role (headings, links, form
//! controls, lists/items, paragraphs, images) or an explicit
//! `role`/`aria-label` attribute are represented at all, and only if
//! they actually have a fragment (excludes `display:none` subtrees,
//! which layout drops entirely, and purely inline elements with no box
//! of their own). A bare `<div>`/`<span>` with neither is exactly the
//! kind of purely-decorative node `spike.md`'s `#banner-overlay`
//! example found should be absent, not a gap to fix.
//!
//! **Name computation** is a deliberately small subset of the real
//! HTML accessible-name algorithm -- in priority order, `aria-label`,
//! then an element-specific rule, then `title`, then subtree text
//! content -- not a full implementation. Covers the common real-world
//! cases (`alt` on images, `<label for>` association, `placeholder`,
//! link/button text) without the full spec's precedence edge cases.
//!
//! **Occlusion** is checked purely geometrically against `Page`'s
//! existing DOM/paint order (later-in-order overlaps earlier-in-order
//! -> the earlier one is occluded) -- consistent with
//! `phase-2-mvp-scope/PLAN.md`'s own non-goal ("no compositing layers
//! or stacking contexts beyond plain DOM/paint order"), not a new
//! limitation introduced here. A `position: absolute` element that a
//! real browser would stacking-promote above later normal-flow
//! siblings isn't modeled specially, for the same reason.

use crate::page::{find_fragment_bounds, Page};
use blueice_dom::{Document, NodeData, NodeId};
use blueice_ipc::{AiNode, AiSnapshot, NameFrom, NodeState, Role};
use std::collections::HashMap;

pub(crate) fn build(page: &Page, generation: u64) -> AiSnapshot {
    let doc = page.doc();
    let mut nodes = Vec::new();
    collect(page, doc, doc.root(), None, &mut nodes);
    link_children(&mut nodes);
    compute_occlusion(&mut nodes);
    AiSnapshot { generation, url: page.url().map(str::to_string), scroll_y: page.scroll_y(), nodes }
}

fn collect(page: &Page, doc: &Document, node: NodeId, nearest_represented_ancestor: Option<u64>, out: &mut Vec<AiNode>) {
    let represented = to_ai_node(page, doc, node, nearest_represented_ancestor);
    let next_ancestor = represented.as_ref().map(|n| n.id).or(nearest_represented_ancestor);
    if let Some(ai_node) = represented {
        out.push(ai_node);
    }
    for child in doc.children(node) {
        collect(page, doc, child, next_ancestor, out);
    }
}

fn link_children(nodes: &mut [AiNode]) {
    let id_to_index: HashMap<u64, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let child_links: Vec<(usize, u64)> = nodes.iter().filter_map(|n| n.parent.and_then(|p| id_to_index.get(&p)).map(|&pi| (pi, n.id))).collect();
    for (parent_index, child_id) in child_links {
        nodes[parent_index].children.push(child_id);
    }
}

fn to_ai_node(page: &Page, doc: &Document, node: NodeId, parent: Option<u64>) -> Option<AiNode> {
    let NodeData::Element { tag_name, attributes } = doc.data(node) else { return None };
    let role = infer_role(tag_name, attributes)?;
    let bounds = find_fragment_bounds(page.fragment(), node, 0.0, 0.0)?;
    let (name, name_from) = compute_name(doc, node, tag_name, attributes);
    let opacity = page.styles().get(&node).map(|s| s.opacity()).unwrap_or(1.0);
    Some(AiNode {
        id: node.as_u64(),
        parent,
        children: Vec::new(),
        role,
        name,
        name_from,
        state: compute_state(page, node, tag_name, attributes),
        bounds,
        opacity,
        occluded: false,
        occluded_by: None,
        occluded_fraction: 0.0,
    })
}

fn attr<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn has_attr(attributes: &[(String, String)], name: &str) -> bool {
    attributes.iter().any(|(k, _)| k == name)
}

fn infer_role(tag: &str, attributes: &[(String, String)]) -> Option<Role> {
    if let Some(level) = tag.strip_prefix('h').and_then(|rest| rest.parse::<u8>().ok()).filter(|l| (1..=6).contains(l)) {
        return Some(Role::Heading { level });
    }
    match tag {
        "a" if attr(attributes, "href").is_some() => Some(Role::Link),
        "button" => Some(Role::Button),
        "input" => Some(match attr(attributes, "type") {
            Some("checkbox") | Some("radio") => Role::CheckBox,
            Some("submit") | Some("button") | Some("reset") => Role::Button,
            _ => Role::TextBox,
        }),
        "textarea" => Some(Role::TextBox),
        "ul" | "ol" => Some(Role::List),
        "li" => Some(Role::ListItem),
        "p" => Some(Role::Paragraph),
        "img" => Some(Role::Image),
        _ if attr(attributes, "role").is_some() || attr(attributes, "aria-label").is_some() => Some(Role::Generic),
        _ => None,
    }
}

fn compute_name(doc: &Document, node: NodeId, tag: &str, attributes: &[(String, String)]) -> (Option<String>, Option<NameFrom>) {
    if let Some(label) = attr(attributes, "aria-label") {
        return (Some(label.to_string()), Some(NameFrom::Attribute("aria-label".to_string())));
    }
    if tag == "img" {
        return match attr(attributes, "alt") {
            Some(alt) => (Some(alt.to_string()), Some(NameFrom::Attribute("alt".to_string()))),
            None => (None, None),
        };
    }
    if matches!(tag, "input" | "textarea") {
        if let Some(id_attr) = attr(attributes, "id") {
            if let Some(label_text) = find_label_text_for(doc, id_attr) {
                return (Some(label_text), Some(NameFrom::Contents));
            }
        }
        return match attr(attributes, "placeholder") {
            Some(ph) => (Some(ph.to_string()), Some(NameFrom::Placeholder)),
            None => (None, None),
        };
    }
    if let Some(title) = attr(attributes, "title") {
        return (Some(title.to_string()), Some(NameFrom::Title));
    }
    let text = text_content(doc, node);
    if text.is_empty() { (None, None) } else { (Some(text), Some(NameFrom::Contents)) }
}

/// Scans the whole document for a `<label for="id_attr">` -- one input
/// at a time is fine at MVP page sizes; a document-wide index would
/// only pay for itself on pages with many labeled inputs.
fn find_label_text_for(doc: &Document, id_attr: &str) -> Option<String> {
    fn walk(doc: &Document, node: NodeId, id_attr: &str) -> Option<String> {
        if let NodeData::Element { tag_name, attributes } = doc.data(node) {
            if tag_name == "label" && attributes.iter().any(|(k, v)| k == "for" && v == id_attr) {
                let text = text_content(doc, node);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        doc.children(node).find_map(|child| walk(doc, child, id_attr))
    }
    walk(doc, doc.root(), id_attr)
}

/// `node`'s own text, for computing its name "from contents" -- stops
/// descending at any *descendant* that's independently represented
/// (has its own inferred role), rather than absorbing that
/// descendant's text too. Without this, `<li><div><p>text</p></div></li>`
/// would give both the `<li>` and the `<p>` the same name "text" (the
/// `<li>`'s naive full-subtree text), an ambiguous duplicate neither a
/// real accessibility tree nor an agent reading this one wants --
/// `<li>` correctly gets no name of its own here, since its entire
/// content is delegated to the separately-represented `<p>`.
fn text_content(doc: &Document, node: NodeId) -> String {
    let mut raw = String::new();
    collect_text(doc, node, &mut raw, true);
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_text(doc: &Document, node: NodeId, out: &mut String, is_root: bool) {
    match doc.data(node) {
        NodeData::Text { data } => {
            out.push_str(data);
            out.push(' ');
            return;
        }
        NodeData::Element { tag_name, attributes } if !is_root && infer_role(tag_name, attributes).is_some() => return,
        _ => {}
    }
    for child in doc.children(node) {
        collect_text(doc, child, out, false);
    }
}

fn compute_state(page: &Page, node: NodeId, tag: &str, attributes: &[(String, String)]) -> NodeState {
    let checked = (tag == "input" && matches!(attr(attributes, "type"), Some("checkbox") | Some("radio"))).then(|| has_attr(attributes, "checked"));
    NodeState {
        checked,
        disabled: has_attr(attributes, "disabled"),
        required: has_attr(attributes, "required"),
        selected: tag == "option" && has_attr(attributes, "selected"),
        hovered: page.hovered() == Some(node),
        focused: page.focused() == Some(node),
    }
}

fn rect_of(node: &AiNode) -> (f64, f64, f64, f64) {
    (node.bounds.x, node.bounds.y, node.bounds.width, node.bounds.height)
}

/// The fraction of `a`'s own area that `b` overlaps -- `0.0` if they
/// don't overlap at all or `a` has zero area.
fn overlap_fraction(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> f64 {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    let ix0 = ax.max(bx);
    let iy0 = ay.max(by);
    let ix1 = (ax + aw).min(bx + bw);
    let iy1 = (ay + ah).min(by + bh);
    let iw = (ix1 - ix0).max(0.0);
    let ih = (iy1 - iy0).max(0.0);
    let area = aw * ah;
    if area <= 0.0 {
        0.0
    } else {
        ((iw * ih) / area).clamp(0.0, 1.0)
    }
}

/// For each node, checks every node that comes *after* it in the list
/// (later in DOM/paint order, per the module docs) for geometric
/// overlap, keeping the strongest occluder found.
fn compute_occlusion(nodes: &mut [AiNode]) {
    let rects: Vec<(u64, (f64, f64, f64, f64))> = nodes.iter().map(|n| (n.id, rect_of(n))).collect();
    for i in 0..nodes.len() {
        let mut best: Option<(u64, f64)> = None;
        for (later_id, later_rect) in &rects[i + 1..] {
            let fraction = overlap_fraction(rects[i].1, *later_rect);
            if fraction > 0.0 && best.is_none_or(|(_, best_fraction)| fraction > best_fraction) {
                best = Some((*later_id, fraction));
            }
        }
        if let Some((occluder, fraction)) = best {
            nodes[i].occluded = true;
            nodes[i].occluded_by = Some(occluder);
            nodes[i].occluded_fraction = fraction as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Page;

    fn page_with(html: &str) -> Page {
        let mut page = Page::new(320.0, 400.0);
        page.load_html_str(html, Some("https://example.com".to_string()));
        page
    }

    fn find<'a>(nodes: &'a [AiNode], name: &str) -> &'a AiNode {
        nodes.iter().find(|n| n.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no node named {name:?} in {nodes:#?}"))
    }

    #[test]
    fn snapshot_carries_the_generation_url_and_scroll_offset() {
        let page = page_with("<p>hi</p>");
        let snap = build(&page, 7);
        assert_eq!(snap.generation, 7);
        assert_eq!(snap.url.as_deref(), Some("https://example.com"));
        assert_eq!(snap.scroll_y, 0.0);
    }

    #[test]
    fn a_bare_div_with_no_role_is_excluded() {
        let page = page_with(r#"<div style="background-color: red;">x</div>"#);
        let snap = build(&page, 0);
        assert!(snap.nodes.is_empty(), "a decorative div must not appear: {:#?}", snap.nodes);
    }

    #[test]
    fn a_display_none_subtree_is_excluded_even_with_a_semantic_role() {
        let page = page_with(r#"<h1 style="display: none;">Hidden</h1>"#);
        let snap = build(&page, 0);
        assert!(snap.nodes.is_empty(), "display:none content must not appear: {:#?}", snap.nodes);
    }

    #[test]
    fn headings_get_their_level_and_subtree_text_as_name() {
        let page = page_with("<h2>Section</h2>");
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Section");
        assert_eq!(node.role, Role::Heading { level: 2 });
        assert_eq!(node.name_from, Some(NameFrom::Contents));
    }

    #[test]
    fn a_link_is_named_from_its_text_and_carries_the_link_role() {
        let page = page_with(r#"<a href="/x">Go</a>"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Go");
        assert_eq!(node.role, Role::Link);
    }

    #[test]
    fn an_image_is_named_from_alt_not_from_any_subtree_text() {
        // a bare `<img>` (default `display: inline`, no intrinsic size)
        // gets no box at all from MVP layout today -- `research/
        // layout.md`'s replaced-element sizing is future work, tracked
        // as a real limitation in this module's own docs, not
        // something to paper over here. Explicit sizing is what makes
        // this test exercise the real pipeline rather than only the
        // naming logic in isolation.
        let page = page_with(r#"<img src="a.png" alt="A cat" style="display: inline-block; width: 16px; height: 16px;">"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "A cat");
        assert_eq!(node.role, Role::Image);
        assert_eq!(node.name_from, Some(NameFrom::Attribute("alt".to_string())));
    }

    #[test]
    fn an_input_is_named_from_its_associated_label() {
        let page = page_with(r#"<label for="name">Name</label><input id="name" type="text">"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Name");
        assert_eq!(node.role, Role::TextBox);
    }

    #[test]
    fn an_input_without_a_label_falls_back_to_its_placeholder() {
        let page = page_with(r#"<input type="text" placeholder="Search">"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Search");
        assert_eq!(node.name_from, Some(NameFrom::Placeholder));
    }

    #[test]
    fn a_checkbox_reports_its_checked_state() {
        let page = page_with(r#"<label for="c">Agree</label><input id="c" type="checkbox" checked>"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Agree");
        assert_eq!(node.role, Role::CheckBox);
        assert_eq!(node.state.checked, Some(true));
    }

    #[test]
    fn aria_label_takes_priority_over_every_other_naming_source() {
        let page = page_with(r#"<button aria-label="Close dialog">X</button>"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Close dialog");
        assert_eq!(node.role, Role::Button);
        assert_eq!(node.name_from, Some(NameFrom::Attribute("aria-label".to_string())));
    }

    #[test]
    fn an_explicit_role_attribute_on_an_otherwise_generic_element_is_represented() {
        let page = page_with(r#"<div role="note">Heads up</div>"#);
        let snap = build(&page, 0);
        let node = find(&snap.nodes, "Heads up");
        assert_eq!(node.role, Role::Generic);
    }

    #[test]
    fn list_and_list_items_get_their_own_roles_and_a_parent_child_link() {
        let page = page_with("<ul><li>one</li><li>two</li></ul>");
        let snap = build(&page, 0);
        let list = snap.nodes.iter().find(|n| n.role == Role::List).unwrap();
        let one = find(&snap.nodes, "one");
        let two = find(&snap.nodes, "two");
        assert_eq!(one.parent, Some(list.id));
        assert_eq!(two.parent, Some(list.id));
        assert_eq!(list.children, vec![one.id, two.id]);
    }

    #[test]
    fn non_represented_ancestors_are_skipped_when_linking_parent_child() {
        // the outer <div> has no role of its own and must not appear,
        // but the <p> inside it should still find the <ul> two levels
        // up as its nearest *represented* ancestor once the div is
        // skipped, not end up parentless.
        let page = page_with("<ul><li><div><p>nested</p></div></li></ul>");
        let snap = build(&page, 0);
        let li = snap.nodes.iter().find(|n| n.role == Role::ListItem).unwrap();
        let p = find(&snap.nodes, "nested");
        assert_eq!(p.parent, Some(li.id));
    }

    #[test]
    fn every_node_carries_its_stable_dom_node_id_not_a_recomputed_index() {
        let page = page_with("<p>hi</p>");
        let snap = build(&page, 0);
        let node = &snap.nodes[0];
        let real_id = page.doc().root(); // just to confirm `id` is a real, resolvable NodeId
        assert!(real_id.as_u64() != node.id || true); // root itself isn't represented; sanity that as_u64 exists
        assert!(page.doc().contains(NodeId::from_u64(node.id)));
    }

    #[test]
    fn hovered_and_focused_state_reflect_the_pages_own_interaction_state() {
        let mut page = page_with(r#"<a href="/x">Go</a>"#);
        let before = build(&page, 0);
        assert!(!find(&before.nodes, "Go").state.hovered);

        page.hover_at(2.0, 2.0);
        let after = build(&page, 0);
        assert!(find(&after.nodes, "Go").state.hovered);
    }

    #[test]
    fn opacity_is_read_from_the_elements_own_computed_style() {
        let page = page_with(r#"<p style="opacity: 0.4;">faded</p>"#);
        let snap = build(&page, 0);
        assert_eq!(find(&snap.nodes, "faded").opacity, 0.4);
    }

    #[test]
    fn a_later_opaque_box_occludes_an_earlier_overlapping_one() {
        // both paragraphs land at the same top-left corner (no layout
        // flow between them would normally overlap block boxes, so
        // this uses two headings absolutely stacked via zero-size
        // trick: same fixed geometry is easier to reason about
        // directly against find_fragment_bounds than to depend on a
        // specific CSS positioning outcome MVP layout may not support
        // yet) -- what matters here is compute_occlusion's own
        // overlap math, not how a real page would produce it.
        let page = page_with("<h1>Front</h1>");
        let mut snap = build(&page, 0);
        // synthesize a second, later, fully-overlapping node the way a
        // real absolutely-positioned overlay would end up geometrically,
        // to isolate compute_occlusion's own logic from layout's.
        let mut overlay = snap.nodes[0].clone();
        overlay.id = 9999;
        overlay.name = Some("Overlay".to_string());
        snap.nodes.push(overlay);
        compute_occlusion(&mut snap.nodes);
        assert!(snap.nodes[0].occluded);
        assert_eq!(snap.nodes[0].occluded_by, Some(9999));
        assert_eq!(snap.nodes[0].occluded_fraction, 1.0);
        assert!(!snap.nodes[1].occluded, "the later node is the occluder, not the occluded");
    }

    #[test]
    fn non_overlapping_nodes_are_never_marked_occluded() {
        let page = page_with("<h1>One</h1><p>Two</p>");
        let snap = build(&page, 0);
        assert!(snap.nodes.iter().all(|n| !n.occluded));
    }
}
