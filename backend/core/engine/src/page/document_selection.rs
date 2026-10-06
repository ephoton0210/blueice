// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public document text, selection and AT geometry belong to the core layout.
use super::*;
use blueice_ipc::accessibility::{
    AccessibilityContext, AccessibilityTextAction as Action,
    AccessibilityTextResult as ResultValue, AccessibilityTextState, AccessibilityTextStyle,
};
use blueice_ipc::input::{
    DocumentSelectionState, PageKey, TextInputAction, TextMovement, TextRange,
    MAX_DOCUMENT_TEXT_UTF16,
};
use blueice_ipc::Bounds;
use blueice_layout::FragmentKind;
use native_editing::geometry::{grapheme_range, intersection, union};
use native_editing::{byte_offset, checked_range};
use std::collections::HashSet;
use unicode_segmentation::UnicodeSegmentation;
use zeroize::Zeroize;

const MAX_RECTS: usize = 65_536;
const MAX_VISITS: usize = 2_097_152;
const MAX_ENCODED_TEXT: usize = 4 * 1024 * 1024;

struct Span {
    start: u32,
    end: u32,
    node: Option<NodeId>,
    line: usize,
    bounds: Bounds,
}

#[derive(Default)]
struct Index {
    text: String,
    length: u32,
    encoded_bytes: usize,
    spans: Vec<Span>,
    source_nodes: Vec<NodeId>,
    limited: bool,
}
impl Drop for Index {
    fn drop(&mut self) {
        self.text.zeroize();
    }
}

#[derive(Default)]
pub(super) struct DocumentSelection {
    index: Index,
    active: bool,
    anchor: u32,
    cursor: u32,
    origin: Option<TextRange>,
    granularity: u8,
    preferred_x: Option<f64>,
}

fn range(anchor: u32, cursor: u32) -> TextRange {
    TextRange {
        location: anchor.min(cursor),
        length: anchor.abs_diff(cursor),
    }
}

fn descendant(page: &Page, node: NodeId, owner: NodeId) -> bool {
    let mut current = Some(node);
    for _ in 0..=256 {
        let Some(id) = current else { return false };
        if id == owner {
            return true;
        }
        current = page.doc.parent(id);
    }
    false
}

fn public_text(page: &Page, node: NodeId) -> bool {
    let mut current = Some(node);
    for _ in 0..=256 {
        let Some(id) = current else {
            return page.accessibility_safe_content(node);
        };
        if matches!(
            native_interaction::tag(&page.doc, id),
            "input" | "textarea" | "button" | "select" | "option"
        ) {
            return false;
        }
        current = page.doc.parent(id);
    }
    false
}

impl Index {
    fn append(&mut self, text: &str, node: Option<NodeId>, line: usize, bounds: Bounds) {
        let length = text.encode_utf16().count() as u32;
        let encoded_bytes = text
            .chars()
            .map(|c| match c {
                '"' | '\\' => 2,
                '\u{0}'..='\u{1f}' => 6,
                _ => c.len_utf8(),
            })
            .sum::<usize>();
        if self.length as usize + length as usize > MAX_DOCUMENT_TEXT_UTF16
            || self.encoded_bytes + encoded_bytes > MAX_ENCODED_TEXT
        {
            self.limited = true;
            return;
        }
        self.spans.push(Span {
            start: self.length,
            end: self.length + length,
            node,
            line,
            bounds,
        });
        self.text.push_str(text);
        self.length += length;
        self.encoded_bytes += encoded_bytes;
    }

