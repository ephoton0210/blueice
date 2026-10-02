// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Page {
    /// Sets the value of a real, supported native text input for the
    /// extension protocol's versioned `dom:write` operation. Unlike the
    /// broader first-party [`NodeAction::SetValue`] compatibility action,
    /// this external boundary verifies that the target is a live `<input>`
    /// with no `type` or `type=text`; an extension cannot use an AI node ID to
    /// smuggle a value attribute onto arbitrary document content or a
    /// sensitive input type.
    pub(crate) fn set_text_input_value(&mut self, id: NodeId, value: String) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown text input node {}", id.as_u64()));
        }
        if !is_supported_text_input(&self.doc, id) {
            return Err(format!(
                "node {} is not a supported text input",
                id.as_u64()
            ));
        }
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            unreachable!("a checked input node remains an element");
        };
        match attributes
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case("value"))
        {
            Some((_, existing)) => *existing = value,
            None => attributes.push(("value".to_string(), value)),
        }
        self.relayout();
        Ok(())
    }

    /// Appends user-entered text to the focused supported input. The input
    /// target is selected solely by [`Self::focus_text_input_at`], rather than
    /// supplied by a frontend or extension, so this remains a keyboard path
    /// rather than a general DOM mutation capability.
    pub(crate) fn insert_focused_text(&mut self, text: &str) -> bool {
        let Some(id) = self
            .focused
            .filter(|id| is_supported_text_input(&self.doc, *id))
        else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let mut value = element_attribute(&self.doc, id, "value")
            .unwrap_or_default()
            .to_string();
        value.push_str(text);
        self.set_text_input_value(id, value)
            .expect("focused supported text input remains writable");
        true
    }

    /// Removes one Unicode scalar from the focused supported input.
    pub(crate) fn delete_focused_text_backward(&mut self) -> bool {
        let Some(id) = self
            .focused
            .filter(|id| is_supported_text_input(&self.doc, *id))
        else {
            return false;
        };
        let mut value = element_attribute(&self.doc, id, "value")
            .unwrap_or_default()
            .to_string();
        if value.pop().is_none() {
            return false;
        }
        self.set_text_input_value(id, value)
            .expect("focused supported text input remains writable");
        true
    }

    /// Sets the text content of a real, enabled native textarea for the
    /// extension protocol's version-4 `dom:write` operation. This is a
    /// distinct bounded operation from text-input attributes: a textarea's
    /// value is represented by its child text, and extensions cannot use it
    /// to replace arbitrary element content.
    pub(crate) fn set_textarea_value(&mut self, id: NodeId, value: String) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown textarea node {}", id.as_u64()));
        }
        let is_supported_textarea = matches!(
            self.doc.data(id),
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("textarea")
                && !attributes
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
        );
        if !is_supported_textarea {
            return Err(format!(
                "node {} is not an enabled native textarea",
                id.as_u64()
            ));
        }
        self.set_node_text_content(id, value)
    }

    /// Narrow v8 extension mutation: only text inside an ordinary rendered
    /// semantic leaf. Keeping the existing text node avoids removing links,
    /// controls, event targets, or arbitrary subtrees through textContent.
    pub(crate) fn set_visible_leaf_text(
        &mut self,
        id: NodeId,
        value: String,
    ) -> Result<(), String> {
        self.validate_visible_text_write(id, &value)?;
        let children = self.doc.children(id).collect::<Vec<_>>();
        let [text_id] = children.as_slice() else {
            return Err("the target must have exactly one text child".to_string());
        };
        if !matches!(self.doc.data(*text_id), NodeData::Text { .. }) {
            return Err("the target contains nested content".to_string());
        }
        if let NodeData::Text { data } = self.doc.data_mut(*text_id) {
            *data = value;
        }
        self.restyle_and_relayout();
        Ok(())
    }

    /// Version 9 deliberately expands textContent-style edits to ordinary
    /// inline formatting inside a rendered heading, paragraph, or list item.
    /// Validation finishes before any descendant is removed. A link, form
    /// control, semantic child, element with an ID or inline event handler,
    /// or hidden/interactive annotation cannot be deleted through this API.
    pub(crate) fn set_visible_text_content(
        &mut self,
        id: NodeId,
        value: String,
    ) -> Result<(), String> {
        self.validate_visible_text_write(id, &value)?;
        let mut pending = self.doc.children(id).collect::<Vec<_>>();
        let mut inspected = 0;
        while let Some(node) = pending.pop() {
            inspected += 1;
            if inspected > 128 {
                return Err("visible text content has too many descendants".to_string());
            }
            match self.doc.data(node) {
                NodeData::Text { .. } => {}
                NodeData::Element {
                    tag_name,
                    attributes,
                } if matches!(
                    tag_name.as_str(),
                    "span"
                        | "strong"
                        | "em"
                        | "b"
                        | "i"
                        | "small"
                        | "code"
                        | "mark"
                        | "u"
                        | "s"
                        | "br"
                ) && !attributes.iter().any(|(name, _)| {
                    let name = name.to_ascii_lowercase();
                    matches!(
                        name.as_str(),
                        "id" | "role" | "tabindex" | "contenteditable" | "hidden" | "aria-hidden"
                    ) || name.starts_with("on")
                }) && !self.styles.get(&node).is_some_and(|style| {
                    style.display.eq_ignore_ascii_case("none") || style.opacity() <= 0.0
                }) =>
                {
                    pending.extend(self.doc.children(node))
                }
                _ => {
                    return Err(
                        "visible text content may replace only ordinary inline formatting"
                            .to_string(),
                    )
                }
            }
        }
        self.set_node_text_content(id, value)
    }

    fn validate_visible_text_write(&self, id: NodeId, value: &str) -> Result<(), String> {
        if value.len() > blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES {
            return Err("visible text exceeds the protocol limit".to_string());
        }
        if value.trim().is_empty() {
            return Err("visible text cannot be empty".to_string());
        }
        if value
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return Err("visible text contains a control character".to_string());
        }
        if !self
            .url
            .as_deref()
            .is_some_and(|url| url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err("extension text writes require an ordinary web page".to_string());
        }
        if !self.doc.contains(id) {
            return Err(format!("unknown visible text node {}", id.as_u64()));
        }
        let NodeData::Element { tag_name, .. } = self.doc.data(id) else {
            return Err("the target must be a semantic element".to_string());
        };
        if !matches!(
            tag_name.as_str(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "li"
        ) {
            return Err("the target is not a supported semantic text leaf".to_string());
        }
        if element_attribute(&self.doc, id, "aria-label").is_some()
            || element_attribute(&self.doc, id, "title").is_some()
        {
            return Err("the target has a separate accessible name".to_string());
        }
        let mut ancestor = Some(id);
        while let Some(node) = ancestor {
            if element_attribute(&self.doc, node, "hidden").is_some()
                || element_attribute(&self.doc, node, "aria-hidden")
                    .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
                || self
                    .styles
                    .get(&node)
                    .is_some_and(|style| style.opacity() <= 0.0)
            {
                return Err("the target is hidden".to_string());
            }
            ancestor = self.doc.parent(node);
        }
        if !find_fragment_bounds(&self.fragment, id, 0.0, 0.0)
            .is_some_and(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
        {
            return Err("the target is not rendered".to_string());
        }
        Ok(())
    }

    /// Sets an integer value on one real, enabled native range input for the
    /// extension protocol's version-7 `dom:write` operation. This is not a
    /// generic numeric attribute setter: core owns the live `min`, `max`, and
    /// `step` checks before it changes the one `value` attribute. The first
    /// increment deliberately accepts only integer range constraints (or the
    /// native 0..=100/step-1 defaults); decimal and `step=any` controls need a
    /// later, separately specified numeric representation.
    pub(crate) fn set_range_input_value(&mut self, id: NodeId, value: i64) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown range input node {}", id.as_u64()));
        }
        let (min, max, step) = match self.doc.data(id) {
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("input")
                && attributes.iter().any(|(name, input_type)| {
                    name.eq_ignore_ascii_case("type") && input_type.eq_ignore_ascii_case("range")
                })
                && !attributes
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("disabled")) =>
            {
                let integer_attribute = |name: &str, default: i64| {
                    attributes
                        .iter()
                        .find(|(attribute, _)| attribute.eq_ignore_ascii_case(name))
                        .map_or(Ok(default), |(_, raw)| raw.parse::<i64>().map_err(|_| ()))
                };
                let min = integer_attribute("min", 0).map_err(|_| {
                    format!("range input node {} has a non-integer min", id.as_u64())
                })?;
                let max = integer_attribute("max", 100).map_err(|_| {
                    format!("range input node {} has a non-integer max", id.as_u64())
                })?;
                let step = integer_attribute("step", 1).map_err(|_| {
                    format!("range input node {} has a non-integer step", id.as_u64())
                })?;
                (min, max, step)
            }
            _ => {
                return Err(format!(
                    "node {} is not an enabled integer range input",
                    id.as_u64()
                ));
            }
        };
        if min > max {
            return Err(format!(
                "range input node {} has min above max",
                id.as_u64()
            ));
        }
        if step <= 0 {
            return Err(format!(
                "range input node {} has a non-positive step",
                id.as_u64()
            ));
        }
        // Do the offset arithmetic in i128: a valid i64 range can span
        // across zero, making `value - min` overflow even though both values
        // are individually valid protocol integers.
        let step_aligned = (i128::from(value) - i128::from(min)) % i128::from(step) == 0;
        if !(min..=max).contains(&value) || !step_aligned {
            return Err(format!(
                "value {value} is outside the integer range constraints for node {}",
                id.as_u64()
            ));
        }
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            unreachable!("a checked range input remains an element");
        };
        match attributes
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case("value"))
        {
            Some((_, existing)) => *existing = value.to_string(),
            None => attributes.push(("value".to_string(), value.to_string())),
        }
        self.relayout();
        Ok(())
    }

    /// Sets the checked state of a real, enabled native checkbox for the
    /// extension protocol's version-3 `dom:write` operation. This remains a
    /// bounded semantic operation rather than a generic attribute setter:
    /// radios have group semantics that need their own protocol design, and a
    /// disabled control must remain unavailable to an extension write.
    pub(crate) fn set_checkbox_checked(&mut self, id: NodeId, checked: bool) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown checkbox node {}", id.as_u64()));
        }
        let is_supported_checkbox = matches!(
            self.doc.data(id),
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("input")
                && attributes.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("type") && value.eq_ignore_ascii_case("checkbox")
                })
                && !attributes.iter().any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
        );
        if !is_supported_checkbox {
            return Err(format!(
                "node {} is not an enabled native checkbox",
                id.as_u64()
            ));
        }
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            unreachable!("a checked checkbox node remains an element");
        };
        if checked {
            if !attributes
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("checked"))
            {
                attributes.push(("checked".to_string(), String::new()));
            }
        } else {
            attributes.retain(|(name, _)| !name.eq_ignore_ascii_case("checked"));
        }
        self.relayout();
        Ok(())
    }

    /// Selects one real, enabled native radio input for the extension
    /// protocol's version-5 `dom:write` operation. A radio is deliberately
    /// not treated as a checkbox: core derives its local group from the live
    /// document and clears the group's other members atomically. To avoid
    /// silently approximating the HTML form-owner algorithm, this first
    /// operation accepts only named radios with an ordinary ancestor form (or
    /// no form) and rejects externally form-associated controls.
    pub(crate) fn set_radio_checked(&mut self, id: NodeId) -> Result<(), String> {
        let (name, form_owner) = extension_radio_group_scope(&self.doc, id)?;
        let mut members = Vec::new();
        collect_extension_radio_group_members(
            &self.doc,
            self.doc.root(),
            &name,
            form_owner,
            &mut members,
        );
        if !members.contains(&id) {
            return Err("the live radio group could not be resolved".to_string());
        }

        for member in members {
            let NodeData::Element { attributes, .. } = self.doc.data_mut(member) else {
                unreachable!("a collected radio group member remains an element");
            };
            if member == id {
                if !attributes
                    .iter()
                    .any(|(attribute, _)| attribute.eq_ignore_ascii_case("checked"))
                {
                    attributes.push(("checked".to_string(), String::new()));
                }
            } else {
                attributes.retain(|(attribute, _)| !attribute.eq_ignore_ascii_case("checked"));
            }
        }
        self.relayout();
        Ok(())
    }

    /// Selects one real, enabled `<option>` for the extension protocol's
    /// version-6 `dom:write` operation. Like radio selection, the extension
    /// carries only a stable node ID: core derives the owning live `<select>`
    /// and performs the whole single-select transition atomically. The first
    /// slice intentionally does not approximate multiple-select or disabled
    /// option-group semantics, so those controls are rejected rather than
    /// partially changed.
    pub(crate) fn select_option(&mut self, id: NodeId) -> Result<(), String> {
        let select = extension_select_option_owner(&self.doc, id)?;
        let mut options = Vec::new();
        collect_extension_select_options(&self.doc, select, &mut options);
        if !options.contains(&id) {
            return Err("the live select options could not be resolved".to_string());
        }

        for option in options {
            let NodeData::Element { attributes, .. } = self.doc.data_mut(option) else {
                unreachable!("a collected select option remains an element");
            };
            if option == id {
                if !attributes
                    .iter()
                    .any(|(attribute, _)| attribute.eq_ignore_ascii_case("selected"))
                {
                    attributes.push(("selected".to_string(), String::new()));
                }
            } else {
                attributes.retain(|(attribute, _)| !attribute.eq_ignore_ascii_case("selected"));
            }
        }
        self.relayout();
        Ok(())
    }
}
