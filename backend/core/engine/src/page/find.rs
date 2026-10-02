// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Search the layout's text runs, retaining their exact document-space geometry.
//! Block boundaries cannot create matches; soft line wrapping can. Native values
//! are searched only through painted, public runs, never by serializing the DOM.

use super::*;
use blueice_ipc::find::{
    FindAction, FindState, MAX_FIND_MATCHES, MAX_FIND_QUERY_BYTES, MAX_FIND_RECTS,
};
use blueice_ipc::Bounds;
use blueice_layout::{FragmentKind, NativeForm};
use regex::{Regex, RegexBuilder};
use std::ops::Range;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;
const MAX_INDEX_SEGMENTS: usize = 200_000;

#[derive(Default)]
pub(super) struct FindSession {
    query: String,
    case_sensitive: bool,
    pattern: Option<Regex>,
    index: Index,
    matches: Vec<Range<usize>>,
    active: Option<usize>,
    wrapped: bool,
    limited: bool,
    revision: u64,
}

#[derive(Default)]
struct Index {
    text: String,
    segments: Vec<Segment>,
    limited: bool,
}
struct Segment {
    start: usize,
    end: usize,
    bounds: Bounds,
}

fn normalized_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfc()
        .collect()
}

fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let width = (a.x + a.width).min(b.x + b.width) - x;
    let height = (a.y + a.height).min(b.y + b.height) - y;
    (width > 0.0 && height > 0.0).then_some(Bounds {
        x,
        y,
        width,
        height,
    })
}

impl Index {
    fn boundary(&mut self) {
        if !self.text.ends_with('\n') && self.text.len() < MAX_INDEX_BYTES {
            self.text.push('\n');
        }
    }
    fn word_space(&mut self) {
        if !self.text.is_empty()
            && !self.text.ends_with(['\n', ' '])
            && self.text.len() < MAX_INDEX_BYTES
        {
            self.text.push(' ');
        }
    }
    fn run(&mut self, text: &str, bounds: Bounds, style: &ComputedStyle, clip: Option<Bounds>) {
        let mut x = bounds.x;
        for cluster in text.graphemes(true) {
            let width = blueice_font::measure_text_width(
                cluster,
                style.font_size_px,
                style.is_bold(),
                style.is_italic(),
            );
            let raw = Bounds { x, width, ..bounds };
            x += width;
            let shown: String = if cluster.chars().all(char::is_whitespace) {
                " ".into()
            } else {
                cluster.nfc().collect()
            };
            if shown == " " && self.text.ends_with(' ') {
                continue;
            }
            if self.text.len() + shown.len() > MAX_INDEX_BYTES
                || self.segments.len() == MAX_INDEX_SEGMENTS
            {
                self.limited = true;
                return;
            }
            let visible = clip.map_or(Some(raw), |clip| intersection(raw, clip));
            if let Some(bounds) = visible {
                let start = self.text.len();
                self.text.push_str(&shown);
                self.segments.push(Segment {
                    start,
                    end: self.text.len(),
                    bounds,
                });
            } else {
                // A clipped run cannot join text on its two sides into a match.
                self.boundary();
            }
        }
    }
    fn rects(&self, range: &Range<usize>) -> Vec<Bounds> {
        let start = self
            .segments
            .partition_point(|part| part.end <= range.start);
        let mut rects: Vec<Bounds> = Vec::new();
        for part in self.segments[start..]
            .iter()
            .take_while(|part| part.start < range.end)
        {
            if let Some(last) = rects.last_mut() {
                if (last.y - part.bounds.y).abs() < 0.01
                    && (last.height - part.bounds.height).abs() < 0.01
                    && part.bounds.x >= last.x
                {
                    last.width = part.bounds.x + part.bounds.width - last.x;
                    continue;
                }
            }
            if rects.len() == MAX_FIND_RECTS {
                break;
            }
            rects.push(part.bounds);
        }
        rects
    }
}