    fn build(page: &Page) -> Self {
        enum Visit<'a> {
            Enter(&'a Fragment, f64, f64, usize),
            Break,
        }
        let mut index = Self::default();
        let mut pending = vec![Visit::Enter(&page.fragment, 0.0, 0.0, 0)];
        let mut visited = 0;
        let mut line = 0;
        let mut boundary = false;
        while let Some(visit) = pending.pop() {
            let (fragment, x, y, depth) = match visit {
                Visit::Break => {
                    boundary = true;
                    continue;
                }
                Visit::Enter(f, x, y, d) => (f, x, y, d),
            };
            visited += 1;
            if visited > MAX_VISITS {
                index.limited = true;
                break;
            }
            if depth > 256 {
                index.limited = true;
                boundary = true;
                continue;
            }
            let x = x + fragment.x;
            let y = y + fragment.y;
            if fragment.node.is_some_and(|node| !public_text(page, node)) {
                boundary = true;
                continue;
            }
            match &fragment.kind {
                FragmentKind::NativeControl { .. } | FragmentKind::NativeText { .. } => {
                    boundary = true;
                    continue;
                }
                FragmentKind::Block => {
                    boundary = true;
                    pending.push(Visit::Break);
                }
                FragmentKind::Line => line += 1,
                FragmentKind::Text(text) => {
                    let Some(node) = fragment.node else { continue };
                    let Some(style) = page.styles.get(&node) else {
                        continue;
                    };
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(previous) = index.spans.last() {
                        let previous_end = previous.bounds.x + previous.bounds.width;
                        let bounds = Bounds {
                            x: previous_end,
                            y: previous.bounds.y,
                            width: if !boundary && (previous.bounds.y - y).abs() < 0.01 {
                                (x - previous_end).max(0.0)
                            } else {
                                0.0
                            },
                            height: previous.bounds.height,
                        };
                        index.append(
                            if boundary { "\n" } else { " " },
                            None,
                            previous.line,
                            bounds,
                        );
                    }
                    boundary = false;
                    let mut pen = x;
                    for cluster in text.graphemes(true) {
                        let width = blueice_font::measure_text_width(
                            cluster,
                            style.font_size_px,
                            style.is_bold(),
                            style.is_italic(),
                        );
                        index.append(
                            cluster,
                            Some(node),
                            line,
                            Bounds {
                                x: pen,
                                y,
                                width,
                                height: fragment.height,
                            },
                        );
                        pen += width;
                        if index.limited {
                            break;
                        }
                    }
                }
            }
            if index.limited {
                break;
            }
            pending.extend(
                fragment
                    .children
                    .iter()
                    .rev()
                    .take(MAX_VISITS)
                    .map(|child| Visit::Enter(child, x, y, depth + 1)),
            );
        }
        // Ordinary layout runs carry style-owner IDs. Keep the corresponding
        // DOM text identities as well, so replacement with identical text
        // cannot reuse a selection belonging to removed source nodes.
        let owners = index
            .spans
            .iter()
            .filter_map(|span| span.node)
            .collect::<HashSet<_>>();
        let mut nodes = vec![(page.doc.root(), 0)];
        let mut visits = 0;
        while let Some((node, depth)) = nodes.pop() {
            visits += 1;
            if visits > MAX_VISITS || depth > 256 {
                index.limited = true;
                break;
            }
            if !public_text(page, node) {
                continue;
            }
            if matches!(page.doc.data(node), NodeData::Text { .. }) {
                let mut parent = page.doc.parent(node);
                while let Some(id) = parent {
                    if owners.contains(&id) {
                        index.source_nodes.push(node);
                        break;
                    }
                    parent = page.doc.parent(id);
                }
            }
            nodes.extend(
                page.doc
                    .children(node)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(|id| (id, depth + 1)),
            );
        }
        index
    }

    fn scope(&self, page: &Page, node: NodeId) -> Option<TextRange> {
        let mut first = None;
        let mut end = 0;
        for span in &self.spans {
            if span
                .node
                .is_some_and(|source| descendant(page, source, node))
            {
                first.get_or_insert(span.start);
                end = span.end;
            }
        }
        first.map(|start| TextRange {
            location: start,
            length: end - start,
        })
    }

    fn substring(&self, range: TextRange) -> Option<&str> {
        let bytes = checked_range(&self.text, range).ok()?;
        Some(&self.text[bytes])
    }

    fn caret(&self, offset: u32) -> Option<Bounds> {
        let span = self
            .spans
            .iter()
            .find(|span| span.start <= offset && offset < span.end)
            .or_else(|| self.spans.last())?;
        Some(Bounds {
            x: if offset >= span.end {
                span.bounds.x + span.bounds.width
            } else {
                span.bounds.x
            },
            width: 1.0,
            ..span.bounds
        })
    }

