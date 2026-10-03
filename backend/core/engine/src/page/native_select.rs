// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Select interaction belongs to the core; AppKit only presents bounded labels.
use super::native_interaction::tag;
use super::*;
use blueice_ipc::input::{PageKey, SelectChoice, SelectControlState, TextInputAction};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct SelectSession {
    node: Option<NodeId>,
    active: Option<NodeId>,
    anchor: Option<NodeId>,
    prefix: String,
    typed: Option<Instant>,
    positions: HashMap<NodeId, usize>,
    ensure_active: bool,
}

impl SelectSession {
    pub(super) fn focus(&mut self, node: Option<NodeId>) {
        self.reset_focus();
        self.node = node;
        self.ensure_active = true;
    }
    fn reset_focus(&mut self) {
        self.node = None;
        self.active = None;
        self.anchor = None;
        self.prefix.clear();
        self.typed = None;
        self.ensure_active = false;
    }
}

impl Page {
    pub(crate) fn native_option_selected(&self, option: NodeId) -> bool {
        let mut parent = self.doc.parent(option);
        while let Some(id) = parent {
            if tag(&self.doc, id) == "select" {
                return blueice_layout::select_options(&self.doc, id)
                    .iter()
                    .any(|o| o.node == option && o.selected);
            }
            parent = self.doc.parent(id);
        }
        false
    }

    pub(super) fn native_options(&self, select: NodeId) -> Vec<NodeId> {
        blueice_layout::select_options(&self.doc, select)
            .into_iter()
            .map(|option| option.node)
            .collect()
    }

    pub(crate) fn native_select_value(&self, id: NodeId) -> Option<String> {
        let option = blueice_layout::select_options(&self.doc, id)
            .into_iter()
            .find(|option| option.selected)?
            .node;
        Some(
            element_attribute(&self.doc, option, "value")
                .map(str::to_string)
                .unwrap_or_else(|| {
                    node_text_content(&self.doc, option)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                }),
        )
    }

    pub(super) fn native_select_state(&self) -> Option<SelectControlState> {
        let id = self
            .focused
            .filter(|id| tag(&self.doc, *id) == "select" && self.native_focusable(*id))?;
        let options = blueice_layout::select_options(&self.doc, id);
        let active = self.select_active(id, &options);
        Some(SelectControlState {
            node_id: id.as_u64(),
            multiple: element_attribute(&self.doc, id, "multiple").is_some(),
            popup: blueice_layout::select_display_size(&self.doc, id) == 1
                && element_attribute(&self.doc, id, "multiple").is_none(),
            limited: options.len() > 1024
                || options.iter().any(|o| {
                    o.label.chars().count() > 256
                        || o.group.as_ref().is_some_and(|g| g.chars().count() > 256)
                }),
            bounds: find_fragment_bounds(&self.fragment, id, 0.0, 0.0)?,
            active_option: active.map(|id| id.as_u64()),
            options: options
                .into_iter()
                .take(1024)
                .map(|option| SelectChoice {
                    node_id: option.node.as_u64(),
                    label: option.label.chars().take(256).collect(),
                    group: option.group.map(|g| g.chars().take(256).collect()),
                    selected: option.selected,
                    disabled: self.native_control_disabled(option.node),
                })
                .collect(),
        })
    }

    fn select_active(
        &self,
        id: NodeId,
        options: &[blueice_layout::SelectOption],
    ) -> Option<NodeId> {
        self.native_select
            .active
            .filter(|active| {
                self.native_select.node == Some(id)
                    && options
                        .iter()
                        .any(|o| o.node == *active && !self.native_control_disabled(*active))
            })
            .or_else(|| {
                options
                    .iter()
                    .find(|o| o.selected && !self.native_control_disabled(o.node))
                    .map(|o| o.node)
            })
            .or_else(|| {
                options
                    .iter()
                    .find(|o| !self.native_control_disabled(o.node))
                    .map(|o| o.node)
            })
    }

    fn choose_select(
        &mut self,
        id: NodeId,
        target: NodeId,
        extend: bool,
        toggle: bool,
    ) -> Result<bool, String> {
        let options = blueice_layout::select_options(&self.doc, id);
        if !options.iter().any(|o| o.node == target) || self.native_control_disabled(target) {
            return Err("Option is unavailable in the focused select".into());
        }
        let multiple = element_attribute(&self.doc, id, "multiple").is_some();
        let active = self.select_active(id, &options).unwrap_or(target);
        let anchor = self
            .native_select
            .anchor
            .filter(|anchor| {
                self.native_select.node == Some(id) && options.iter().any(|o| o.node == *anchor)
            })
            .unwrap_or(active);
        let target_index = options.iter().position(|o| o.node == target).unwrap();
        let anchor_index = options.iter().position(|o| o.node == anchor).unwrap();
        for (index, option) in options.iter().enumerate() {
            let selected = if multiple && extend {
                if self.native_control_disabled(option.node) {
                    option.selected
                } else {
                    (anchor_index.min(target_index)..=anchor_index.max(target_index))
                        .contains(&index)
                        || toggle && option.selected
                }
            } else if toggle && multiple {
                if option.node == target {
                    !option.selected
                } else {
                    option.selected
                }
            } else if toggle
                && blueice_layout::select_display_size(&self.doc, id) > 1
                && option.node == target
            {
                !option.selected
            } else {
                option.node == target
            };
            self.native_set_boolean(option.node, "selected", selected);
        }
        self.native_select.node = Some(id);
        self.native_select.active = Some(target);
        self.native_select.anchor = Some(if extend && multiple { anchor } else { target });
        self.native_select.ensure_active = true;
        self.relayout();
        Ok(true)
    }

