// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native focus and control defaults use the current document, never a
//! frontend-selected node or an extension DOM-write capability.

use super::*;
use blueice_ipc::input::{
    FocusDirection, PageKey, TextInputAction, TextInputContext, TextMovement,
};

pub(super) fn tag(doc: &Document, id: NodeId) -> &str {
    match doc.data(id) {
        NodeData::Element { tag_name, .. } => tag_name,
        _ => "",
    }
}

fn has(doc: &Document, id: NodeId, name: &str) -> bool {
    element_attribute(doc, id, name).is_some()
}

pub(super) fn input_type(doc: &Document, id: NodeId) -> String {
    element_attribute(doc, id, "type")
        .unwrap_or("text")
        .to_ascii_lowercase()
}

fn descendants(doc: &Document, root: NodeId, nodes: &mut Vec<NodeId>) {
    nodes.push(root);
    for child in doc.children(root) {
        descendants(doc, child, nodes);
    }
}

fn inside(doc: &Document, id: NodeId, ancestor: NodeId) -> bool {
    let mut node = Some(id);
    while let Some(id) = node {
        if id == ancestor {
            return true;
        }
        node = doc.parent(id);
    }
    false
}

/// The form attribute resolves against the first matching ID, including when
/// that match is not a form. Unresolved explicit owners do not fall back.
pub(super) fn form_owner(doc: &Document, id: NodeId) -> Option<NodeId> {
    if let Some(name) = element_attribute(doc, id, "form") {
        let owner = find_element_by_id(doc, doc.root(), name)?;
        return (tag(doc, owner) == "form").then_some(owner);
    }
    nearest_form_ancestor(doc, id)
}

impl Page {
    pub(crate) fn native_activation_link(&self, node: NodeId) -> Option<String> {
        nearest_link_href(&self.doc, node).map(|href| self.resolve_link_href(href))
    }
    pub(crate) fn native_control_disabled(&self, id: NodeId) -> bool {
        if !self.doc.contains(id) {
            return true;
        }
        if !matches!(
            tag(&self.doc, id),
            "input" | "button" | "select" | "textarea" | "fieldset" | "option" | "optgroup"
        ) {
            return false;
        }
        if has(&self.doc, id, "disabled") {
            return true;
        }
        let mut ancestor = self.doc.parent(id);
        while let Some(parent) = ancestor {
            if tag(&self.doc, parent) == "fieldset" && has(&self.doc, parent, "disabled") {
                let legend = self
                    .doc
                    .children(parent)
                    .find(|child| tag(&self.doc, *child) == "legend");
                if !legend.is_some_and(|legend| inside(&self.doc, id, legend)) {
                    return true;
                }
            }
            if matches!(tag(&self.doc, parent), "select" | "optgroup")
                && has(&self.doc, parent, "disabled")
            {
                return true;
            }
            ancestor = self.doc.parent(parent);
        }
        false
    }

    pub(crate) fn native_focusable(&self, id: NodeId) -> bool {
        if !self.doc.contains(id) || self.native_control_disabled(id) {
            return false;
        }
        let kind = tag(&self.doc, id);
        if kind == "input" && input_type(&self.doc, id) == "hidden" {
            return false;
        }
        let default = matches!(kind, "input" | "button" | "select" | "textarea")
            || kind == "a" && has(&self.doc, id, "href");
        if !default
            && element_attribute(&self.doc, id, "tabindex")
                .and_then(|v| v.trim().parse::<i32>().ok())
                .is_none()
        {
            return false;
        }
        let mut ancestor = Some(id);
        while let Some(node) = ancestor {
            if has(&self.doc, node, "hidden") || has(&self.doc, node, "inert") {
                return false;
            }
            ancestor = self.doc.parent(node);
        }
        find_fragment_bounds(&self.fragment, id, 0.0, 0.0).is_some()
    }

    pub(super) fn native_pointer_focus(&self, target: Option<NodeId>) -> Option<NodeId> {
        let mut node = target?;
        loop {
            if tag(&self.doc, node) == "label" {
                let target = if let Some(name) = element_attribute(&self.doc, node, "for") {
                    find_element_by_id(&self.doc, self.doc.root(), name)
                } else {
                    let mut children = Vec::new();
                    descendants(&self.doc, node, &mut children);
                    children.into_iter().find(|child| {
                        matches!(tag(&self.doc, *child), "input" | "select" | "textarea")
                    })
                };
                return target.filter(|id| self.native_focusable(*id));
            }
            if self.native_focusable(node) {
                return Some(node);
            }
            if matches!(
                tag(&self.doc, node),
                "input" | "button" | "select" | "textarea"
            ) {
                return None;
            }
            node = self.doc.parent(node)?;
        }
    }