    fn rects(&self, range: TextRange) -> (Vec<Bounds>, bool) {
        let mut rects: Vec<Bounds> = Vec::new();
        for span in self.spans.iter().filter(|span| {
            span.end > range.location
                && span.start < range.end().unwrap_or(0)
                && span.bounds.width > 0.0
        }) {
            if let Some(last) = rects.last_mut() {
                if (last.y - span.bounds.y).abs() < 0.01
                    && (last.height - span.bounds.height).abs() < 0.01
                    && (span.bounds.x - last.x - last.width).abs() < 0.01
                {
                    last.width += span.bounds.width;
                    continue;
                }
            }
            if rects.len() == MAX_RECTS {
                return (rects, true);
            }
            rects.push(span.bounds);
        }
        (rects, false)
    }

    fn nearest(&self, x: f64, y: f64, scope: TextRange) -> Option<u32> {
        let mut best = None;
        let mut distance = (f64::INFINITY, f64::INFINITY);
        for span in self.spans.iter().filter(|s| {
            s.node.is_some() && s.start >= scope.location && s.end <= scope.end().unwrap_or(0)
        }) {
            let dy = (y - span.bounds.y - span.bounds.height / 2.0).abs();
            for (offset, cx) in [
                (span.start, span.bounds.x),
                (span.end, span.bounds.x + span.bounds.width),
            ] {
                let d = (dy, (x - cx).abs());
                if d.0
                    .total_cmp(&distance.0)
                    .then_with(|| d.1.total_cmp(&distance.1))
                    .is_lt()
                {
                    distance = d;
                    best = Some(offset);
                }
            }
        }
        best
    }

    fn word(&self, offset: u32) -> TextRange {
        let mut start = 0;
        for word in self.text.split_word_bounds() {
            let end = start + word.encode_utf16().count() as u32;
            if start <= offset && offset < end {
                return TextRange {
                    location: start,
                    length: end - start,
                };
            }
            start = end;
        }
        TextRange {
            location: self.length,
            length: 0,
        }
    }

    fn paragraph(&self, offset: u32) -> TextRange {
        let byte = byte_offset(&self.text, offset).unwrap_or(self.text.len());
        let start = self.text[..byte].rfind('\n').map_or(0, |b| b + 1);
        let end = self.text[byte..]
            .find('\n')
            .map_or(self.text.len(), |b| byte + b);
        let location = self.text[..start].encode_utf16().count() as u32;
        TextRange {
            location,
            length: self.text[start..end].encode_utf16().count() as u32,
        }
    }

    fn word_movement(&self, offset: u32, forward: bool) -> u32 {
        let mut cursor = 0;
        let mut previous = 0;
        for segment in self.text.split_word_bounds() {
            let end = cursor + segment.encode_utf16().count() as u32;
            if segment.unicode_words().next().is_some() {
                if forward && end > offset {
                    return end;
                }
                if !forward && cursor < offset {
                    previous = cursor;
                }
            }
            cursor = end;
        }
        if forward {
            self.length
        } else {
            previous
        }
    }

    fn lines(&self, scope: TextRange) -> Vec<TextRange> {
        let mut lines: Vec<(usize, u32, u32)> = Vec::new();
        for span in self.spans.iter().filter(|s| {
            s.node.is_some() && s.start >= scope.location && s.end <= scope.end().unwrap_or(0)
        }) {
            if let Some(last) = lines.last_mut().filter(|last| last.0 == span.line) {
                last.2 = span.end;
            } else {
                lines.push((span.line, span.start, span.end));
            }
        }
        lines
            .iter()
            .enumerate()
            .map(|(i, (_, start, end))| TextRange {
                location: *start,
                length: lines.get(i + 1).map_or(*end, |next| next.1) - start,
            })
            .collect()
    }
}

