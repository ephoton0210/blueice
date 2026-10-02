// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn find_element_by_id(doc: &Document, node: NodeId, id: &str) -> Option<NodeId> {
    if let NodeData::Element { attributes, .. } = doc.data(node) {
        if attributes
            .iter()
            .any(|(name, value)| name == "id" && value == id)
        {
            return Some(node);
        }
    }
    doc.children(node)
        .find_map(|child| find_element_by_id(doc, child, id))
}

pub(super) fn element_attribute<'a>(
    doc: &'a Document,
    node: NodeId,
    name: &str,
) -> Option<&'a str> {
    let NodeData::Element { attributes, .. } = doc.data(node) else {
        return None;
    };
    attributes
        .iter()
        .find(|(attribute, _)| attribute.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Returns the group identity for one extension-selectable radio. This is
/// intentionally stricter than arbitrary script DOM mutation: the extension
/// provides only a stable node ID, so all group membership comes from the
/// core-owned current document.
pub(super) fn extension_radio_group_scope(
    doc: &Document,
    id: NodeId,
) -> Result<(String, Option<NodeId>), String> {
    if !doc.contains(id) {
        return Err(format!("unknown radio node {}", id.as_u64()));
    }
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(id)
    else {
        return Err(format!(
            "node {} is not an enabled named native radio",
            id.as_u64()
        ));
    };
    let is_radio = tag_name.eq_ignore_ascii_case("input")
        && attributes.iter().any(|(attribute, value)| {
            attribute.eq_ignore_ascii_case("type") && value.eq_ignore_ascii_case("radio")
        });
    let disabled = attributes
        .iter()
        .any(|(attribute, _)| attribute.eq_ignore_ascii_case("disabled"));
    let externally_associated = attributes
        .iter()
        .any(|(attribute, _)| attribute.eq_ignore_ascii_case("form"));
    let name = attributes
        .iter()
        .find(|(attribute, _)| attribute.eq_ignore_ascii_case("name"))
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty());
    if !is_radio || disabled || externally_associated || name.is_none() {
        return Err(format!(
            "node {} is not an enabled named native radio",
            id.as_u64()
        ));
    }
    Ok((
        name.expect("a checked radio name is present"),
        nearest_form_ancestor(doc, id),
    ))
}

pub(super) fn nearest_form_ancestor(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut ancestor = doc.parent(id);
    while let Some(node) = ancestor {
        if matches!(doc.data(node), NodeData::Element { tag_name, .. } if tag_name.eq_ignore_ascii_case("form"))
        {
            return Some(node);
        }
        ancestor = doc.parent(node);
    }
    None
}

pub(super) fn collect_extension_radio_group_members(
    doc: &Document,
    node: NodeId,
    name: &str,
    form_owner: Option<NodeId>,
    members: &mut Vec<NodeId>,
) {
    if let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    {
        let is_member = tag_name.eq_ignore_ascii_case("input")
            && attributes.iter().any(|(attribute, value)| {
                attribute.eq_ignore_ascii_case("type") && value.eq_ignore_ascii_case("radio")
            })
            && !attributes
                .iter()
                .any(|(attribute, _)| attribute.eq_ignore_ascii_case("form"))
            && attributes
                .iter()
                .any(|(attribute, value)| attribute.eq_ignore_ascii_case("name") && value == name)
            && nearest_form_ancestor(doc, node) == form_owner;
        if is_member {
            members.push(node);
        }
    }
    for child in doc.children(node) {
        collect_extension_radio_group_members(doc, child, name, form_owner, members);
    }
}