    pub(super) fn native_select_key(
        &mut self,
        id: NodeId,
        key: PageKey,
        extend: bool,
        toggle: bool,
    ) -> Result<bool, String> {
        let options = blueice_layout::select_options(&self.doc, id);
        let enabled: Vec<_> = options
            .iter()
            .filter(|o| !self.native_control_disabled(o.node))
            .map(|o| o.node)
            .collect();
        let Some(active) = self.select_active(id, &options) else {
            return Ok(false);
        };
        let index = enabled.iter().position(|id| *id == active).unwrap_or(0);
        let target = match key {
            PageKey::ArrowDown | PageKey::ArrowRight => enabled[(index + 1).min(enabled.len() - 1)],
            PageKey::ArrowUp | PageKey::ArrowLeft => enabled[index.saturating_sub(1)],
            PageKey::PageDown => {
                enabled[(index
                    + blueice_layout::select_display_size(&self.doc, id)
                        .saturating_sub(1)
                        .max(1) as usize)
                    .min(enabled.len() - 1)]
            }
            PageKey::PageUp => {
                enabled[index.saturating_sub(
                    blueice_layout::select_display_size(&self.doc, id)
                        .saturating_sub(1)
                        .max(1) as usize,
                )]
            }
            PageKey::Home => enabled[0],
            PageKey::End => *enabled.last().unwrap(),
            PageKey::Space if element_attribute(&self.doc, id, "multiple").is_some() => active,
            _ => return Ok(false),
        };
        self.native_select.prefix.clear();
        self.native_select.typed = None;
        if toggle && !extend && key != PageKey::Space {
            self.native_select.node = Some(id);
            self.native_select.active = Some(target);
            self.native_select.ensure_active = true;
            self.relayout();
            return Ok(true);
        }
        self.choose_select(id, target, extend, toggle || key == PageKey::Space)
    }

    fn select_typeahead(&mut self, id: NodeId, text: &str) -> Result<bool, String> {
        if text.is_empty()
            || text.chars().any(char::is_control)
            || text.encode_utf16().count() > 256
        {
            return Ok(false);
        }
        let options = blueice_layout::select_options(&self.doc, id);
        let active = self.select_active(id, &options);
        let continuing = self.native_select.node == Some(id)
            && self
                .native_select
                .typed
                .is_some_and(|time| time.elapsed() < Duration::from_secs(1));
        let text = text.to_lowercase();
        let repeated = continuing && text.chars().count() == 1 && self.native_select.prefix == text;
        if !continuing || repeated {
            self.native_select.prefix = text;
        } else if self.native_select.prefix.len() + text.len() <= 1024 {
            self.native_select.prefix.push_str(&text);
        }
        self.native_select.node = Some(id);
        self.native_select.typed = Some(Instant::now());
        let start = options
            .iter()
            .position(|o| Some(o.node) == active)
            .unwrap_or(0);
        let target = (0..options.len())
            .map(|offset| (start + offset + usize::from(!continuing || repeated)) % options.len())
            .find(|index| {
                !self.native_control_disabled(options[*index].node)
                    && options[*index]
                        .label
                        .to_lowercase()
                        .starts_with(&self.native_select.prefix)
            });
        if let Some(index) = target {
            self.choose_select(id, options[index].node, false, false)
        } else {
            Ok(false)
        }
    }