    pub(crate) fn native_focus_at(&mut self, focused: Option<NodeId>) -> bool {
        self.native_focus_exit = None;
        let focused = focused.filter(|id| self.native_focusable(*id));
        if self.focused == focused {
            return false;
        }
        self.commit_native_composition();
        self.focused = focused;
        self.native_select
            .focus(focused.filter(|id| tag(&self.doc, *id) == "select"));
        self.native_focus_start = focused;
        self.native_focus_generation = self.native_focus_generation.wrapping_add(1);
        self.native_editor = None;
        self.relayout();
        if let Some(id) = focused {
            if let Some(bounds) = find_fragment_bounds(&self.fragment, id, 0.0, 0.0) {
                if bounds.y < self.scroll_y {
                    self.scroll_y = bounds.y;
                } else if bounds.y + bounds.height > self.scroll_y + self.viewport_height {
                    self.scroll_y = (bounds.y + bounds.height - self.viewport_height).max(0.0);
                }
                self.scroll_y = self
                    .scroll_y
                    .min((self.fragment.height - self.viewport_height).max(0.0));
            }
        }
        true
    }

    pub(super) fn native_radios(&self, id: NodeId) -> Vec<NodeId> {
        let name = element_attribute(&self.doc, id, "name").unwrap_or("");
        if name.is_empty() {
            return vec![id];
        }
        let owner = form_owner(&self.doc, id);
        let mut nodes = Vec::new();
        descendants(&self.doc, self.doc.root(), &mut nodes);
        nodes
            .into_iter()
            .filter(|node| {
                tag(&self.doc, *node) == "input"
                    && input_type(&self.doc, *node) == "radio"
                    && element_attribute(&self.doc, *node, "name") == Some(name)
                    && form_owner(&self.doc, *node) == owner
            })
            .collect()
    }

