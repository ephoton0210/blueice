// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parameterized AT text requests share the native editor and exact layout.
use super::*;
use blueice_ipc::accessibility::{
    AccessibilityTextAction as Action, AccessibilityTextContext as Context,
    AccessibilityTextReply as Reply, AccessibilityTextResult as ResultValue,
    AccessibilityTextState as State, AccessibilityTextStyle, ACCESSIBILITY_TEXT_VERSION,
};
use blueice_ipc::Bounds;

impl Page {
    fn accessibility_editor(
        &self,
        context: &Context,
        source: u64,
    ) -> Result<(EditorSession, ControlInfo), String> {
        if context.version != ACCESSIBILITY_TEXT_VERSION
            || context.frame_source != source
            || context.document_generation != self.document_generation
            || context.frame_generation != self.frame_generation
        {
            return Err("Stale or unsupported accessibility text context".into());
        }
        let node = NodeId::from_u64(context.node_id);
        if !self.doc.contains(node)
            || !self.native_focusable(node)
            || find_fragment_bounds(&self.fragment, node, 0.0, 0.0).is_none()
        {
            return Err("Accessibility text control is unavailable".into());
        }
        let info = control_info(&self.doc, node).ok_or("Unsupported accessibility text control")?;
        let value = control_value(&self.doc, node, info);
        if value.encode_utf16().count() > MAX_EDIT_TEXT_UTF16 {
            return Err("Accessibility text exceeds the supported control limit".into());
        }
        let editor = self
            .native_editor
            .as_ref()
            .filter(|e| self.focused == Some(node) && e.node == node && e.observed == value)
            .cloned()
            .unwrap_or_else(|| EditorSession::new(node, value));
        Ok((editor, info))
    }

    fn accessibility_visible_clip(&self, geometry: &geometry::Geometry) -> Option<Bounds> {
        geometry::intersection(
            geometry.clip,
            Bounds {
                x: 0.0,
                y: self.scroll_y,
                width: self.viewport_width,
                height: self.viewport_height,
            },
        )
    }

    fn accessibility_text_state(
        &self,
        editor: &EditorSession,
        info: ControlInfo,
    ) -> Result<State, String> {
        let geometry = geometry::Geometry::new(self, editor, info)
            .ok_or("Accessibility text geometry is unavailable")?;
        let style = self
            .styles
            .get(&editor.node)
            .ok_or("Accessibility text style is unavailable")?;
        let color = match style.color {
            Color::Rgba(r, g, b, a) => [r, g, b, a],
            Color::CurrentColor => [0, 0, 0, 255],
        };
        let focused = self.focused == Some(editor.node);
        Ok(State {
            text: (!info.protected).then(|| editor.observed.clone()),
            text_length: length(&editor.observed),
            protected: info.protected,
            writable: info.writable,
            multiline: info.multiline,
            focused,
            selection: focused.then(|| editor.selection()),
            marked: focused
                .then(|| editor.composition.as_ref().map(|c| c.marked))
                .flatten(),
            visible_range: self
                .accessibility_visible_clip(&geometry)
                .map(|clip| geometry.visible_range(clip))
                .unwrap_or_default(),
            insertion_line: focused.then(|| geometry.line_index(editor.cursor)),
            line_count: geometry.line_count(),
            style: AccessibilityTextStyle {
                font_size_px: style.font_size_px,
                bold: style.is_bold(),
                italic: style.is_italic(),
                color,
            },
        })
    }

    pub(crate) fn accessibility_text(
        &mut self,
        context: &Context,
        source: u64,
        action: Action,
    ) -> Result<(bool, ResultValue), String> {
        let (editor, info) = self.accessibility_editor(context, source)?;
        let geometry = geometry::Geometry::new(self, &editor, info)
            .ok_or("Accessibility text geometry is unavailable")?;
        let text_length = length(&editor.observed);
        let set_value = matches!(action, Action::SetValue { .. });
        let reply = match action {
            Action::Inspect => {
                ResultValue::State(Box::new(self.accessibility_text_state(&editor, info)?))
            }
            Action::Bounds { range } => {
                checked_range(&editor.observed, range)?;
                ResultValue::Bounds(
                    self.accessibility_visible_clip(&geometry).and_then(|clip| {
                        geometry::intersection(geometry.range_bounds(range), clip)
                    }),
                )
            }
            Action::LineForIndex { index } => {
                ResultValue::Index((index <= text_length).then(|| geometry.line_index(index)))
            }
            Action::RangeForLine { line } => {
                ResultValue::Range(geometry.range_for_line(line, text_length))
            }
            Action::RangeForIndex { index } => {
                ResultValue::Range(geometry::grapheme_range(&editor.observed, index))
            }
            Action::RangeForPosition { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err("Invalid accessibility text point".into());
                }
                ResultValue::Range(
                    self.accessibility_visible_clip(&geometry)
                        .filter(|clip| {
                            x >= clip.x
                                && x <= clip.x + clip.width
                                && y >= clip.y
                                && y <= clip.y + clip.height
                        })
                        .and_then(|_| geometry.range_for_position(&editor.observed, x, y)),
                )
            }
            Action::ScrollToRange { range } => {
                checked_range(&editor.observed, range)?;
                self.native_text_scroll_target = Some((editor.node, range));
                self.relayout();
                let (current, info) = self.accessibility_editor(context, source)?;
                return Ok((
                    true,
                    ResultValue::State(Box::new(self.accessibility_text_state(&current, info)?)),
                ));
            }
            Action::Select { range } => {
                checked_range(&editor.observed, range)?;
                self.commit_native_composition();
                self.native_focus_at(Some(editor.node));
                let context = self.native_text_input_state(source).context();
                self.native_text_input(&context, source, TextInputAction::Select { range })?;
                let (current, info) = self
                    .live_editor()
                    .ok_or("Accessibility text selection is unavailable")?;
                return Ok((
                    true,
                    ResultValue::State(Box::new(self.accessibility_text_state(&current, info)?)),
                ));
            }
            Action::ReplaceSelection { text } | Action::SetValue { text } => {
                if !info.writable {
                    return Err("Accessibility text control is read-only".into());
                }
                if text.encode_utf16().count() > MAX_EDIT_TEXT_UTF16 {
                    return Err("Accessibility replacement exceeds the supported limit".into());
                }
                // Validation must finish before focus or composition changes.
                let range = if set_value {
                    TextRange {
                        location: 0,
                        length: text_length,
                    }
                } else {
                    editor.selection()
                };
                let text = normalize(text, info.multiline);
                if (text_length - range.length) as usize + text.encode_utf16().count()
                    > MAX_EDIT_TEXT_UTF16
                {
                    return Err("Accessibility replacement exceeds the supported limit".into());
                }
                self.commit_native_composition();
                self.native_focus_at(Some(editor.node));
                let context = self.native_text_input_state(source).context();
                self.native_text_input(
                    &context,
                    source,
                    TextInputAction::Replace {
                        text,
                        replacement: Some(range),
                    },
                )?;
                let (current, info) = self
                    .live_editor()
                    .ok_or("Accessibility text replacement is unavailable")?;
                return Ok((
                    true,
                    ResultValue::State(Box::new(self.accessibility_text_state(&current, info)?)),
                ));
            }
        };
        Ok((false, reply))
    }

    pub(crate) fn accessibility_text_reply(
        &self,
        mut context: Context,
        result: ResultValue,
    ) -> Reply {
        context.frame_generation = self.frame_generation;
        Reply { context, result }
    }
}