impl Page {
    pub(super) fn rebuild_document_selection(&mut self) {
        let next = Index::build(self);
        let old = &self.document_selection.index;
        let same = old.text == next.text
            && old.source_nodes == next.source_nodes
            && old.spans.len() == next.spans.len()
            && old
                .spans
                .iter()
                .zip(&next.spans)
                .all(|(a, b)| a.node == b.node && a.start == b.start && a.end == b.end);
        if !same {
            self.clear_document_selection();
        }
        self.document_selection.index = next;
    }
    pub(super) fn clear_document_selection(&mut self) {
        let selection = &mut self.document_selection;
        selection.active = false;
        selection.anchor = 0;
        selection.cursor = 0;
        selection.origin = None;
        selection.preferred_x = None;
        selection.granularity = 1;
    }
    pub(super) fn document_selection_active(&self) -> bool {
        self.document_selection.active
    }
    pub(super) fn document_text_is_public(&self, node: NodeId) -> bool {
        public_text(self, node)
    }
    pub(super) fn document_selection_state(&self) -> DocumentSelectionState {
        let selection = &self.document_selection;
        let range = range(selection.anchor, selection.cursor);
        DocumentSelectionState {
            version: 1,
            text_length: selection.index.length,
            active: selection.active,
            selection: range,
            selected_text: selection.active.then(|| {
                selection
                    .index
                    .substring(range)
                    .unwrap_or_default()
                    .to_string()
            }),
            limited: selection.index.limited || selection.index.rects(range).1,
        }
    }
    pub(super) fn paint_document_selection(&self, frame: &mut Frame) {
        if !self.document_selection.active {
            return;
        }
        for bounds in self
            .document_selection
            .index
            .rects(range(
                self.document_selection.anchor,
                self.document_selection.cursor,
            ))
            .0
        {
            frame.commands.push(PaintCommand::Rect {
                rect: Rect {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                },
                color: Color::Rgba(64, 128, 255, 96),
            });
        }
    }
    fn reveal_document_cursor(&mut self) {
        if let Some(bounds) = self
            .document_selection
            .index
            .caret(self.document_selection.cursor)
        {
            if bounds.y < self.scroll_y {
                self.scroll_y = bounds.y;
            } else if bounds.y + bounds.height > self.scroll_y + self.viewport_height {
                self.scroll_y = (bounds.y + bounds.height - self.viewport_height).max(0.0);
            }
            self.scroll_y = self
                .scroll_y
                .clamp(0.0, (self.fragment.height - self.viewport_height).max(0.0));
        }
    }
    pub(super) fn document_input_action(
        &mut self,
        action: &TextInputAction,
    ) -> Option<Result<bool, String>> {
        let active = self.document_selection.active;
        let no_control = self.focused.is_none();
        let movement = match action {
            TextInputAction::Key { key, shift } if active => {
                let direction = match key {
                    PageKey::ArrowLeft => Some(TextMovement::Backward),
                    PageKey::ArrowRight => Some(TextMovement::Forward),
                    PageKey::ArrowUp => Some(TextMovement::Up),
                    PageKey::ArrowDown => Some(TextMovement::Down),
                    PageKey::Home => Some(TextMovement::Beginning),
                    PageKey::End => Some(TextMovement::End),
                    _ => None,
                };
                direction.map(|direction| (direction, *shift))
            }
            TextInputAction::Move { direction, extend } if active || no_control => {
                Some((*direction, *extend))
            }
            _ => None,
        };
        if let Some((direction, extend)) = movement {
            let s = &mut self.document_selection;
            if s.index.text.is_empty() {
                return Some(Ok(false));
            }
            let old = range(s.anchor, s.cursor);
            let collapsed = !extend && old.length > 0;
            let offsets = std::iter::once(0)
                .chain(s.index.text.graphemes(true).scan(0, |offset, text| {
                    *offset += text.encode_utf16().count() as u32;
                    Some(*offset)
                }))
                .collect::<Vec<_>>();
            let target = match direction {
                TextMovement::Beginning => 0,
                TextMovement::End => s.index.length,
                TextMovement::Backward if collapsed => old.location,
                TextMovement::Forward if collapsed => old.end().unwrap(),
                TextMovement::Backward => offsets
                    .iter()
                    .copied()
                    .take_while(|&o| o < s.cursor)
                    .last()
                    .unwrap_or(0),
                TextMovement::Forward => offsets
                    .iter()
                    .copied()
                    .find(|&o| o > s.cursor)
                    .unwrap_or(s.index.length),
                TextMovement::WordBackward => s.index.word_movement(s.cursor, false),
                TextMovement::WordForward => s.index.word_movement(s.cursor, true),
                TextMovement::LineBeginning
                | TextMovement::LineEnd
                | TextMovement::Up
                | TextMovement::Down => {
                    let lines = s.index.lines(TextRange {
                        location: 0,
                        length: s.index.length,
                    });
                    let i = lines
                        .iter()
                        .rposition(|r| r.location <= s.cursor)
                        .unwrap_or(0);
                    match direction {
                        TextMovement::LineBeginning => lines[i].location,
                        TextMovement::LineEnd => lines[i].end().unwrap(),
                        _ => {
                            let next = if direction == TextMovement::Down {
                                (i + 1).min(lines.len() - 1)
                            } else {
                                i.saturating_sub(1)
                            };
                            let x = *s.preferred_x.get_or_insert_with(|| {
                                s.index.caret(s.cursor).map_or(0.0, |c| c.x)
                            });
                            let y = s
                                .index
                                .caret(lines[next].location)
                                .map_or(0.0, |c| c.y + c.height / 2.0);
                            s.index.nearest(x, y, lines[next]).unwrap_or(s.cursor)
                        }
                    }
                }
            };
            s.cursor = target;
            if !extend {
                s.anchor = target;
            }
            s.active = true;
            s.origin = None;
            if !matches!(direction, TextMovement::Up | TextMovement::Down) {
                s.preferred_x = None;
            }
            self.reveal_document_cursor();
            return Some(Ok(true));
        }
        match action {
            TextInputAction::Pointer {
                x,
                y,
                extend,
                click_count,
            }
            | TextInputAction::DocumentPointer {
                x,
                y,
                extend,
                click_count,
            } => {
                if !x.is_finite() || !y.is_finite() || x.abs() > 1e9 || y.abs() > 1e9 {
                    return Some(Err("Invalid document selection point".into()));
                }
                if matches!(action, TextInputAction::Pointer { .. })
                    && self.focused.is_some()
                    && !active
                {
                    return None;
                }
                if !extend
                    && self
                        .click_target(*x, *y)
                        .is_some_and(|node| !public_text(self, node))
                {
                    return Some(Err(
                        "Document selection point belongs to excluded content".into()
                    ));
                }
                let s = &self.document_selection;
                let Some(target) = s.index.nearest(
                    *x,
                    *y + self.scroll_y,
                    TextRange {
                        location: 0,
                        length: s.index.length,
                    },
                ) else {
                    return Some(Ok(false));
                };
                if !*extend {
                    self.native_focus_at(None);
                }
                let s = &mut self.document_selection;
                if *extend && s.active {
                    if let Some(origin) = s.origin {
                        let chosen = if s.granularity == 3 {
                            s.index.paragraph(target)
                        } else {
                            s.index.word(target)
                        };
                        if target < origin.location {
                            s.anchor = origin.end().unwrap();
                            s.cursor = chosen.location;
                        } else {
                            s.anchor = origin.location;
                            s.cursor = chosen.end().unwrap();
                        }
                    } else {
                        s.cursor = target;
                    }
                } else {
                    let chosen = match click_count {
                        3.. => s.index.paragraph(target),
                        2 => s.index.word(target),
                        _ => TextRange {
                            location: target,
                            length: 0,
                        },
                    };
                    s.anchor = chosen.location;
                    s.cursor = chosen.end().unwrap();
                    s.granularity = *click_count;
                    s.origin = (*click_count > 1).then_some(chosen);
                }
                s.active = true;
                s.preferred_x = None;
                self.reveal_document_cursor();
                Some(Ok(true))
            }
            TextInputAction::SelectAll | TextInputAction::DocumentSelectAll
                if active || no_control || matches!(action, TextInputAction::DocumentSelectAll) =>
            {
                let s = &mut self.document_selection;
                s.active = true;
                s.anchor = 0;
                s.cursor = s.index.length;
                s.origin = None;
                s.preferred_x = None;
                Some(Ok(true))
            }
            TextInputAction::Select { range: requested } if active || no_control => {
                if let Err(error) = checked_range(&self.document_selection.index.text, *requested) {
                    return Some(Err(error));
                }
                let s = &mut self.document_selection;
                s.active = true;
                s.anchor = requested.location;
                s.cursor = requested.end().unwrap();
                s.origin = None;
                s.preferred_x = None;
                self.reveal_document_cursor();
                Some(Ok(true))
            }
            TextInputAction::Replace { .. }
            | TextInputAction::Compose { .. }
            | TextInputAction::FinishComposition
            | TextInputAction::CancelComposition
            | TextInputAction::Delete { .. }
            | TextInputAction::Undo
            | TextInputAction::Redo
            | TextInputAction::SelectKey { .. }
            | TextInputAction::SelectOption { .. }
            | TextInputAction::SelectPointer { .. }
            | TextInputAction::SelectScroll { .. }
                if active =>
            {
                Some(Err("Document text is read-only".into()))
            }
            TextInputAction::Key {
                key: PageKey::Escape,
                ..
            } if active => {
                self.clear_document_selection();
                Some(Ok(true))
            }
            TextInputAction::Key {
                key: PageKey::Enter,
                ..
            } if active => Some(Ok(false)),
            TextInputAction::Key {
                key: PageKey::Space | PageKey::PageDown | PageKey::PageUp,
                shift,
            } if active => {
                let backwards = matches!(
                    action,
                    TextInputAction::Key {
                        key: PageKey::PageUp,
                        ..
                    }
                ) || *shift;
                self.scroll_by(self.viewport_height * 0.9 * if backwards { -1.0 } else { 1.0 });
                Some(Ok(true))
            }
            _ => None,
        }
    }

