// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Caret and selection geometry derives from the exact native layout runs.

use super::*;
use blueice_ipc::{
    input::{TextCaret, MAX_EDIT_CARETS},
    Bounds,
};
use blueice_layout::FragmentKind;

struct Row {
    start: u32,
    end: u32,
    carets: Vec<TextCaret>,
}

pub(super) struct Geometry {
    pub bounds: Bounds,
    pub clip: Bounds,
    rows: Vec<Row>,
}

impl Geometry {
    pub fn new(page: &Page, editor: &EditorSession, info: ControlInfo) -> Option<Self> {
        let bounds = find_fragment_bounds(&page.fragment, editor.node, 0.0, 0.0)?;
        fn content_box(fragment: &Fragment, node: NodeId, x: f64, y: f64) -> Option<Bounds> {
            let x = x + fragment.x;
            let y = y + fragment.y;
            if fragment.node == Some(node) {
                if let FragmentKind::NativeControl {
                    content_x,
                    content_y,
                    content_width,
                    content_height,
                    ..
                } = fragment.kind
                {
                    return Some(Bounds {
                        x: x + content_x,
                        y: y + content_y,
                        width: content_width,
                        height: content_height,
                    });
                }
            }
            fragment
                .children
                .iter()
                .find_map(|child| content_box(child, node, x, y))
        }
        let clip = content_box(&page.fragment, editor.node, 0.0, 0.0)?;
        let style = page.styles.get(&editor.node)?;
        let mut runs = Vec::new();
        fn collect(
            fragment: &Fragment,
            node: NodeId,
            x: f64,
            y: f64,
            runs: &mut Vec<(u32, u32, f64, f64, f64)>,
        ) {
            let x = x + fragment.x;
            let y = y + fragment.y;
            if fragment.node == Some(node) {
                if let FragmentKind::NativeText {
                    source_start,
                    source_end,
                    ..
                } = fragment.kind
                {
                    runs.push((source_start, source_end, x, y, fragment.height));
                }
            }
            for child in &fragment.children {
                collect(child, node, x, y, runs);
            }
        }
        collect(&page.fragment, editor.node, 0.0, 0.0, &mut runs);
        let mut source = Vec::new();
        let mut offset = 0;
        for cluster in editor.observed.graphemes(true) {
            let end = offset + length(cluster);
            source.push((offset, end, cluster));
            offset = end;
        }
        let mut rows = Vec::new();
        for (start, end, x, y, height) in runs {
            let mut carets = vec![TextCaret {
                offset: start,
                bounds: Bounds {
                    x,
                    y,
                    width: 1.0,
                    height,
                },
            }];
            let mut pen = x;
            let index = source.partition_point(|(_, end, _)| *end <= start);
            for &(begin, next, cluster) in source[index..]
                .iter()
                .take_while(|(begin, _, _)| *begin < end)
            {
                if begin < start {
                    continue;
                }
                if !cluster.contains(['\r', '\n']) {
                    let shown = if info.protected { "•" } else { cluster };
                    pen += blueice_font::measure_text_width(
                        shown,
                        style.font_size_px,
                        style.is_bold(),
                        style.is_italic(),
                    );
                }
                carets.push(TextCaret {
                    offset: next,
                    bounds: Bounds {
                        x: pen,
                        y,
                        width: 1.0,
                        height,
                    },
                });
            }
            rows.push(Row { start, end, carets });
        }
        (!rows.is_empty()).then_some(Self { bounds, clip, rows })
    }

    fn row(&self, offset: u32) -> &Row {
        self.rows
            .iter()
            .rev()
            .find(|row| row.start <= offset)
            .unwrap_or(&self.rows[0])
    }

    fn row_caret(row: &Row, offset: u32) -> Bounds {
        row.carets
            .iter()
            .rev()
            .find(|caret| caret.offset <= offset)
            .unwrap_or(&row.carets[0])
            .bounds
    }

