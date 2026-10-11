// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drag targets, source selection and default actions belong to this live
//! core document. The frontend supplies points and explicitly dropped data.

use super::*;
use blueice_ipc::input::{
    NativeDragItem, NativeDragMouse, NativeDragOperation as Operation, NativeDragState,
    PointerPoint, TextInputAction, TextRange, MAX_DRAG_FORMATS,
};
use blueice_ipc::page_host::{valid_drag_format, valid_drag_items};

#[derive(Clone)]
pub(crate) struct DragDestination {
    pub id: u64,
    pub point: PointerPoint,
    pub mouse: Option<NativeDragMouse>,
    pub target: Option<NodeId>,
    pub types: Vec<String>,
    pub allowed: Vec<Operation>,
    pub operation: Operation,
}

#[derive(Clone)]
pub(crate) struct DragSource {
    pub id: u64,
    pub point: PointerPoint,
    pub mouse: Option<NativeDragMouse>,
    pub target: NodeId,
    pub items: Vec<NativeDragItem>,
    pub allowed: Vec<Operation>,
    pub edit: Option<SourceEdit>,
}

#[derive(Clone)]
pub(crate) struct SourceEdit {
    node: NodeId,
    range: TextRange,
    observed: String,
}

impl Page {
    pub(crate) fn set_native_drag_geometry(
        &self,
        node: NodeId,
        event: &mut blueice_ipc::page_host::PageHostDragEvent,
    ) {
        let point = event.point();
        let page_y = point.y + self.scroll_y;
        event.page_x_bits = point.x.to_bits();
        event.page_y_bits = page_y.to_bits();
        event.offset_x_bits = point.x.to_bits();
        event.offset_y_bits = page_y.to_bits();
        if let Some(bounds) = find_fragment_bounds(&self.fragment, node, 0.0, 0.0) {
            // The padding edge starts immediately inside the border. These
            // supported border lengths use the same font context as layout.
            let border = |name: &str| {
                self.styles
                    .get(&node)
                    .map_or(0.0, |style| match style.other.get(name) {
                        Some(blueice_css::Value::Length(blueice_css::Length::Px(value))) => *value,
                        Some(blueice_css::Value::Length(blueice_css::Length::Em(value))) => {
                            value * style.font_size_px
                        }
                        _ => 0.0,
                    })
            };
            event.offset_x_bits = (point.x - bounds.x - border("border-left-width")).to_bits();
            event.offset_y_bits = (page_y - bounds.y - border("border-top-width")).to_bits();
        }
    }

    pub(crate) fn native_drag_static_source(&self, node: NodeId) -> bool {
        if !self.native_activation_available(node) {
            return false;
        }
        if self.native_activation_link(node).is_some() {
            return true;
        }
        let mut current = Some(node);
        for _ in 0..=256 {
            let Some(candidate) = current else {
                return false;
            };
            if element_attribute(&self.doc, candidate, "draggable")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            {
                return true;
            }
            current = self.doc.parent(candidate);
        }
        false
    }

    pub(crate) fn native_drag_target(&self, point: PointerPoint) -> Option<NodeId> {
        if !self.validate_menu_point(point.x, point.y) {
            return None;
        }
        self.click_event_target(point.x, point.y)
            .filter(|node| self.native_activation_available(*node))
    }

    pub(crate) fn begin_native_drag(
        &mut self,
        id: u64,
        point: PointerPoint,
        types: Vec<String>,
        allowed: Vec<Operation>,
        mouse: Option<NativeDragMouse>,
    ) -> Result<(Option<DragDestination>, DragDestination), String> {
        if id == 0
            || types.len() > MAX_DRAG_FORMATS
            || types.iter().any(|format| !valid_drag_format(format))
            || allowed.is_empty()
            || allowed.len() > 3
            || allowed.contains(&Operation::None)
            || allowed
                .iter()
                .enumerate()
                .any(|(index, operation)| allowed[..index].contains(operation))
        {
            return Err("Invalid native drag metadata".into());
        }
        let types: Vec<_> = types
            .into_iter()
            .map(|format| {
                if format == "Files" {
                    format
                } else {
                    format.to_ascii_lowercase()
                }
            })
            .collect();
        if types
            .iter()
            .enumerate()
            .any(|(index, format)| types[..index].contains(format))
        {
            return Err("Duplicate native drag formats".into());
        }
        let previous = self.native_drag.clone();
        if let Some(drag) = &self.native_drag {
            if drag.id == id && (drag.types != types || drag.allowed != allowed) {
                return Err("Native drag metadata changed within a session".into());
            }
        }
        if self.native_drag.as_ref().is_none_or(|drag| drag.id != id) {
            if id <= self.native_drag_last_id {
                return Err("Native drag identifier has expired".into());
            }
            self.native_drag_last_id = id;
        }
        let drag = DragDestination {
            id,
            point,
            mouse,
            target: self.native_drag_target(point),
            types,
            allowed,
            operation: Operation::None,
        };
        self.native_drag = Some(drag.clone());
        Ok((previous, drag))
    }

