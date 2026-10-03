// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native form defaults belong to the document owner, like live values. A
//! reset is local and cannot produce a network request or grant a capability.

use super::native_interaction::{form_owner, input_type, tag};
use super::*;

#[derive(Clone)]
pub(super) enum ControlDefault {
    Input {
        value: Option<String>,
        checked: bool,
    },
    Textarea(String),
    Option {
        selected: bool,
    },
}

impl Page {
    /// Capture defaults before the first live native mutation, including
    /// controls appended through the existing page-script DOM boundary. Keep
    /// removed nodes out of the cache; document replacement clears it.
    pub(super) fn remember_native_form_defaults(&mut self) {
        self.native_form_defaults
            .retain(|node, _| self.doc.contains(*node));
        let mut pending = vec![self.doc.root()];
        while let Some(node) = pending.pop() {
            pending.extend(self.doc.children(node));
            if self.native_form_defaults.contains_key(&node) {
                continue;
            }
            let default = match tag(&self.doc, node) {
                "input" => ControlDefault::Input {
                    // A value attribute never selects a file or restores a path.
                    value: (input_type(&self.doc, node) != "file")
                        .then(|| element_attribute(&self.doc, node, "value").map(str::to_string))
                        .flatten(),
                    checked: element_attribute(&self.doc, node, "checked").is_some(),
                },
                "textarea" => ControlDefault::Textarea(node_text_content(&self.doc, node)),
                "option" => ControlDefault::Option {
                    selected: element_attribute(&self.doc, node, "selected").is_some(),
                },
                _ => continue,
            };
            self.native_form_defaults.insert(node, default);
        }
    }

    pub(super) fn reset_native_form(&mut self, form: NodeId) {
        self.native_file_revision = self.native_file_revision.wrapping_add(1);
        let mut nodes = Vec::new();
        let mut pending = vec![self.doc.root()];
        while let Some(node) = pending.pop() {
            pending.extend(
                self.doc
                    .children(node)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            );
            if matches!(tag(&self.doc, node), "input" | "textarea" | "select")
                && form_owner(&self.doc, node) == Some(form)
            {
                nodes.push(node);
            }
        }
        for node in nodes {
            if tag(&self.doc, node) == "select" {
                for option in self.native_options(node) {
                    self.restore_native_form_default(option);
                }
            } else {
                self.restore_native_form_default(node);
            }
        }
        // Retain focus on the reset button, but invalidate selection, marked
        // text and queued edits made before reset. CancelComposition must not
        // be able to bring a pre-reset value back into this document.
        self.native_editor = None;
        self.native_focus_generation = self.native_focus_generation.wrapping_add(1);
        self.native_focus_exit = None;
        self.restyle_and_relayout();
    }

    /// A page-script textContent/appendChild change to textarea markup is an
    /// explicit default change. Native editing and extension value setters
    /// deliberately do not call this hook.
    pub(super) fn native_form_script_text_change(&mut self, node: NodeId) {
        self.native_form_defaults
            .retain(|node, _| self.doc.contains(*node));
        let mut ancestor = Some(node);
        while let Some(node) = ancestor {
            if tag(&self.doc, node) == "textarea" {
                self.native_form_defaults.insert(
                    node,
                    ControlDefault::Textarea(node_text_content(&self.doc, node)),
                );
                break;
            }
            ancestor = self.doc.parent(node);
        }
    }

    fn restore_native_form_default(&mut self, node: NodeId) {
        if input_type(&self.doc, node) == "file" {
            self.native_files.remove(&node);
            self.doc.set_file_control_names(node, vec![]);
        }
        let Some(default) = self.native_form_defaults.get(&node).cloned() else {
            return;
        };
        match default {
            ControlDefault::Input { value, checked } => {
                let NodeData::Element { attributes, .. } = self.doc.data_mut(node) else {
                    return;
                };
                attributes.retain(|(name, _)| !name.eq_ignore_ascii_case("value"));
                if let Some(value) = value {
                    attributes.push(("value".into(), value));
                }
                self.native_set_boolean(node, "checked", checked);
            }
            ControlDefault::Textarea(value) => {
                for child in self.doc.children(node).collect::<Vec<_>>() {
                    self.doc.remove_subtree(child);
                }
                if !value.is_empty() {
                    let text = self.doc.create_node(NodeData::Text { data: value });
                    self.doc.append_child(node, text);
                }
            }
            ControlDefault::Option { selected } => {
                self.native_set_boolean(node, "selected", selected)
            }
        }
    }
}
