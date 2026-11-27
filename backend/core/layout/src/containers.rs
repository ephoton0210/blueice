// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The content-box sizes of a page's query containers (`container-type`), read off a laid-out
//! fragment tree, for the cascade's next pass: `@container` rules and `cq*` units resolve
//! against exactly these.

use crate::fragment::Fragment;
use blueice_css::{ComputedStyle, LengthContext, Value};
use blueice_dom::NodeId;
use std::collections::HashMap;

/// The (width, height) of each container's content box, by element. A container that
/// generated no box (`display: none` or `contents`) has no entry.
pub fn container_content_sizes(
    styles: &HashMap<NodeId, ComputedStyle>,
    fragment: &Fragment,
    viewport_width: f64,
    viewport_height: f64,
) -> HashMap<NodeId, (f64, f64)> {
    let mut sizes = HashMap::new();
    let root_font_size = fragment
        .node
        .and_then(|n| styles.get(&n))
        .map_or(16.0, |s| s.font_size_px);
    visit(
        styles,
        fragment,
        fragment.width,
        (viewport_width, viewport_height, root_font_size),
        &mut sizes,
    );
    sizes
}

fn visit(
    styles: &HashMap<NodeId, ComputedStyle>,
    fragment: &Fragment,
    parent_width: f64,
    environment: (f64, f64, f64),
    sizes: &mut HashMap<NodeId, (f64, f64)>,
) {
    if let Some(node) = fragment.node {
        if let Some(style) = styles.get(&node) {
            let is_container = matches!(
                style.other.get("container-type"),
                Some(Value::Keyword(k)) if k == "size" || k == "inline-size"
            );
            if is_container && !sizes.contains_key(&node) {
                let context = LengthContext {
                    font_size: style.font_size_px,
                    root_font_size: environment.2,
                    viewport_width: environment.0,
                    viewport_height: environment.1,
                    percent_base: parent_width,
                };
                let side = |name: &str| {
                    style
                        .other
                        .get(name)
                        .and_then(|v| v.to_px(&context))
                        .unwrap_or(0.0)
                        .max(0.0)
                };
                let border = |name: &str| {
                    let styled = match style.other.get(&format!("border-{name}-style")) {
                        Some(Value::Keyword(k)) => !matches!(k.as_str(), "none" | "hidden"),
                        _ => true,
                    };
                    if styled {
                        side(&format!("border-{name}-width"))
                    } else {
                        0.0
                    }
                };
                let horizontal = side("padding-left")
                    + side("padding-right")
                    + border("left")
                    + border("right");
                let vertical = side("padding-top")
                    + side("padding-bottom")
                    + border("top")
                    + border("bottom");
                sizes.insert(
                    node,
                    (
                        (fragment.width - horizontal).max(0.0),
                        (fragment.height - vertical).max(0.0),
                    ),
                );
            }
        }
    }
    for child in &fragment.children {
        visit(styles, child, fragment.width, environment, sizes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout, Constraints};
    use blueice_css::{cascade_with_viewport, ua_stylesheet, Origin};

    fn sizes_of(html: &str) -> (blueice_dom::Document, HashMap<NodeId, (f64, f64)>) {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let styles = cascade_with_viewport(&doc, &[(Origin::Ua, &ua)], 800.0, 600.0);
        let fragment = layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: 800.0,
                viewport_height: 600.0,
            },
        );
        let sizes = container_content_sizes(&styles, &fragment, 800.0, 600.0);
        (doc, sizes)
    }

    fn element(doc: &blueice_dom::Document, id: &str) -> NodeId {
        let mut stack = vec![doc.root()];
        while let Some(n) = stack.pop() {
            if let blueice_dom::NodeData::Element { attributes, .. } = doc.data(n) {
                if attributes.iter().any(|(k, v)| k == "id" && v == id) {
                    return n;
                }
            }
            stack.extend(doc.children(n));
        }
        panic!("no #{id}");
    }

    #[test]
    fn a_containers_size_is_its_content_box() {
        let (doc, sizes) = sizes_of(
            "<div id=c style=\"container-type:size;width:300px;height:100px;padding:10px;border:2px solid;box-sizing:content-box\"></div>",
        );
        assert_eq!(sizes[&element(&doc, "c")], (300.0, 100.0));
    }

    #[test]
    fn a_border_box_container_gives_up_its_padding_and_border() {
        let (doc, sizes) = sizes_of(
            "<div id=c style=\"container-type:inline-size;width:300px;height:100px;padding:10px;border:2px solid;box-sizing:border-box\"></div>",
        );
        assert_eq!(sizes[&element(&doc, "c")], (276.0, 76.0));
    }

    #[test]
    fn only_containers_are_reported_and_a_hidden_one_has_no_entry() {
        let (doc, sizes) = sizes_of(
            "<div id=a style=\"width:50px\"></div><div id=b style=\"display:none;container-type:size\"></div>",
        );
        assert!(sizes.is_empty(), "{sizes:?} {:?}", element(&doc, "a"));
    }
}