    pub(crate) fn current_native_drag(&self, id: u64) -> Result<DragDestination, String> {
        self.native_drag
            .as_ref()
            .filter(|drag| drag.id == id)
            .cloned()
            .ok_or_else(|| "Native drag session has expired".into())
    }

    pub(crate) fn set_native_drag_operation(
        &mut self,
        id: u64,
        operation: Operation,
    ) -> Result<(), String> {
        let drag = self
            .native_drag
            .as_mut()
            .filter(|drag| drag.id == id)
            .ok_or("Native drag session has expired")?;
        drag.operation = if operation == Operation::None || drag.allowed.contains(&operation) {
            operation
        } else {
            Operation::None
        };
        Ok(())
    }

    pub(crate) fn validate_native_drag_items(
        &self,
        id: u64,
        items: &[NativeDragItem],
    ) -> Result<DragDestination, String> {
        let drag = self.current_native_drag(id)?;
        if !valid_drag_items(items)
            || items.iter().any(|item| match item {
                NativeDragItem::Text { format, .. } => {
                    !drag.types.contains(&format.to_ascii_lowercase())
                }
                NativeDragItem::File(_) => !drag.types.iter().any(|format| format == "Files"),
            })
        {
            return Err("Native drop content differs from the advertised formats".into());
        }
        Ok(drag)
    }

    pub(crate) fn end_native_drag_destination(
        &mut self,
        id: u64,
    ) -> Result<DragDestination, String> {
        self.current_native_drag(id)?;
        Ok(self.native_drag.take().expect("validated drag"))
    }

    pub(crate) fn native_drag_default_operation(
        &self,
        drag: &DragDestination,
        source: u64,
    ) -> Operation {
        let Some(target) = drag.target else {
            return Operation::None;
        };
        let file = drag.types.iter().any(|format| format == "Files")
            && self
                .file_input_state(target.as_u64(), source, self.document_generation)
                .is_ok();
        let text = drag.types.iter().any(|format| format == "text/plain")
            && self.native_drag_text_writable(target);
        if file && drag.allowed.contains(&Operation::Copy) {
            return Operation::Copy;
        }
        if text {
            return [Operation::Copy, Operation::Move, Operation::Link]
                .into_iter()
                .find(|operation| drag.allowed.contains(operation))
                .unwrap_or(Operation::None);
        }
        Operation::None
    }

    fn native_drag_text_writable(&self, target: NodeId) -> bool {
        self.supports_native_text_input(target)
            && element_attribute(&self.doc, target, "readonly").is_none()
    }

