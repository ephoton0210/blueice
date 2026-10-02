// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One core-owned native editor. Ranges are UTF-16 and edits are document-
//! fenced. This boundary does not expand extension DOM-write permissions.

use super::*;
use blueice_ipc::input::{
    TextControlState, TextInputAction, TextInputContext, TextInputState, TextMovement, TextRange,
    MAX_EDIT_TEXT_UTF16, TEXT_INPUT_VERSION,
};
use unicode_segmentation::UnicodeSegmentation;

mod geometry;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct EditorSession {
    node: NodeId,
    anchor: u32,
    cursor: u32,
    observed: String,
    composition: Option<Composition>,
}

#[derive(Clone)]
struct Composition {
    original: String,
    anchor: u32,
    cursor: u32,
    marked: TextRange,
}

#[derive(Clone, Copy)]
struct ControlInfo {
    protected: bool,
    multiline: bool,
    writable: bool,
}

fn attribute<'a>(doc: &'a Document, id: NodeId, name: &str) -> Option<&'a str> {
    element_attribute(doc, id, name)
}

fn control_info(doc: &Document, id: NodeId) -> Option<ControlInfo> {
    if !doc.contains(id) {
        return None;
    }
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(id)
    else {
        return None;
    };
    if attributes
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
    {
        return None;
    }
    let multiline = tag_name.eq_ignore_ascii_case("textarea");
    let kind = attribute(doc, id, "type")
        .unwrap_or("text")
        .to_ascii_lowercase();
    if !(multiline
        || (tag_name.eq_ignore_ascii_case("input")
            && matches!(
                kind.as_str(),
                "text" | "password" | "search" | "email" | "url" | "tel"
            )))
    {
        return None;
    }
    Some(ControlInfo {
        protected: !multiline && kind == "password",
        multiline,
        writable: !attributes
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("readonly")),
    })
}

fn control_value(doc: &Document, id: NodeId, info: ControlInfo) -> String {
    if info.multiline {
        node_text_content(doc, id)
    } else {
        attribute(doc, id, "value").unwrap_or_default().to_string()
    }
}

pub(super) fn supports_native_input(doc: &Document, id: NodeId) -> bool {
    control_info(doc, id).is_some_and(|info| {
        info.writable && control_value(doc, id, info).encode_utf16().count() <= MAX_EDIT_TEXT_UTF16
    })
}

pub(super) fn supports_native_selection(doc: &Document, id: NodeId) -> bool {
    control_info(doc, id).is_some_and(|info| {
        control_value(doc, id, info).encode_utf16().count() <= MAX_EDIT_TEXT_UTF16
    })
}