    pub(super) fn native_select_action(
        &mut self,
        action: &TextInputAction,
    ) -> Option<Result<bool, String>> {
        if let TextInputAction::SelectScroll { x, y, rows } = action {
            if !x.is_finite() || !y.is_finite() {
                return Some(Err("Invalid select scroll pointer".into()));
            }
            let id = self.native_pointer_focus(self.click_target(*x, *y))?;
            if tag(&self.doc, id) != "select"
                || (blueice_layout::select_display_size(&self.doc, id) == 1
                    && element_attribute(&self.doc, id, "multiple").is_none())
            {
                return Some(Ok(false));
            }
            let max = self.native_options(id).len();
            let position = self.native_select.positions.entry(id).or_insert(0);
            *position = position
                .saturating_add_signed((*rows).clamp(-1024, 1024) as isize)
                .min(max);
            self.native_select.ensure_active = false;
            self.relayout();
            return Some(Ok(true));
        }
        let explicit = matches!(
            action,
            TextInputAction::SelectOption { .. }
                | TextInputAction::SelectKey { .. }
                | TextInputAction::SelectPointer { .. }
                | TextInputAction::SelectScroll { .. }
        );
        let id = match self
            .focused
            .filter(|id| tag(&self.doc, *id) == "select" && self.native_focusable(*id))
        {
            Some(id) => id,
            None => return explicit.then(|| Err("No live focused select".into())),
        };
        Some(match action {
            TextInputAction::SelectOption {
                option_id,
                frame_generation,
                extend,
                toggle,
            } => {
                if *frame_generation != self.frame_generation {
                    Err("Stale select popup frame".into())
                } else if let Some(target) = self
                    .native_options(id)
                    .into_iter()
                    .find(|id| id.as_u64() == *option_id)
                {
                    self.choose_select(id, target, *extend, *toggle)
                } else {
                    Err("Option does not belong to the focused select".into())
                }
            }
            TextInputAction::SelectKey {
                key,
                extend,
                toggle,
            } => self.native_select_key(id, *key, *extend, *toggle),
            TextInputAction::Replace {
                text,
                replacement: None,
            } => self.select_typeahead(id, text),
            TextInputAction::SelectAll
                if element_attribute(&self.doc, id, "multiple").is_some() =>
            {
                for option in self.native_options(id) {
                    if !self.native_control_disabled(option) {
                        self.native_set_boolean(option, "selected", true);
                    }
                }
                self.relayout();
                Ok(true)
            }
            TextInputAction::SelectPointer {
                x,
                y,
                extend,
                toggle,
            } => self.select_pointer(id, *x, *y, *extend, *toggle),
            TextInputAction::Pointer { x, y, extend, .. } => {
                self.select_pointer(id, *x, *y, *extend, false)
            }
            _ => Ok(false),
        })
    }

    fn select_pointer(
        &mut self,
        id: NodeId,
        x: f64,
        y: f64,
        extend: bool,
        toggle: bool,
    ) -> Result<bool, String> {
        if !x.is_finite() || !y.is_finite() {
            return Err("Invalid select pointer".into());
        }
        if blueice_layout::select_display_size(&self.doc, id) == 1
            && element_attribute(&self.doc, id, "multiple").is_none()
        {
            return Ok(false);
        }
        let target = self.native_options(id).into_iter().find(|option| {
            find_fragment_bounds(&self.fragment, *option, 0.0, 0.0).is_some_and(|b| {
                b.width > 0.0
                    && b.height > 0.0
                    && x >= b.x
                    && x < b.x + b.width
                    && y + self.scroll_y >= b.y
                    && y + self.scroll_y < b.y + b.height
            })
        });
        if let Some(target) = target {
            if self.native_control_disabled(target) {
                Ok(false)
            } else {
                self.choose_select(id, target, extend, toggle)
            }
        } else {
            Ok(false)
        }
    }

    pub(super) fn adjust_native_select_scroll(&mut self) {
        fn apply(fragment: &mut Fragment, session: &mut SelectSession, focused: Option<NodeId>) {
            if let blueice_layout::FragmentKind::NativeControl {
                content_height,
                form:
                    Some(blueice_layout::NativeForm::SelectList {
                        options,
                        first,
                        row_height,
                        active,
                    }),
                ..
            } = &mut fragment.kind
            {
                if let Some(node) = fragment.node {
                    let rows = (*content_height / *row_height).floor().max(1.0) as usize;
                    *first = session.positions.get(&node).copied().unwrap_or(*first);
                    *active = None;
                    if focused == Some(node) {
                        *active = session
                            .active
                            .filter(|id| {
                                session.node == Some(node)
                                    && options.iter().any(|o| o.node == *id && !o.disabled)
                            })
                            .or_else(|| {
                                options
                                    .iter()
                                    .find(|o| o.selected && !o.disabled)
                                    .map(|o| o.node)
                            })
                            .or_else(|| options.iter().find(|o| !o.disabled).map(|o| o.node));
                        if session.ensure_active && session.node == Some(node) {
                            if let Some(index) =
                                options.iter().position(|o| Some(o.node) == *active)
                            {
                                if index < *first {
                                    *first = index;
                                } else if index >= *first + rows {
                                    *first = index + 1 - rows;
                                }
                            }
                        }
                    }
                    *first = (*first).min(options.len().saturating_sub(rows));
                    session.positions.insert(node, *first);
                }
                blueice_layout::update_select_option_bounds(fragment);
            }
            for child in &mut fragment.children {
                apply(child, session, focused);
            }
        }
        self.native_select
            .positions
            .retain(|node, _| self.doc.contains(*node) && tag(&self.doc, *node) == "select");
        apply(&mut self.fragment, &mut self.native_select, self.focused);
        self.native_select.ensure_active = false;
    }
}