    fn native_tab_order(&self) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        descendants(&self.doc, self.doc.root(), &mut nodes);
        let mut order: Vec<_> = nodes
            .into_iter()
            .filter_map(|node| {
                if !self.native_focusable(node) {
                    return None;
                }
                let index = element_attribute(&self.doc, node, "tabindex")
                    .and_then(|v| v.trim().parse::<i32>().ok())
                    .unwrap_or(0);
                if index < 0 {
                    return None;
                }
                if tag(&self.doc, node) == "input" && input_type(&self.doc, node) == "radio" {
                    let group: Vec<_> = self
                        .native_radios(node)
                        .into_iter()
                        .filter(|id| {
                            self.native_focusable(*id)
                                && element_attribute(&self.doc, *id, "tabindex")
                                    .and_then(|v| v.trim().parse::<i32>().ok())
                                    .is_none_or(|v| v >= 0)
                        })
                        .collect();
                    let stop = group
                        .iter()
                        .find(|id| has(&self.doc, **id, "checked"))
                        .or_else(|| group.first());
                    if stop != Some(&node) {
                        return None;
                    }
                }
                Some(((index <= 0, index.max(0)), node))
            })
            .collect();
        order.sort_by_key(|(index, _)| *index);
        order.into_iter().map(|(_, node)| node).collect()
    }

    pub(crate) fn native_key_activation(&self, action: &TextInputAction) -> Option<NodeId> {
        let TextInputAction::Key { key, .. } = action else {
            return None;
        };
        let focused = self.focused.filter(|id| self.native_focusable(*id))?;
        let kind = tag(&self.doc, focused);
        let input = input_type(&self.doc, focused);
        if kind == "input"
            && input == "radio"
            && matches!(
                key,
                PageKey::ArrowLeft | PageKey::ArrowRight | PageKey::ArrowUp | PageKey::ArrowDown
            )
        {
            let group: Vec<_> = self
                .native_radios(focused)
                .into_iter()
                .filter(|id| self.native_focusable(*id))
                .collect();
            let index = group.iter().position(|id| *id == focused)?;
            let forward = matches!(key, PageKey::ArrowRight | PageKey::ArrowDown);
            return Some(
                group[if forward {
                    (index + 1) % group.len()
                } else {
                    (index + group.len() - 1) % group.len()
                }],
            );
        }
        match key {
            PageKey::Space
                if kind == "button"
                    || kind == "input"
                        && matches!(
                            input.as_str(),
                            "checkbox" | "radio" | "button" | "submit" | "reset"
                        ) =>
            {
                Some(focused)
            }
            PageKey::Enter
                if matches!(kind, "button" | "a")
                    || kind == "input"
                        && matches!(input.as_str(), "button" | "submit" | "reset") =>
            {
                Some(focused)
            }
            PageKey::Enter
                if kind == "input"
                    && matches!(
                        input.as_str(),
                        "text" | "password" | "search" | "email" | "url" | "tel"
                    ) =>
            {
                let owner = form_owner(&self.doc, focused)?;
                let mut nodes = Vec::new();
                descendants(&self.doc, self.doc.root(), &mut nodes);
                nodes
                    .into_iter()
                    .find(|node| {
                        form_owner(&self.doc, *node) == Some(owner)
                            && (tag(&self.doc, *node) == "button"
                                && !matches!(
                                    input_type(&self.doc, *node).as_str(),
                                    "button" | "reset"
                                )
                                || tag(&self.doc, *node) == "input"
                                    && input_type(&self.doc, *node) == "submit")
                    })
                    .filter(|id| !self.native_control_disabled(*id))
            }
            _ => None,
        }
    }

    pub(super) fn native_page_key(
        &mut self,
        key: PageKey,
        shift: bool,
        context: &TextInputContext,
        source: u64,
    ) -> Result<bool, String> {
        self.native_focus_exit = None;
        if key == PageKey::Tab {
            let order = self.native_tab_order();
            let current = self
                .focused
                .and_then(|id| order.iter().position(|node| *node == id));
            let mut next = match (current, shift) {
                (Some(index), true) => index.checked_sub(1).and_then(|i| order.get(i)).copied(),
                (Some(index), false) => order.get(index + 1).copied(),
                (None, true) => order.last().copied(),
                (None, false) => order.first().copied(),
            };
            if current.is_none() {
                if let Some(start) = self.native_focus_start {
                    let mut nodes = Vec::new();
                    descendants(&self.doc, self.doc.root(), &mut nodes);
                    if let Some(index) = nodes.iter().position(|id| *id == start) {
                        next = if shift {
                            nodes[..index]
                                .iter()
                                .rev()
                                .find(|id| order.contains(id))
                                .copied()
                        } else {
                            nodes[index + 1..]
                                .iter()
                                .find(|id| order.contains(id))
                                .copied()
                        };
                    }
                }
            }
            self.native_focus_at(next);
            if next.is_none() {
                self.native_focus_generation = self.native_focus_generation.wrapping_add(1);
                self.native_focus_exit = Some(if shift {
                    FocusDirection::Backward
                } else {
                    FocusDirection::Forward
                });
            }
            return Ok(true);
        }
        let Some(id) = self.focused.filter(|id| self.native_focusable(*id)) else {
            return Ok(false);
        };
        if tag(&self.doc, id) == "select" {
            return self.native_select_key(id, key, shift, false);
        }
        if tag(&self.doc, id) == "input" && input_type(&self.doc, id) == "range" {
            return self.native_range_key(id, key);
        }
        if let Some(editor) = self.native_text_input_state(source).focused {
            let action = match key {
                PageKey::Space => TextInputAction::Replace {
                    text: " ".into(),
                    replacement: None,
                },
                PageKey::Enter if editor.multiline => TextInputAction::Replace {
                    text: "\n".into(),
                    replacement: None,
                },
                PageKey::ArrowLeft => TextInputAction::Move {
                    direction: TextMovement::Backward,
                    extend: shift,
                },
                PageKey::ArrowRight => TextInputAction::Move {
                    direction: TextMovement::Forward,
                    extend: shift,
                },
                PageKey::ArrowUp => TextInputAction::Move {
                    direction: TextMovement::Up,
                    extend: shift,
                },
                PageKey::ArrowDown => TextInputAction::Move {
                    direction: TextMovement::Down,
                    extend: shift,
                },
                PageKey::Home => TextInputAction::Move {
                    direction: TextMovement::Beginning,
                    extend: shift,
                },
                PageKey::End => TextInputAction::Move {
                    direction: TextMovement::End,
                    extend: shift,
                },
                _ => return Ok(false),
            };
            return self.native_text_input(context, source, action);
        }
        Ok(false)
    }

    fn native_range_key(&mut self, id: NodeId, key: PageKey) -> Result<bool, String> {
        let number = |name: &str, default: f64| {
            element_attribute(&self.doc, id, name)
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(default)
        };
        let (min, max, current) =
            blueice_layout::input_range_values(&self.doc, id).ok_or("Invalid range control")?;
        let step = number("step", 1.0);
        let step = if step > 0.0 { step } else { 1.0 };
        let target = match key {
            PageKey::ArrowRight | PageKey::ArrowUp => current + step,
            PageKey::ArrowLeft | PageKey::ArrowDown => current - step,
            PageKey::Home => min,
            PageKey::End => max,
            _ => return Ok(false),
        };
        let steps = ((target.clamp(min, max) - min) / step)
            .round()
            .min(((max - min) / step).floor());
        let target = (min + steps * step).clamp(min, max);
        if !target.is_finite() {
            return Err("Native range constraints exceed numeric limits".into());
        }
        // Avoid binary-float residue in DOM/semantic values while retaining
        // significant decimal range steps. Extreme exponents use Rust's form.
        let raw = if target.abs() < 1e15 && step >= 1e-12 {
            let raw = format!("{target:.12}");
            raw.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            target.to_string()
        };
        self.native_set_attribute(id, "value", raw);
        self.relayout();
        Ok(true)
    }

    fn native_set_attribute(&mut self, id: NodeId, name: &str, value: String) {
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            return;
        };
        if let Some((_, old)) = attributes
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            *old = value;
        } else {
            attributes.push((name.into(), value));
        }
    }

    pub(super) fn native_set_boolean(&mut self, id: NodeId, name: &str, value: bool) {
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            return;
        };
        attributes.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        if value {
            attributes.push((name.into(), String::new()));
        }
    }

    pub(crate) fn native_control_activation(
        &mut self,
        target: NodeId,
    ) -> Result<Option<crate::navigation_request::BrowserNavigation>, String> {
        if !self.doc.contains(target) {
            return Ok(None);
        }
        let Some(id) = self.native_pointer_focus(Some(target)) else {
            return Ok(None);
        };
        let input = input_type(&self.doc, id);
        if (tag(&self.doc, id) == "button" && !matches!(input.as_str(), "button" | "reset"))
            || (tag(&self.doc, id) == "input" && input == "submit")
        {
            if self.submission_pending {
                return Ok(None);
            }
            return form_owner(&self.doc, id)
                .map(|form| self.prepare_native_form_submission(form, Some(id)))
                .transpose();
        }
        if matches!(tag(&self.doc, id), "input" | "button") && input_type(&self.doc, id) == "reset"
        {
            if let Some(form) = form_owner(&self.doc, id) {
                self.reset_native_form(form);
            }
            return Ok(None);
        }
        if tag(&self.doc, id) == "input" {
            match input_type(&self.doc, id).as_str() {
                "checkbox" => {
                    self.native_set_boolean(id, "checked", !has(&self.doc, id, "checked"));
                }
                "radio" => {
                    for member in self.native_radios(id) {
                        self.native_set_boolean(member, "checked", member == id);
                    }
                    self.native_focus_at(Some(id));
                }
                _ => {}
            }
        }
        self.relayout();
        Ok(None)
    }

    pub(super) fn paint_native_focus(&self, frame: &mut Frame) {
        let Some(id) = self.focused.filter(|id| self.native_focusable(*id)) else {
            return;
        };
        if let Some(bounds) = find_fragment_bounds(&self.fragment, id, 0.0, 0.0) {
            let color = Color::Rgba(35, 100, 220, 255);
            for rect in [
                Rect {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: 2.0,
                },
                Rect {
                    x: bounds.x,
                    y: bounds.y + bounds.height - 2.0,
                    width: bounds.width,
                    height: 2.0,
                },
                Rect {
                    x: bounds.x,
                    y: bounds.y,
                    width: 2.0,
                    height: bounds.height,
                },
                Rect {
                    x: bounds.x + bounds.width - 2.0,
                    y: bounds.y,
                    width: 2.0,
                    height: bounds.height,
                },
            ] {
                frame.commands.push(PaintCommand::Rect { rect, color });
            }
        }
    }
}