    pub(crate) fn apply_native_drop_default(
        &mut self,
        drag: &DragDestination,
        source: u64,
        items: Vec<NativeDragItem>,
    ) -> Result<(bool, Option<NodeId>), String> {
        let Some(target) = drag
            .target
            .filter(|node| self.native_activation_available(*node))
        else {
            return Ok((false, None));
        };
        let files: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                NativeDragItem::File(file) => Some(file.clone()),
                _ => None,
            })
            .collect();
        if !files.is_empty()
            && self
                .file_input_state(target.as_u64(), source, self.document_generation)
                .is_ok()
        {
            let context = self
                .file_input_state(target.as_u64(), source, self.document_generation)?
                .context;
            self.set_file_input(&context, source, files)?;
            return Ok((true, Some(target)));
        }
        let Some(text) = items.into_iter().find_map(|item| match item {
            NativeDragItem::Text { format, data } if format.eq_ignore_ascii_case("text/plain") => {
                Some(data)
            }
            _ => None,
        }) else {
            return Ok((false, None));
        };
        if !self.native_drag_text_writable(target) {
            return Ok((false, None));
        }
        if text.encode_utf16().count() > blueice_ipc::input::MAX_EDIT_TEXT_UTF16 {
            return Err("Dropped text exceeds the editor limit".into());
        }
        self.native_focus_at(Some(target));
        let context = self.native_text_input_state(source).context();
        self.native_text_input(
            &context,
            source,
            TextInputAction::Pointer {
                x: drag.point.x,
                y: drag.point.y,
                extend: false,
                click_count: 1,
            },
        )?;
        let focused = self
            .native_text_input_state(source)
            .focused
            .ok_or("Drop editor is unavailable")?;
        if drag.operation == Operation::Move {
            if let Some(edit) = self
                .native_drag_source
                .as_ref()
                .and_then(|source| source.edit.as_ref())
                .filter(|edit| edit.node == target && focused.text.as_ref() == Some(&edit.observed))
                .cloned()
            {
                let caret = focused.selection.location;
                let end = edit.range.end().ok_or("Drag source range overflow")?;
                if caret >= edit.range.location && caret <= end {
                    self.native_drag_source
                        .as_mut()
                        .expect("source exists")
                        .edit = None;
                    return Ok((true, None));
                }
                let bytes = native_editing::checked_range(&edit.observed, edit.range)?;
                let insertion = native_editing::byte_offset(&edit.observed, caret)
                    .ok_or("Drop caret is unavailable")?;
                let mut moved = edit.observed.clone();
                let insertion = if insertion > bytes.end {
                    insertion - bytes.len()
                } else {
                    insertion
                };
                moved.replace_range(bytes, "");
                moved.insert_str(insertion, &text);
                self.native_text_input(
                    &context,
                    source,
                    TextInputAction::Replace {
                        text: moved,
                        replacement: Some(TextRange {
                            location: 0,
                            length: edit.observed.encode_utf16().count() as u32,
                        }),
                    },
                )?;
                self.native_drag_source
                    .as_mut()
                    .expect("source exists")
                    .edit = None;
                return Ok((true, Some(target)));
            }
        }
        let changed = self.native_text_input(
            &context,
            source,
            TextInputAction::Replace {
                text,
                replacement: None,
            },
        )?;
        Ok((true, changed.then_some(target)))
    }

    pub(crate) fn native_drag_reply(
        &self,
        source: u64,
        id: u64,
        active: bool,
        operation: Operation,
        items: Option<Vec<NativeDragItem>>,
        mouse: Option<NativeDragMouse>,
    ) -> NativeDragState {
        NativeDragState {
            version: 1,
            drag_id: id,
            frame_source: source,
            document_generation: self.document_generation,
            frame_generation: self.frame_generation,
            mouse,
            active,
            operation,
            allowed_operations: self
                .native_drag_source
                .as_ref()
                .filter(|drag| drag.id == id)
                .map(|drag| drag.allowed.clone())
                .or_else(|| {
                    self.native_drag
                        .as_ref()
                        .filter(|drag| drag.id == id)
                        .map(|drag| drag.allowed.clone())
                })
                .unwrap_or_else(|| {
                    // An ended transaction still acknowledges its committed
                    // operation after its live source/destination is consumed.
                    if operation == Operation::None {
                        Vec::new()
                    } else {
                        vec![operation]
                    }
                }),
            items,
        }
    }

    pub(crate) fn native_drag_source_seed(
        &self,
        id: u64,
        point: PointerPoint,
        allowed: Vec<Operation>,
        source: u64,
    ) -> Result<DragSource, String> {
        if id == 0
            || id <= self.native_drag_source_last_id
            || self.native_drag_source.is_some()
            || allowed.is_empty()
            || allowed.len() > 3
            || allowed.contains(&Operation::None)
            || allowed
                .iter()
                .enumerate()
                .any(|(index, op)| allowed[..index].contains(op))
        {
            return Err("Native drag source is unavailable".into());
        }
        let target = self
            .native_drag_target(point)
            .ok_or("Native drag source is unavailable")?;
        let state = self.native_text_input_state(source);
        if let Some(control) = state.focused.filter(|control| {
            control.node_id == target.as_u64()
                && !control.protected
                && control.selection.length > 0
                && control
                    .selection_rects
                    .iter()
                    .any(|bounds| contains(*bounds, point))
        }) {
            let text = control
                .text
                .ok_or("Protected text cannot become a drag source")?;
            let bytes = native_editing::checked_range(&text, control.selection)?;
            let payload = text[bytes].to_string();
            let edit = control.writable.then_some(SourceEdit {
                node: target,
                range: control.selection,
                observed: text,
            });
            let allowed = allowed
                .into_iter()
                .filter(|operation| *operation != Operation::Move || edit.is_some())
                .collect();
            return Ok(DragSource {
                id,
                point,
                mouse: None,
                target,
                items: vec![NativeDragItem::Text {
                    format: "text/plain".into(),
                    data: payload,
                }],
                allowed,
                edit,
            });
        }
        if let Some(text) = self.document_drag_text_at(point) {
            return Ok(DragSource {
                id,
                point,
                mouse: None,
                target,
                items: vec![NativeDragItem::Text {
                    format: "text/plain".into(),
                    data: text,
                }],
                allowed: allowed
                    .into_iter()
                    .filter(|operation| *operation != Operation::Move)
                    .collect(),
                edit: None,
            });
        }
        if let Some(url) = self.native_activation_link(target) {
            return Ok(DragSource {
                id,
                point,
                mouse: None,
                target,
                items: vec![
                    NativeDragItem::Text {
                        format: "text/uri-list".into(),
                        data: url.clone(),
                    },
                    NativeDragItem::Text {
                        format: "text/plain".into(),
                        data: url,
                    },
                ],
                allowed,
                edit: None,
            });
        }
        let mut node = Some(target);
        while let Some(candidate) = node {
            if element_attribute(&self.doc, candidate, "draggable")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            {
                return Ok(DragSource {
                    id,
                    point,
                    mouse: None,
                    target: candidate,
                    items: Vec::new(),
                    allowed,
                    edit: None,
                });
            }
            node = self.doc.parent(candidate);
        }
        Err("This point has no document drag source".into())
    }

    pub(crate) fn install_native_drag_source(
        &mut self,
        mut drag: DragSource,
        items: Vec<NativeDragItem>,
    ) -> Result<(), String> {
        if drag.allowed.is_empty() || !valid_drag_items(&items) {
            return Err("Native drag source payload is unavailable".into());
        }
        drag.items = items;
        self.native_drag_source = Some(drag);
        Ok(())
    }

    pub(crate) fn reserve_native_drag_source(&mut self, id: u64) -> Result<(), String> {
        if id == 0 || id <= self.native_drag_source_last_id || self.native_drag_source.is_some() {
            return Err("Native drag source identifier has expired".into());
        }
        self.native_drag_source_last_id = id;
        Ok(())
    }

    pub(crate) fn current_native_drag_source(&self, id: u64) -> Result<DragSource, String> {
        self.native_drag_source
            .as_ref()
            .filter(|source| source.id == id)
            .cloned()
            .ok_or_else(|| "Native drag source has expired".into())
    }

    pub(crate) fn move_native_drag_source(
        &mut self,
        id: u64,
        point: PointerPoint,
        mouse: Option<NativeDragMouse>,
    ) -> Result<DragSource, String> {
        let drag = self
            .native_drag_source
            .as_mut()
            .filter(|source| source.id == id)
            .ok_or("Native drag source has expired")?;
        drag.point = point;
        drag.mouse = mouse;
        Ok(drag.clone())
    }

    pub(crate) fn end_native_drag_source(
        &mut self,
        id: u64,
        source: u64,
        operation: Operation,
    ) -> Result<Option<NodeId>, String> {
        let drag = self.current_native_drag_source(id)?;
        if operation != Operation::None && !drag.allowed.contains(&operation) {
            return Err("Native drag source operation was not allowed".into());
        }
        self.native_drag_source = None;
        if operation != Operation::Move {
            return Ok(None);
        }
        let Some(edit) = drag.edit else {
            return Ok(None);
        };
        let saved_focus = self.focused;
        let saved_editor = self.native_editor.clone();
        self.native_focus_at(Some(edit.node));
        let state = self.native_text_input_state(source);
        let result = if state.focused.as_ref().is_some_and(|control| {
            control.text.as_ref() == Some(&edit.observed) && control.writable
        }) {
            self.native_text_input(
                &state.context(),
                source,
                TextInputAction::Replace {
                    text: String::new(),
                    replacement: Some(edit.range),
                },
            )
            .map(|changed| changed.then_some(edit.node))
        } else {
            Ok(None)
        };
        if saved_focus != Some(edit.node) {
            self.native_focus_at(saved_focus);
            self.native_editor = saved_editor;
        }
        result
    }

    pub(crate) fn clear_native_drag(&mut self) {
        self.native_drag = None;
        self.native_drag_source = None;
    }
}