    pub(super) fn accessibility_document_text(
        &mut self,
        context: &AccessibilityContext,
        source: u64,
        action: &Action,
    ) -> Option<Result<(bool, ResultValue), String>> {
        let node = NodeId::from_u64(context.node_id);
        if self.doc.contains(node)
            && matches!(
                native_interaction::tag(&self.doc, node),
                "input" | "textarea" | "select" | "button"
            )
        {
            return None;
        }
        Some((|| {
            if context.version != 1
                || context.frame_source != source
                || context.document_generation != self.document_generation
                || context.frame_generation != self.frame_generation
            {
                return Err("Stale or unsupported document text context".into());
            }
            if !self.doc.contains(node) || !public_text(self, node) {
                return Err("Document text target is unavailable".into());
            }
            let scope = self
                .document_selection
                .index
                .scope(self, node)
                .ok_or("Document text target has no public layout text")?;
            let text = self.document_selection.index.substring(scope).unwrap();
            let index = &self.document_selection.index;
            let lines = index.lines(scope);
            let local = |global: TextRange| TextRange {
                location: global.location - scope.location,
                length: global.length,
            };
            let global = |local: TextRange| TextRange {
                location: scope.location + local.location,
                length: local.length,
            };
            let clip = Bounds {
                x: 0.0,
                y: self.scroll_y,
                width: self.viewport_width,
                height: self.viewport_height,
            };
            let result = match action {
                Action::Inspect => None,
                Action::SetValue { .. } | Action::ReplaceSelection { .. } => {
                    return Err("Document text is read-only".into())
                }
                Action::Bounds { range } => {
                    checked_range(text, *range)?;
                    let rects = if range.length == 0 {
                        index
                            .caret(scope.location + range.location)
                            .into_iter()
                            .collect()
                    } else {
                        index.rects(global(*range)).0
                    };
                    Some(ResultValue::Bounds(union(
                        &rects
                            .into_iter()
                            .filter_map(|b| intersection(b, clip))
                            .collect::<Vec<_>>(),
                    )))
                }
                Action::RangeForIndex { index: offset } => {
                    Some(ResultValue::Range(grapheme_range(text, *offset)))
                }
                Action::RangeForLine { line } => Some(ResultValue::Range(
                    lines.get(*line as usize).copied().map(local),
                )),
                Action::LineForIndex { index: offset } => {
                    Some(ResultValue::Index((*offset <= scope.length).then(|| {
                        lines
                            .iter()
                            .rposition(|r| r.location <= scope.location + offset)
                            .unwrap_or(0) as u32
                    })))
                }
                Action::RangeForPosition { x, y } => {
                    if !x.is_finite() || !y.is_finite() {
                        return Err("Invalid document text point".into());
                    }
                    let offset = index
                        .spans
                        .iter()
                        .find(|span| {
                            span.node.is_some()
                                && span.start >= scope.location
                                && span.end <= scope.end().unwrap()
                                && intersection(span.bounds, clip).is_some_and(|b| {
                                    *x >= b.x
                                        && *x <= b.x + b.width
                                        && *y >= b.y
                                        && *y <= b.y + b.height
                                })
                        })
                        .map(|span| span.start - scope.location);
                    Some(ResultValue::Range(
                        offset.and_then(|o| grapheme_range(text, o)),
                    ))
                }
                Action::Select { range } => {
                    checked_range(text, *range)?;
                    let requested = global(*range);
                    let s = &mut self.document_selection;
                    s.active = true;
                    s.anchor = requested.location;
                    s.cursor = requested.end().unwrap();
                    s.origin = None;
                    s.preferred_x = None;
                    self.reveal_document_cursor();
                    return Ok((
                        true,
                        ResultValue::State(Box::new(
                            self.document_accessibility_state(node, scope)?,
                        )),
                    ));
                }
                Action::ScrollToRange { range } => {
                    checked_range(text, *range)?;
                    let bounds = index
                        .caret(scope.location + range.location)
                        .ok_or("Document range has no geometry")?;
                    self.scroll_y = (bounds.y - (self.viewport_height - bounds.height) / 2.0)
                        .clamp(0.0, (self.fragment.height - self.viewport_height).max(0.0));
                    return Ok((
                        true,
                        ResultValue::State(Box::new(
                            self.document_accessibility_state(node, scope)?,
                        )),
                    ));
                }
            };
            Ok((
                false,
                match result {
                    Some(result) => result,
                    None => ResultValue::State(Box::new(
                        self.document_accessibility_state(node, scope)?,
                    )),
                },
            ))
        })())
    }

