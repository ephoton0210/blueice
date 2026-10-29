// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_dom::{Document, NodeData, NodeId};

fn element(doc: &Document, from: NodeId, tag: &str) -> Option<NodeId> {
    if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
        return Some(from);
    }
    doc.children(from).find_map(|c| element(doc, c, tag))
}

fn tags(doc: &Document, node: NodeId) -> Vec<String> {
    doc.children(node)
        .filter_map(|c| match doc.data(c) {
            NodeData::Element { tag_name, .. } => Some(tag_name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn self_closing_svg_children_are_siblings_not_nested() {
    let doc = blueice_html::parse(
        "<svg viewBox='0 0 10 10'><path d='M0 0'/><circle r='1'/><g><rect width='1'/></g></svg><p>x</p>",
    );
    let svg = element(&doc, doc.root(), "svg").unwrap();
    assert_eq!(tags(&doc, svg), ["path", "circle", "g"]);
    let g = element(&doc, svg, "g").unwrap();
    assert_eq!(tags(&doc, g), ["rect"]);
    let body = element(&doc, doc.root(), "body").unwrap();
    assert_eq!(tags(&doc, body), ["svg", "p"], "the paragraph follows the svg");
}

#[test]
fn camel_case_svg_names_survive() {
    let doc = blueice_html::parse(
        "<svg viewBox='0 0 4 4' preserveAspectRatio='none'><linearGradient gradientUnits='userSpaceOnUse'/><clipPath/></svg>",
    );
    let svg = element(&doc, doc.root(), "svg").unwrap();
    let NodeData::Element { attributes, .. } = doc.data(svg) else {
        unreachable!()
    };
    assert!(attributes.iter().any(|(k, _)| k == "viewBox"));
    assert!(attributes.iter().any(|(k, _)| k == "preserveAspectRatio"));
    assert_eq!(tags(&doc, svg), ["linearGradient", "clipPath"]);
    let gradient = element(&doc, svg, "linearGradient").unwrap();
    let NodeData::Element { attributes, .. } = doc.data(gradient) else {
        unreachable!()
    };
    assert!(attributes.iter().any(|(k, _)| k == "gradientUnits"));
}

#[test]
fn html_after_an_svg_is_ordinary_html_again() {
    let doc = blueice_html::parse("<svg><image/><style>a{}</style></svg><p>x</p><b>y</b>");
    let body = element(&doc, doc.root(), "body").unwrap();
    assert_eq!(tags(&doc, body), ["svg", "p", "b"]);
    let svg = element(&doc, body, "svg").unwrap();
    assert_eq!(tags(&doc, svg), ["image", "style"]);
}