/// Resolves the single-select that owns an extension-selectable option. This
/// intentionally models only the safe subset whose semantics core can own
/// exactly: a live, enabled native select without `multiple`, and a live,
/// enabled option not contained by a disabled optgroup. Form association does
/// not alter selection membership, so it is intentionally not part of this
/// local DOM transition.
pub(super) fn extension_select_option_owner(doc: &Document, id: NodeId) -> Result<NodeId, String> {
    if !doc.contains(id) {
        return Err(format!("unknown option node {}", id.as_u64()));
    }
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(id)
    else {
        return Err(format!(
            "node {} is not an enabled single-select option",
            id.as_u64()
        ));
    };
    let is_enabled_option = tag_name.eq_ignore_ascii_case("option")
        && !attributes
            .iter()
            .any(|(attribute, _)| attribute.eq_ignore_ascii_case("disabled"));
    let select = nearest_select_ancestor(doc, id);
    let select_is_enabled_single = select.is_some_and(|select| {
        matches!(
            doc.data(select),
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("select")
                && !attributes.iter().any(|(attribute, _)| {
                    attribute.eq_ignore_ascii_case("disabled")
                        || attribute.eq_ignore_ascii_case("multiple")
                })
        )
    });
    if !is_enabled_option || !select_is_enabled_single || has_disabled_optgroup_ancestor(doc, id) {
        return Err(format!(
            "node {} is not an enabled single-select option",
            id.as_u64()
        ));
    }
    Ok(select.expect("a checked enabled single select is present"))
}

pub(super) fn nearest_select_ancestor(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut ancestor = doc.parent(id);
    while let Some(node) = ancestor {
        if matches!(doc.data(node), NodeData::Element { tag_name, .. } if tag_name.eq_ignore_ascii_case("select"))
        {
            return Some(node);
        }
        ancestor = doc.parent(node);
    }
    None
}

pub(super) fn has_disabled_optgroup_ancestor(doc: &Document, id: NodeId) -> bool {
    let mut ancestor = doc.parent(id);
    while let Some(node) = ancestor {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name.eq_ignore_ascii_case("optgroup")
                && attributes
                    .iter()
                    .any(|(attribute, _)| attribute.eq_ignore_ascii_case("disabled"))
            {
                return true;
            }
        }
        ancestor = doc.parent(node);
    }
    false
}

pub(super) fn collect_extension_select_options(
    doc: &Document,
    node: NodeId,
    options: &mut Vec<NodeId>,
) {
    if matches!(doc.data(node), NodeData::Element { tag_name, .. } if tag_name.eq_ignore_ascii_case("option"))
    {
        options.push(node);
    }
    for child in doc.children(node) {
        collect_extension_select_options(doc, child, options);
    }
}

pub(super) fn node_text_content(doc: &Document, node: NodeId) -> String {
    match doc.data(node) {
        NodeData::Text { data } => data.clone(),
        NodeData::Document | NodeData::Element { .. } => doc
            .children(node)
            .map(|child| node_text_content(doc, child))
            .collect(),
    }
}

pub(super) fn crop(pixmap: &Pixmap, top: f64, width: f64, height: f64) -> Pixmap {
    let top = top.round().max(0.0) as u32;
    let w = (width.round().max(0.0) as u32).min(pixmap.width.max(1));
    let h = height.round().max(0.0) as u32;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for row in 0..h {
        let src_row = top + row;
        if src_row < pixmap.height {
            let start = ((src_row * pixmap.width) * 4) as usize;
            let end = start + (w as usize * 4);
            pixels.extend_from_slice(&pixmap.pixels[start..end.min(pixmap.pixels.len())]);
            pixels.resize((row as usize + 1) * w as usize * 4, 255);
        } else {
            pixels.extend(std::iter::repeat_n(255u8, w as usize * 4));
        }
    }
    Pixmap {
        width: w,
        height: h,
        pixels,
    }
}

/// Finds `id`'s own fragment and resolves its bounds to document-
/// content coordinates (accumulating each ancestor's offset the same
/// way `blueice-paint`'s own tree walk does -- a `Fragment`'s `x`/`y`
/// are relative to its parent, not absolute) -- `None` if `id` has no
/// fragment at all, which is exactly right for a `display:none`
/// subtree (dropped entirely by layout, per `research/layout.md`) or a
/// stale ID from before the last navigation. Shared by
/// [`Page::render`]'s highlight overlay, [`Page::act`]'s
/// `ScrollIntoView`, and `ai_snapshot`'s bounds extraction, so the
/// three never disagree about where a node actually is.
pub(crate) fn find_fragment_bounds(
    fragment: &Fragment,
    id: NodeId,
    offset_x: f64,
    offset_y: f64,
) -> Option<blueice_ipc::Bounds> {
    let x = offset_x + fragment.x;
    let y = offset_y + fragment.y;
    if fragment.node == Some(id) {
        return Some(blueice_ipc::Bounds {
            x,
            y,
            width: fragment.width,
            height: fragment.height,
        });
    }
    fragment
        .children
        .iter()
        .find_map(|child| find_fragment_bounds(child, id, x, y))
}