    fn document_accessibility_state(
        &self,
        node: NodeId,
        scope: TextRange,
    ) -> Result<AccessibilityTextState, String> {
        let s = &self.document_selection;
        let selected = range(s.anchor, s.cursor);
        let focused = s.active
            && if selected.length == 0 {
                s.cursor >= scope.location && s.cursor <= scope.end().unwrap()
            } else {
                selected.location < scope.end().unwrap() && selected.end().unwrap() > scope.location
            };
        let selection = focused.then(|| {
            let start = selected
                .location
                .max(scope.location)
                .min(scope.end().unwrap());
            let end = selected.end().unwrap().min(scope.end().unwrap()).max(start);
            TextRange {
                location: start - scope.location,
                length: end - start,
            }
        });
        let lines = s.index.lines(scope);
        let clip = Bounds {
            x: 0.0,
            y: self.scroll_y,
            width: self.viewport_width,
            height: self.viewport_height,
        };
        let visible = s
            .index
            .spans
            .iter()
            .filter(|span| {
                span.node.is_some()
                    && span.start >= scope.location
                    && span.end <= scope.end().unwrap()
                    && intersection(span.bounds, clip).is_some()
            })
            .collect::<Vec<_>>();
        let start = visible.first().map_or(scope.location, |s| s.start);
        let end = visible.last().map_or(start, |s| s.end);
        let style = self
            .styles
            .get(&node)
            .or_else(|| {
                s.index
                    .spans
                    .iter()
                    .filter_map(|s| s.node)
                    .find_map(|n| self.styles.get(&n))
            })
            .ok_or("Document text style is unavailable")?;
        let color = match style.color {
            Color::Rgba(r, g, b, a) => [r, g, b, a],
            Color::CurrentColor => [0, 0, 0, 255],
        };
        Ok(AccessibilityTextState {
            document: true,
            text: Some(s.index.substring(scope).unwrap().into()),
            text_length: scope.length,
            protected: false,
            writable: false,
            multiline: lines.len() > 1,
            focused,
            selection,
            marked: None,
            visible_range: TextRange {
                location: start - scope.location,
                length: end - start,
            },
            insertion_line: focused.then(|| {
                lines
                    .iter()
                    .rposition(|r| r.location <= s.cursor)
                    .unwrap_or(0) as u32
            }),
            line_count: lines.len().max(1) as u32,
            style: AccessibilityTextStyle {
                font_size_px: style.font_size_px,
                bold: style.is_bold(),
                italic: style.is_italic(),
                color,
            },
        })
    }
}