fn contains(bounds: blueice_ipc::Bounds, point: PointerPoint) -> bool {
    point.x >= bounds.x
        && point.y >= bounds.y
        && point.x <= bounds.x + bounds.width
        && point.y <= bounds.y + bounds.height
}

pub(crate) fn effect_allowed(allowed: &[Operation]) -> &'static str {
    match (
        allowed.contains(&Operation::Copy),
        allowed.contains(&Operation::Move),
        allowed.contains(&Operation::Link),
    ) {
        (true, true, true) => "all",
        (true, true, false) => "copyMove",
        (true, false, true) => "copyLink",
        (false, true, true) => "linkMove",
        (true, false, false) => "copy",
        (false, true, false) => "move",
        (false, false, true) => "link",
        _ => "none",
    }
}

pub(crate) fn restrict_operations(allowed: &[Operation], effect: &str) -> Vec<Operation> {
    allowed
        .iter()
        .copied()
        .filter(|operation| match effect {
            "all" | "uninitialized" => true,
            "copy" => *operation == Operation::Copy,
            "move" => *operation == Operation::Move,
            "link" => *operation == Operation::Link,
            "copyMove" => matches!(operation, Operation::Copy | Operation::Move),
            "copyLink" => matches!(operation, Operation::Copy | Operation::Link),
            "linkMove" => matches!(operation, Operation::Link | Operation::Move),
            _ => false,
        })
        .collect()
}