fn searchable(page: &Page, node: NodeId) -> bool {
    let mut ancestor = Some(node);
    while let Some(node) = ancestor {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = page.doc.data(node)
        {
            if matches!(
                tag_name.as_str(),
                "head" | "script" | "style" | "template" | "noscript"
            ) || attributes
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("hidden"))
            {
                return false;
            }
            if tag_name == "input" {
                let kind = element_attribute(&page.doc, node, "type")
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                let protected =
                    element_attribute(&page.doc, node, "autocomplete").is_some_and(|value| {
                        value.split_ascii_whitespace().any(|token| {
                            token.to_ascii_lowercase().starts_with("cc-")
                                || token.eq_ignore_ascii_case("one-time-code")
                        })
                    });
                if protected
                    || matches!(
                        kind.as_str(),
                        "password" | "payment" | "creditcard" | "hidden"
                    )
                {
                    return false;
                }
            }
        }
        if let Some(style) = page.styles.get(&node) {
            if style.opacity() == 0.0 || style.display == "none" {
                return false;
            }
        }
        ancestor = page.doc.parent(node);
    }
    true
}

fn collect(
    page: &Page,
    fragment: &Fragment,
    x: f64,
    y: f64,
    clip: Option<Bounds>,
    index: &mut Index,
) {
    if index.limited {
        return;
    }
    if fragment.node.is_some_and(|node| !searchable(page, node)) {
        index.boundary();
        return;
    }
    let x = x + fragment.x;
    let y = y + fragment.y;
    let bounds = Bounds {
        x,
        y,
        width: fragment.width,
        height: fragment.height,
    };
    let style = fragment.node.and_then(|node| page.styles.get(&node));
    match &fragment.kind {
        FragmentKind::Text(text) => {
            index.word_space();
            if let Some(style) = style {
                index.run(text, bounds, style, clip);
            }
        }
        FragmentKind::NativeText { text, .. } => {
            if let Some(style) = style {
                index.run(text, bounds, style, clip);
            }
        }
        FragmentKind::Block => {
            index.boundary();
            for child in &fragment.children {
                collect(page, child, x, y, clip, index);
            }
            index.boundary();
        }
        FragmentKind::Line => {
            for child in &fragment.children {
                collect(page, child, x, y, clip, index);
            }
        }
        FragmentKind::NativeControl {
            content_x,
            content_y,
            content_width,
            content_height,
            form,
        } => {
            index.boundary();
            let content = Bounds {
                x: x + content_x,
                y: y + content_y,
                width: *content_width,
                height: *content_height,
            };
            let content = clip.map_or(Some(content), |clip| intersection(content, clip));
            if let Some(content) = content {
                if let (Some(NativeForm::Button(text) | NativeForm::Select(text)), Some(style)) =
                    (form, style)
                {
                    let width = blueice_font::measure_text_width(
                        text,
                        style.font_size_px,
                        style.is_bold(),
                        style.is_italic(),
                    );
                    let bounds = Bounds {
                        x: content.x,
                        y: content.y,
                        width,
                        height: style.font_size_px * 1.2,
                    };
                    index.run(text, bounds, style, Some(content));
                } else if form.is_none() {
                    for (row, child) in fragment.children.iter().enumerate() {
                        if row > 0 {
                            index.word_space();
                        }
                        collect(page, child, x, y, Some(content), index);
                    }
                }
            }
            index.boundary();
        }
    }
}