pub(super) const HIGHLIGHT_COLOR: Color = Color::Rgba(255, 149, 0, 255);
pub(super) const HIGHLIGHT_THICKNESS: f64 = 2.0;

/// A four-edge outline around `bounds`, built from ordinary
/// `PaintCommand::BorderEdge`s rather than a new paint-command variant
/// -- an AI-requested highlight is an interaction-layer concept
/// `Page` owns, not a CSS box-model feature `blueice-paint` needs to
/// know about.
pub(super) fn highlight_border_commands(bounds: blueice_ipc::Bounds) -> Vec<PaintCommand> {
    let blueice_ipc::Bounds {
        x,
        y,
        width,
        height,
    } = bounds;
    let t = HIGHLIGHT_THICKNESS;
    vec![
        PaintCommand::BorderEdge {
            rect: Rect {
                x,
                y,
                width,
                height: t,
            },
            color: HIGHLIGHT_COLOR,
        },
        PaintCommand::BorderEdge {
            rect: Rect {
                x: x + width - t,
                y,
                width: t,
                height,
            },
            color: HIGHLIGHT_COLOR,
        },
        PaintCommand::BorderEdge {
            rect: Rect {
                x,
                y: y + height - t,
                width,
                height: t,
            },
            color: HIGHLIGHT_COLOR,
        },
        PaintCommand::BorderEdge {
            rect: Rect {
                x,
                y,
                width: t,
                height,
            },
            color: HIGHLIGHT_COLOR,
        },
    ]
}

pub(super) fn hit_test(fragment: &Fragment, x: f64, y: f64) -> Option<NodeId> {
    hit_test_rec(fragment, x, y, 0.0, 0.0)
}

pub(super) fn hit_test_rec(
    fragment: &Fragment,
    x: f64,
    y: f64,
    offset_x: f64,
    offset_y: f64,
) -> Option<NodeId> {
    let fx = offset_x + fragment.x;
    let fy = offset_y + fragment.y;
    if x < fx || y < fy || x > fx + fragment.width || y > fy + fragment.height {
        return None;
    }
    for child in &fragment.children {
        if let Some(hit) = hit_test_rec(child, x, y, fx, fy) {
            return Some(hit);
        }
    }
    fragment.node
}

pub(super) fn nearest_link_href(doc: &Document, mut node: NodeId) -> Option<String> {
    loop {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name == "a" {
                if let Some((_, href)) = attributes.iter().find(|(k, _)| k == "href") {
                    return Some(href.clone());
                }
            }
        }
        node = doc.parent(node)?;
    }
}

pub(super) fn nearest_supported_text_input(doc: &Document, mut node: NodeId) -> Option<NodeId> {
    loop {
        if is_supported_text_input(doc, node) {
            return Some(node);
        }
        node = doc.parent(node)?;
    }
}

pub(super) fn is_supported_text_input(doc: &Document, id: NodeId) -> bool {
    matches!(
        doc.data(id),
        NodeData::Element {
            tag_name,
            attributes,
        } if tag_name.eq_ignore_ascii_case("input")
            && !attributes
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
            && attributes
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("type"))
                .is_none_or(|(_, input_type)| input_type.eq_ignore_ascii_case("text"))
    )
}

/// The HTML for `url`, for the small set of `about:` URLs `navigate`
/// serves locally instead of fetching over the network -- `None` for
/// any other URL (including unrecognized `about:` ones, which aren't
/// treated as built-in pages here). Owned `String`, not `&'static
/// str`: the credits page is generated per request from `blueice-i18n`
/// at whatever locale the URL's `?lang=` parameter asks for
/// (`credits::locale_from_url`), not a single fixed literal.
pub(crate) fn built_in_page(url: &str) -> Option<String> {
    if url == "about:blank" {
        return Some(String::new());
    }
    if url == crate::credits::CREDITS_URL
        || url.starts_with(&format!("{}?", crate::credits::CREDITS_URL))
    {
        return Some(crate::credits::credits_html(
            crate::credits::locale_from_url(url),
        ));
    }
    None
}