    pub fn caret(&self, offset: u32) -> Bounds {
        Self::row_caret(self.row(offset), offset)
    }

    pub fn bounded_carets(&self, cursor: u32) -> Vec<TextCaret> {
        let mut carets: Vec<_> = self.rows.iter().flat_map(|row| &row.carets).collect();
        let center = carets.partition_point(|caret| caret.offset < cursor);
        let start = center.saturating_sub(MAX_EDIT_CARETS / 2);
        let end = (start + MAX_EDIT_CARETS).min(carets.len());
        carets.drain(start..end).cloned().collect()
    }

    pub fn rects(&self, range: TextRange) -> Vec<Bounds> {
        if range.length == 0 {
            return Vec::new();
        }
        let Some(end) = range.end() else {
            return Vec::new();
        };
        self.rows
            .iter()
            .filter_map(|row| {
                let begin = range.location.max(row.start);
                let finish = end.min(row.end);
                if begin >= finish {
                    return None;
                }
                let left = Self::row_caret(row, begin);
                let right = Self::row_caret(row, finish);
                Some(Bounds {
                    width: (right.x - left.x).max(1.0),
                    ..left
                })
            })
            .collect()
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.bounds.x
            && y >= self.bounds.y
            && x <= self.bounds.x + self.bounds.width
            && y <= self.bounds.y + self.bounds.height
    }

    pub fn nearest(&self, x: f64, y: f64) -> u32 {
        let row = self
            .rows
            .iter()
            .min_by(|a, b| {
                let distance = |row: &Row| {
                    let rect = row.carets[0].bounds;
                    (y - rect.y - rect.height / 2.0).abs()
                };
                distance(a).total_cmp(&distance(b))
            })
            .expect("native run");
        row.carets
            .iter()
            .min_by(|a, b| (a.bounds.x - x).abs().total_cmp(&(b.bounds.x - x).abs()))
            .expect("caret")
            .offset
    }

    pub fn line_range(&self, offset: u32) -> (u32, u32) {
        let row = self.row(offset);
        (row.start, row.end)
    }

    pub fn vertical(&self, offset: u32, down: bool) -> u32 {
        let row = self
            .rows
            .iter()
            .rposition(|row| row.start <= offset)
            .unwrap_or(0);
        let next = if down {
            (row + 1).min(self.rows.len() - 1)
        } else {
            row.saturating_sub(1)
        };
        let x = self.caret(offset).x;
        self.rows[next]
            .carets
            .iter()
            .min_by(|a, b| (a.bounds.x - x).abs().total_cmp(&(b.bounds.x - x).abs()))
            .expect("caret")
            .offset
    }