fn length(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn byte_offset(text: &str, target: u32) -> Option<usize> {
    let mut offset = 0_u32;
    for (byte, ch) in text.char_indices() {
        if offset == target {
            return Some(byte);
        }
        offset += ch.len_utf16() as u32;
        if offset > target {
            return None;
        }
    }
    (offset == target).then_some(text.len())
}

fn checked_range(text: &str, range: TextRange) -> Result<std::ops::Range<usize>, String> {
    let end = range.end().ok_or("Text range overflow")?;
    let start = byte_offset(text, range.location).ok_or("Text range splits a Unicode scalar")?;
    let end = byte_offset(text, end).ok_or("Text range is outside the control")?;
    Ok(start..end)
}

fn grapheme_offsets(text: &str) -> Vec<u32> {
    let mut offsets = vec![0];
    let mut offset = 0;
    for cluster in text.graphemes(true) {
        offset += length(cluster);
        offsets.push(offset);
    }
    offsets
}

impl EditorSession {
    fn new(node: NodeId, value: String) -> Self {
        let cursor = length(&value);
        Self {
            node,
            anchor: cursor,
            cursor,
            observed: value,
            composition: None,
        }
    }
    fn selection(&self) -> TextRange {
        TextRange {
            location: self.anchor.min(self.cursor),
            length: self.anchor.abs_diff(self.cursor),
        }
    }
}

impl Page {
    pub(crate) fn focus_native_editor_at(&mut self, target: Option<NodeId>) -> bool {
        let changed = self.native_focus_at(self.native_pointer_focus(target));
        self.native_focus_start = target.and_then(|id| self.event_element_target(id));
        changed
    }

    fn live_editor(&self) -> Option<(EditorSession, ControlInfo)> {
        let node = self.focused?;
        if !self.native_focusable(node) {
            return None;
        }
        let info = control_info(&self.doc, node)?;
        let value = control_value(&self.doc, node, info);
        if value.encode_utf16().count() > MAX_EDIT_TEXT_UTF16 {
            return None;
        }
        let editor = self
            .native_editor
            .as_ref()
            .filter(|editor| editor.node == node && editor.observed == value)
            .cloned()
            .unwrap_or_else(|| EditorSession::new(node, value));
        Some((editor, info))
    }

    pub(crate) fn native_text_input_state(&self, source: u64) -> TextInputState {
        let focused = self.live_editor().and_then(|(editor, info)| {
            let geometry = geometry::Geometry::new(self, &editor, info)?;
            Some(TextControlState {
                node_id: editor.node.as_u64(),
                text: (!info.protected).then(|| editor.observed.clone()),
                text_length: length(&editor.observed),
                protected: info.protected,
                writable: info.writable,
                multiline: info.multiline,
                selection: editor.selection(),
                marked: editor
                    .composition
                    .as_ref()
                    .map(|composition| composition.marked),
                bounds: geometry.bounds,
                caret: geometry.caret(editor.cursor),
                carets: geometry.bounded_carets(editor.cursor),
                selection_rects: geometry.rects(editor.selection()),
            })
        });
        TextInputState {
            version: TEXT_INPUT_VERSION,
            frame_source: source,
            document_generation: self.document_generation,
            focus_generation: self.native_focus_generation,
            frame_generation: self.frame_generation,
            tab_id: 0,
            scroll_y: self.scroll_y,
            focused_node: self.focused.map(|id| id.as_u64()),
            focus_exit: self.native_focus_exit,
            focused,
        }
    }

    pub(crate) fn validate_native_input_context(
        &self,
        context: &TextInputContext,
        source: u64,
    ) -> Result<(), String> {
        if context.version != TEXT_INPUT_VERSION
            || context.frame_source != source
            || context.document_generation != self.document_generation
            || context.focus_generation != self.native_focus_generation
        {
            return Err("Stale or unsupported native input context".into());
        }
        Ok(())
    }

    pub(crate) fn native_text_input(
        &mut self,
        context: &TextInputContext,
        source: u64,
        action: TextInputAction,
    ) -> Result<bool, String> {
        self.validate_native_input_context(context, source)?;
        if let TextInputAction::Key { key, shift } = action {
            return self.native_page_key(key, shift, context, source);
        }
        let Some((mut editor, info)) = self.live_editor() else {
            return Ok(false);
        };
        let original = editor.observed.clone();
        match action {
            TextInputAction::Key { .. } => unreachable!("page keys handled above"),
            TextInputAction::Replace { text, replacement } => {
                if !info.writable {
                    return Err("Native text control is read-only".into());
                }
                let range = replacement
                    .or_else(|| {
                        editor
                            .composition
                            .as_ref()
                            .map(|composition| composition.marked)
                    })
                    .unwrap_or_else(|| editor.selection());
                let text = normalize(text, info.multiline);
                replace(&mut editor, range, &text)?;
                editor.composition = None;
            }
            TextInputAction::Compose {
                text,
                selection,
                replacement,
            } => {
                if !info.writable {
                    return Err("Native text control is read-only".into());
                }
                let text = normalize(text, info.multiline);
                checked_range(&text, selection)?;
                let range = replacement
                    .or_else(|| {
                        editor
                            .composition
                            .as_ref()
                            .map(|composition| composition.marked)
                    })
                    .unwrap_or_else(|| editor.selection());
                let mut composition = editor.composition.clone().unwrap_or_else(|| Composition {
                    original: editor.observed.clone(),
                    anchor: editor.anchor,
                    cursor: editor.cursor,
                    marked: range,
                });
                replace(&mut editor, range, &text)?;
                composition.marked = TextRange {
                    location: range.location,
                    length: length(&text),
                };
                editor.anchor = range.location + selection.location;
                editor.cursor = editor.anchor + selection.length;
                editor.composition = Some(composition);
            }
            TextInputAction::FinishComposition => {
                editor.composition = None;
            }
            TextInputAction::CancelComposition => {
                if let Some(composition) = editor.composition.take() {
                    editor.observed = composition.original;
                    editor.anchor = composition.anchor;
                    editor.cursor = composition.cursor;
                }
            }
            TextInputAction::Select { range } => {
                checked_range(&editor.observed, range)?;
                editor.composition = None;
                editor.anchor = range.location;
                editor.cursor = range.end().expect("checked range");
            }
            TextInputAction::SelectAll => {
                editor.composition = None;
                editor.anchor = 0;
                editor.cursor = length(&editor.observed);
            }
            TextInputAction::Move { direction, extend } => {
                editor.composition = None;
                let target = movement(self, &editor, info, direction, extend);
                editor.cursor = target;
                if !extend {
                    editor.anchor = target;
                }
            }
            TextInputAction::Delete { forward } => {
                if !info.writable {
                    return Err("Native text control is read-only".into());
                }
                editor.composition = None;
                let offsets = grapheme_offsets(&editor.observed);
                let mut range = editor.selection();
                if range.length == 0 {
                    let left = offsets
                        .iter()
                        .rev()
                        .find(|&&offset| offset <= editor.cursor)
                        .copied()
                        .unwrap_or(0);
                    let right = offsets
                        .iter()
                        .find(|&&offset| offset >= editor.cursor)
                        .copied()
                        .unwrap_or(length(&editor.observed));
                    let start = if forward {
                        left
                    } else {
                        offsets
                            .iter()
                            .rev()
                            .find(|&&offset| offset < editor.cursor)
                            .copied()
                            .unwrap_or(0)
                    };
                    let end = if forward {
                        offsets
                            .iter()
                            .find(|&&offset| offset > editor.cursor)
                            .copied()
                            .unwrap_or(right)
                    } else {
                        right
                    };
                    range = TextRange {
                        location: start,
                        length: end - start,
                    };
                } else {
                    let end = range.end().expect("core selection");
                    let start = offsets
                        .iter()
                        .rev()
                        .find(|&&offset| offset <= range.location)
                        .copied()
                        .unwrap_or(0);
                    let end = offsets
                        .iter()
                        .find(|&&offset| offset >= end)
                        .copied()
                        .unwrap_or(length(&editor.observed));
                    range = TextRange {
                        location: start,
                        length: end - start,
                    };
                }
                replace(&mut editor, range, "")?;
            }
            TextInputAction::Pointer {
                x,
                y,
                extend,
                click_count,
            } => {
                if !x.is_finite() || !y.is_finite() || !(1..=3).contains(&click_count) {
                    return Err("Invalid native pointer".into());
                }
                if let Some(geometry) = geometry::Geometry::new(self, &editor, info) {
                    let point_y = y + self.scroll_y;
                    if geometry.contains(x, point_y) {
                        let target = geometry.nearest(x, point_y);
                        editor.composition = None;
                        editor.cursor = target;
                        if !extend {
                            editor.anchor = target;
                        }
                        if click_count == 2 {
                            let (start, end) = word_at(&editor.observed, target);
                            editor.anchor = start;
                            editor.cursor = end;
                        } else if click_count == 3 {
                            let (start, end) = geometry.line_range(target);
                            editor.anchor = start;
                            editor.cursor = end;
                        }
                    }
                }
            }
        }
        let changed = editor.observed != original;
        if changed {
            self.write_native_control_value(editor.node, info, &editor.observed);
        }
        self.native_editor = Some(editor);
        if changed {
            self.recascade();
        }
        self.relayout();
        Ok(true)
    }

    fn write_native_control_value(&mut self, node: NodeId, info: ControlInfo, value: &str) {
        if info.multiline {
            let children: Vec<_> = self.doc.children(node).collect();
            for child in children {
                self.doc.remove_subtree(child);
            }
            let text = self.doc.create_node(NodeData::Text { data: value.into() });
            self.doc.append_child(node, text);
        } else if let NodeData::Element { attributes, .. } = self.doc.data_mut(node) {
            match attributes
                .iter_mut()
                .find(|(name, _)| name.eq_ignore_ascii_case("value"))
            {
                Some((_, current)) => *current = value.into(),
                None => attributes.push(("value".into(), value.into())),
            }
        }
    }

    pub(super) fn adjust_native_editor_scroll(&mut self) {
        let Some((editor, info)) = self.live_editor() else {
            return;
        };
        let Some(geometry) = geometry::Geometry::new(self, &editor, info) else {
            return;
        };
        let caret = geometry.caret(editor.cursor);
        let right = geometry.clip.x + geometry.clip.width - 1.0;
        let bottom = geometry.clip.y + geometry.clip.height;
        let dx = (caret.x + caret.width - right).max(0.0);
        let dy = if info.multiline {
            (caret.y + caret.height - bottom).max(0.0)
        } else {
            0.0
        };
        fn translate(fragment: &mut Fragment, node: NodeId, dx: f64, dy: f64) {
            if fragment.node == Some(node)
                && matches!(
                    fragment.kind,
                    blueice_layout::FragmentKind::NativeControl { .. }
                )
            {
                for child in &mut fragment.children {
                    child.x -= dx;
                    child.y -= dy;
                }
                return;
            }
            for child in &mut fragment.children {
                translate(child, node, dx, dy);
            }
        }
        translate(&mut self.fragment, editor.node, dx, dy);
    }

    pub(super) fn paint_native_editor(&self, frame: &mut Frame) {
        let Some((editor, info)) = self.live_editor() else {
            return;
        };
        let Some(geometry) = geometry::Geometry::new(self, &editor, info) else {
            return;
        };
        let rect = |bounds: blueice_ipc::Bounds| Rect {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        };
        frame.commands.push(PaintCommand::PushClip {
            rect: rect(geometry.clip),
        });
        for bounds in geometry.rects(editor.selection()) {
            frame.commands.push(PaintCommand::Rect {
                rect: rect(bounds),
                color: Color::Rgba(35, 100, 220, 72),
            });
        }
        if let Some(composition) = &editor.composition {
            for bounds in geometry.rects(composition.marked) {
                frame.commands.push(PaintCommand::Rect {
                    rect: Rect {
                        x: bounds.x,
                        y: bounds.y + bounds.height - 2.0,
                        width: bounds.width,
                        height: 1.0,
                    },
                    color: Color::Rgba(25, 25, 25, 255),
                });
            }
        }
        frame.commands.push(PaintCommand::Rect {
            rect: rect(geometry.caret(editor.cursor)),
            color: Color::Rgba(0, 0, 0, 255),
        });
        frame.commands.push(PaintCommand::PopClip);
    }
}

fn normalize(text: String, multiline: bool) -> String {
    if multiline {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.replace(['\r', '\n'], "")
    }
}

fn replace(editor: &mut EditorSession, range: TextRange, text: &str) -> Result<(), String> {
    let bytes = checked_range(&editor.observed, range)?;
    let total = usize::try_from(length(&editor.observed) - range.length)
        .unwrap_or(usize::MAX)
        .checked_add(text.encode_utf16().count())
        .ok_or("Native text exceeds limit")?;
    if total > MAX_EDIT_TEXT_UTF16 {
        return Err("Native text exceeds limit".into());
    }
    editor.observed.replace_range(bytes, text);
    editor.cursor = range.location + length(text);
    editor.anchor = editor.cursor;
    Ok(())
}

fn word_at(text: &str, cursor: u32) -> (u32, u32) {
    for (byte, word) in text.unicode_word_indices() {
        let start = length(&text[..byte]);
        let end = start + length(word);
        if cursor >= start && cursor <= end {
            return (start, end);
        }
    }
    (cursor, cursor)
}

fn movement(
    page: &Page,
    editor: &EditorSession,
    info: ControlInfo,
    direction: TextMovement,
    extend: bool,
) -> u32 {
    use TextMovement::*;
    let selection = editor.selection();
    if !extend && selection.length > 0 {
        if direction == Backward {
            return selection.location;
        }
        if direction == Forward {
            return selection.end().expect("core selection");
        }
    }
    let offsets = grapheme_offsets(&editor.observed);
    match direction {
        Backward => offsets
            .into_iter()
            .rev()
            .find(|&offset| offset < editor.cursor)
            .unwrap_or(0),
        Forward => offsets
            .into_iter()
            .find(|&offset| offset > editor.cursor)
            .unwrap_or(length(&editor.observed)),
        Beginning => 0,
        End => length(&editor.observed),
        WordBackward => editor
            .observed
            .unicode_word_indices()
            .map(|(byte, _)| length(&editor.observed[..byte]))
            .rfind(|&offset| offset < editor.cursor)
            .unwrap_or(0),
        WordForward => editor
            .observed
            .unicode_word_indices()
            .map(|(byte, word)| length(&editor.observed[..byte]) + length(word))
            .find(|&offset| offset > editor.cursor)
            .unwrap_or(length(&editor.observed)),
        LineBeginning | LineEnd | Up | Down => geometry::Geometry::new(page, editor, info)
            .map(|geometry| {
                let (start, end) = geometry.line_range(editor.cursor);
                match direction {
                    LineBeginning => start,
                    LineEnd => end,
                    _ => geometry.vertical(editor.cursor, direction == Down),
                }
            })
            .unwrap_or(editor.cursor),
    }
}
