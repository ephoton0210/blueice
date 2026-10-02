// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native control runs preserve whitespace and carry core-source UTF-16
//! ranges. Password fragments contain masks rather than original contents.

use crate::{Fragment, FragmentKind};
use blueice_dom::{Document, NodeData, NodeId};
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct ControlText {
    value: String,
    multiline: bool,
    protected: bool,
}

pub(super) fn control_text(doc: &Document, node: NodeId) -> Option<ControlText> {
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    else {
        return None;
    };
    if tag_name == "textarea" {
        fn collect(doc: &Document, id: NodeId, out: &mut String) {
            if let NodeData::Text { data } = doc.data(id) {
                out.push_str(data);
            }
            for child in doc.children(id) {
                collect(doc, child, out);
            }
        }
        let mut value = String::new();
        collect(doc, node, &mut value);
        return Some(ControlText {
            value,
            multiline: true,
            protected: false,
        });
    }
    if tag_name != "input" {
        return None;
    }
    let kind = attributes
        .iter()
        .find(|(name, _)| name == "type")
        .map(|(_, value)| value.to_ascii_lowercase())
        .unwrap_or_else(|| "text".into());
    if !matches!(
        kind.as_str(),
        "text" | "password" | "search" | "email" | "url" | "tel"
    ) {
        return None;
    }
    let value = attributes
        .iter()
        .find(|(name, _)| name == "value")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    Some(ControlText {
        value,
        multiline: false,
        protected: kind == "password",
    })
}

pub(super) fn lines(
    value: &ControlText,
    node: NodeId,
    width: f64,
    font: f64,
    line_height: f64,
    bold: bool,
    italic: bool,
) -> Vec<Fragment> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut measured = 0.0;
    let mut offset = 0_u32;
    let mut start = 0_u32;
    let emit = |result: &mut Vec<Fragment>, text: String, measured: f64, start: u32, end: u32| {
        result.push(Fragment {
            node: Some(node),
            kind: FragmentKind::NativeText {
                text,
                source_start: start,
                source_end: end,
            },
            x: 0.0,
            y: result.len() as f64 * line_height,
            width: measured,
            height: line_height,
            children: Vec::new(),
        });
    };
    for cluster in value.value.graphemes(true) {
        let length = cluster.encode_utf16().count() as u32;
        if cluster.contains(['\r', '\n']) {
            if value.multiline {
                emit(
                    &mut result,
                    std::mem::take(&mut text),
                    measured,
                    start,
                    offset,
                );
                measured = 0.0;
                start = offset + length;
            }
            offset += length;
            continue;
        }
        let shown = if value.protected { "•" } else { cluster };
        let advance = blueice_font::measure_text_width(shown, font, bold, italic);
        if value.multiline && !text.is_empty() && measured + advance > width {
            emit(
                &mut result,
                std::mem::take(&mut text),
                measured,
                start,
                offset,
            );
            measured = 0.0;
            start = offset;
        }
        text.push_str(shown);
        measured += advance;
        offset += length;
    }
    emit(&mut result, text, measured, start, offset);
    result
}
