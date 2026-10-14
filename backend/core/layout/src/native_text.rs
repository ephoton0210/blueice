// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native control runs preserve whitespace and carry core-source UTF-16
//! ranges. Password fragments contain masks rather than original contents.

use crate::{Fragment, FragmentKind};
use blueice_dom::{Document, NodeData, NodeId};
use unicode_segmentation::UnicodeSegmentation;

/// The same input-button caption is used by layout/paint and the core
/// semantic tree. An explicit empty value suppresses the default caption.
pub fn input_button_label(doc: &Document, node: NodeId) -> Option<String> {
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    else {
        return None;
    };
    if tag_name != "input" {
        return None;
    }
    let attribute = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    let default = match attribute("type")
        .unwrap_or("text")
        .to_ascii_lowercase()
        .as_str()
    {
        "file" => {
            let choose = if attribute("multiple").is_some() {
                "Choose Files"
            } else {
                "Choose File"
            };
            let names = doc.file_control_names(node);
            return Some(format!(
                "{choose} — {}",
                if names.is_empty() {
                    "No file selected".into()
                } else {
                    names.join(", ")
                }
            ));
        }
        "reset" => "Reset",
        "submit" => "Submit",
        "button" => "",
        _ => return None,
    };
    Some(attribute("value").unwrap_or(default).into())
}

/// Finite, clamped range values shared by pixels, semantics and form submission.
/// This exposes the existing control model; complete HTML step sanitization is
/// a separate constraint-validation increment.
pub fn input_range_values(doc: &Document, node: NodeId) -> Option<(f64, f64, f64)> {
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    else {
        return None;
    };
    let attribute = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    if tag_name != "input"
        || !attribute("type").is_some_and(|kind| kind.eq_ignore_ascii_case("range"))
    {
        return None;
    }
    let number = |name, default| {
        attribute(name)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    };
    let min = number("min", 0.0);
    let max = number("max", 100.0).max(min);
    let value = number("value", min / 2.0 + max / 2.0).clamp(min, max);
    Some((min, max, value))
}

pub(super) fn form_control(doc: &Document, node: NodeId) -> Option<crate::NativeForm> {
    use crate::NativeForm;
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    else {
        return None;
    };
    let attribute = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    if tag_name == "select" {
        let options = select_options(doc, node);
        if select_display_size(doc, node) > 1 || attribute("multiple").is_some() {
            return Some(NativeForm::SelectList {
                options,
                first: 0,
                row_height: 20.0,
                active: None,
            });
        }
        return Some(NativeForm::Select(
            options
                .iter()
                .find(|option| option.selected)
                .map(|option| option.label.clone())
                .unwrap_or_default(),
        ));
    }
    if tag_name != "input" {
        return None;
    }
    if let Some(label) = input_button_label(doc, node) {
        return Some(NativeForm::Button(label));
    }
    match attribute("type")
        .unwrap_or("text")
        .to_ascii_lowercase()
        .as_str()
    {
        "checkbox" => Some(NativeForm::CheckBox(attribute("checked").is_some())),
        "radio" => Some(NativeForm::Radio(attribute("checked").is_some())),
        "range" => input_range_values(doc, node).map(|(min, max, value)| NativeForm::Range {
            min,
            max,
            value,
        }),
        _ => None,
    }
}

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

/// Option identities, labels and selectedness shared by layout and interaction.
/// Descendants of another select do not belong to this control.
pub fn select_options(doc: &Document, select: NodeId) -> Vec<crate::SelectOption> {
    fn attr<'a>(doc: &'a Document, id: NodeId, key: &str) -> Option<&'a str> {
        match doc.data(id) {
            NodeData::Element { attributes, .. } => attributes
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str()),
            _ => None,
        }
    }
    fn text(doc: &Document, id: NodeId, out: &mut String) {
        if let NodeData::Text { data } = doc.data(id) {
            out.push_str(data);
        }
        for child in doc.children(id) {
            text(doc, child, out);
        }
    }
    fn collect(
        doc: &Document,
        id: NodeId,
        disabled: bool,
        group: Option<String>,
        out: &mut Vec<crate::SelectOption>,
    ) {
        let NodeData::Element { tag_name, .. } = doc.data(id) else {
            return;
        };
        if tag_name == "select" {
            return;
        }
        let disabled = disabled || attr(doc, id, "disabled").is_some();
        let group = if tag_name == "optgroup" {
            attr(doc, id, "label").map(str::to_string)
        } else {
            group
        };
        if tag_name == "option" {
            let mut label = String::new();
            text(doc, id, &mut label);
            let label = attr(doc, id, "label")
                .filter(|label| !label.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| label.split_whitespace().collect::<Vec<_>>().join(" "));
            out.push(crate::SelectOption {
                node: id,
                label,
                group,
                disabled,
                selected: attr(doc, id, "selected").is_some(),
            });
            return;
        }
        for child in doc.children(id) {
            collect(doc, child, disabled, group.clone(), out);
        }
    }
    let mut options = Vec::new();
    for child in doc.children(select) {
        collect(doc, child, false, None, &mut options);
    }
    if attr(doc, select, "multiple").is_none() {
        let chosen = options
            .iter()
            .rposition(|option| option.selected)
            .or_else(|| {
                (select_display_size(doc, select) == 1)
                    .then(|| options.iter().position(|option| !option.disabled))
                    .flatten()
            });
        for (index, option) in options.iter_mut().enumerate() {
            option.selected = chosen == Some(index);
        }
    }
    options
}

pub fn select_display_size(doc: &Document, node: NodeId) -> u32 {
    let NodeData::Element { attributes, .. } = doc.data(node) else {
        return 1;
    };
    attributes
        .iter()
        .find(|(key, _)| key == "size")
        .and_then(|(_, value)| value.trim().parse::<u32>().ok())
        .filter(|size| *size > 0)
        .unwrap_or_else(|| {
            if attributes.iter().any(|(key, _)| key == "multiple") {
                4
            } else {
                1
            }
        })
}