    pub fn line_count(&self) -> u32 {
        self.rows.len() as u32
    }
    pub fn line_index(&self, offset: u32) -> u32 {
        self.rows
            .iter()
            .rposition(|row| row.start <= offset)
            .unwrap_or(0) as u32
    }
    pub fn range_for_line(&self, line: u32, text_length: u32) -> Option<TextRange> {
        let row = self.rows.get(line as usize)?;
        let end = self
            .rows
            .get(line as usize + 1)
            .map(|next| next.start)
            .unwrap_or(text_length);
        Some(TextRange {
            location: row.start,
            length: end.saturating_sub(row.start),
        })
    }
    pub fn range_for_position(&self, text: &str, x: f64, y: f64) -> Option<TextRange> {
        if !x.is_finite()
            || !y.is_finite()
            || x < self.clip.x
            || x > self.clip.x + self.clip.width
            || y < self.clip.y
            || y > self.clip.y + self.clip.height
        {
            return None;
        }
        let row = self.rows.iter().min_by(|a, b| {
            let d = |row: &Row| {
                let r = row.carets[0].bounds;
                (y - r.y - r.height / 2.0).abs()
            };
            d(a).total_cmp(&d(b))
        })?;
        let offset = row
            .carets
            .windows(2)
            .find(|pair| x >= pair[0].bounds.x && x < pair[1].bounds.x.max(pair[0].bounds.x + 1.0))
            .map(|pair| pair[0].offset)
            .unwrap_or_else(|| self.nearest(x, y).min(length(text).saturating_sub(1)));
        if text.is_empty() {
            Some(TextRange {
                location: 0,
                length: 0,
            })
        } else {
            grapheme_range(text, offset)
        }
    }
    pub fn visible_range(&self, clip: Bounds) -> TextRange {
        let mut start = None;
        let mut end = 0;
        for row in &self.rows {
            for pair in row.carets.windows(2) {
                let rect = Bounds {
                    width: (pair[1].bounds.x - pair[0].bounds.x).max(1.0),
                    ..pair[0].bounds
                };
                if intersection(rect, clip).is_some() {
                    start = Some(start.map_or(pair[0].offset, |old: u32| old.min(pair[0].offset)));
                    end = end.max(pair[1].offset);
                }
            }
        }
        TextRange {
            location: start.unwrap_or(0),
            length: end.saturating_sub(start.unwrap_or(0)),
        }
    }
    pub fn range_bounds(&self, range: TextRange) -> Bounds {
        let rects = if range.length == 0 {
            vec![self.caret(range.location)]
        } else {
            self.rects(range)
        };
        union(&rects).unwrap_or_else(|| self.caret(range.location))
    }
    pub fn scroll_offsets(
        &self,
        old: (f64, f64),
        range: Option<TextRange>,
        multiline: bool,
    ) -> (f64, f64) {
        let right = self.clip.x + self.clip.width - 1.0;
        let bottom = self.clip.y + self.clip.height;
        let max_x = self
            .rows
            .iter()
            .flat_map(|row| &row.carets)
            .map(|c| (c.bounds.x + c.bounds.width - right).max(0.0))
            .fold(0.0, f64::max);
        let max_y = if multiline {
            self.rows
                .iter()
                .flat_map(|row| &row.carets)
                .map(|c| (c.bounds.y + c.bounds.height - bottom).max(0.0))
                .fold(0.0, f64::max)
        } else {
            0.0
        };
        let mut x = old.0.clamp(0.0, max_x);
        let mut y = old.1.clamp(0.0, max_y);
        if let Some(range) = range {
            let rect = self.range_bounds(range);
            if rect.width > self.clip.width || rect.x - x < self.clip.x {
                x = rect.x - self.clip.x;
            } else if rect.x + rect.width - x > right {
                x = rect.x + rect.width - right;
            }
            if multiline {
                if rect.height > self.clip.height || rect.y - y < self.clip.y {
                    y = rect.y - self.clip.y;
                } else if rect.y + rect.height - y > bottom {
                    y = rect.y + rect.height - bottom;
                }
            }
        }
        (x.clamp(0.0, max_x), y.clamp(0.0, max_y))
    }
}

pub(in crate::page) fn grapheme_range(text: &str, index: u32) -> Option<TextRange> {
    let mut offset = 0;
    for cluster in text.graphemes(true) {
        let end = offset + length(cluster);
        if index >= offset && index < end {
            return Some(TextRange {
                location: offset,
                length: end - offset,
            });
        }
        offset = end;
    }
    None
}
pub(in crate::page) fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Bounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}
pub(in crate::page) fn union(rects: &[Bounds]) -> Option<Bounds> {
    let first = *rects.first()?;
    let x = rects.iter().fold(first.x, |x, r| x.min(r.x));
    let y = rects.iter().fold(first.y, |y, r| y.min(r.y));
    let right = rects
        .iter()
        .fold(first.x + first.width, |x, r| x.max(r.x + r.width));
    let bottom = rects
        .iter()
        .fold(first.y + first.height, |y, r| y.max(r.y + r.height));
    Some(Bounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}