impl Page {
    pub(crate) fn rebuild_find(&mut self) {
        let mut find = std::mem::take(&mut self.find);
        let old_text = std::mem::take(&mut find.index).text;
        find.index = Index::default();
        find.matches.clear();
        if let Some(pattern) = &find.pattern {
            collect(self, &self.fragment, 0.0, 0.0, None, &mut find.index);
            for found in pattern
                .find_iter(&find.index.text)
                .take(MAX_FIND_MATCHES + 1)
            {
                find.matches.push(found.range());
            }
        }
        find.limited = find.index.limited || find.matches.len() > MAX_FIND_MATCHES;
        find.matches.truncate(MAX_FIND_MATCHES);
        find.active = if find.matches.is_empty() {
            None
        } else {
            Some(find.active.unwrap_or(0).min(find.matches.len() - 1))
        };
        if old_text != find.index.text {
            find.wrapped = false;
        }
        find.revision = find.revision.wrapping_add(1);
        self.find = find;
    }

    pub(crate) fn find_action(&mut self, action: FindAction) -> Result<(), String> {
        match action {
            FindAction::Update {
                query,
                case_sensitive,
            } => {
                if query.len() > MAX_FIND_QUERY_BYTES {
                    return Err("Find query is too long (maximum 1024 UTF-8 bytes)".into());
                }
                let normalized = normalized_query(&query);
                let pattern = if normalized.is_empty() {
                    None
                } else {
                    Some(
                        RegexBuilder::new(&regex::escape(&normalized))
                            .case_insensitive(!case_sensitive)
                            .build()
                            .map_err(|_| "Find query cannot be compiled")?,
                    )
                };
                self.find.query = query;
                self.find.case_sensitive = case_sensitive;
                self.find.pattern = pattern;
                self.find.active = None;
                self.find.wrapped = false;
                self.rebuild_find();
            }
            FindAction::Next { backwards } => {
                let count = self.find.matches.len();
                if let Some(current) = self.find.active {
                    self.find.wrapped = if backwards {
                        current == 0
                    } else {
                        current + 1 == count
                    };
                    self.find.active = Some(if backwards {
                        (current + count - 1) % count
                    } else {
                        (current + 1) % count
                    });
                }
                self.find.revision = self.find.revision.wrapping_add(1);
            }
            FindAction::Close => self.clear_find(),
        }
        if let Some(bounds) = self.find.active.and_then(|index| {
            self.find
                .index
                .rects(&self.find.matches[index])
                .first()
                .copied()
        }) {
            if bounds.y < self.scroll_y
                || bounds.y + bounds.height > self.scroll_y + self.viewport_height
            {
                self.scroll_y = (bounds.y - (self.viewport_height - bounds.height) / 2.0)
                    .clamp(0.0, (self.fragment.height - self.viewport_height).max(0.0));
            }
        }
        Ok(())
    }

    pub(crate) fn clear_find(&mut self) {
        let revision = self.find.revision.wrapping_add(1);
        self.find = FindSession {
            revision,
            ..FindSession::default()
        };
    }

    pub(crate) fn find_state(&self, source: u64, tab: u64) -> FindState {
        FindState {
            tab_id: tab,
            frame_source: source,
            document_generation: self.document_generation,
            revision: self.find.revision,
            query: self.find.query.clone(),
            case_sensitive: self.find.case_sensitive,
            match_count: self.find.matches.len() as u32,
            active_match: self.find.active.map(|index| index as u32 + 1),
            wrapped: self.find.wrapped,
            limited: self.find.limited,
            rects: self
                .find
                .active
                .map(|index| self.find.index.rects(&self.find.matches[index]))
                .unwrap_or_default(),
        }
    }

    pub(crate) fn paint_find(&self, frame: &mut Frame) {
        for (index, range) in self.find.matches.iter().enumerate() {
            for bounds in self.find.index.rects(range) {
                if Some(index) == self.find.active {
                    frame.commands.extend(highlight_border_commands(bounds));
                } else {
                    frame.commands.push(PaintCommand::Rect {
                        rect: Rect {
                            x: bounds.x,
                            y: bounds.y,
                            width: bounds.width,
                            height: bounds.height,
                        },
                        color: Color::Rgba(255, 230, 0, 90),
                    });
                }
            }
        }
    }
}
