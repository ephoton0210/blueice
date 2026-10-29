// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS Grid layout, in the subset real pages lean on: explicit tracks from
//! `grid-template-columns`/`rows` (`px`/`%`/`fr`/`auto`/`min-content`/
//! `max-content`, `minmax()`, `fit-content()`, `repeat()` including
//! `auto-fill`/`auto-fit`), `grid-template-areas`, line placement
//! (`grid-column`/`row`/`area`, `span`, negative lines), automatic placement
//! (row or column flow, sparse or dense), `gap`, item alignment
//! (`justify-*`/`align-*`, auto margins) and track sizing in the spec's
//! order (base sizes, growth, `fr` resolution, `auto` stretch). Subgrid,
//! masonry and named lines are not implemented.

use crate::block::{layout_block_level, Constraint, Forced, PendingAbs};
use crate::ctx::Ctx;
use crate::flex::{collect_items, FlexInput, FlexLayout, Source};
use crate::fragment::{Fragment, FragmentKind};
use blueice_css::{Length, Token, Value};
use blueice_dom::NodeId;

/// Room enough that nothing wraps when measuring content.
const UNBOUNDED: f64 = 1.0e6;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sz {
    Px(f64),
    Fr(f64),
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    min: Sz,
    max: Sz,
}

impl Track {
    const AUTO: Track = Track {
        min: Sz::Auto,
        max: Sz::Auto,
    };
}

enum ListEntry {
    Track(Track),
    AutoRepeat { tracks: Vec<Track>, fit: bool },
}

/// One component of a track list, or `None` when it is not understood.
fn size_of(tokens: &[Token], base: f64, font: &blueice_css::LengthContext) -> Option<Sz> {
    match tokens {
        [Token::Ident(word)] => match word.to_ascii_lowercase().as_str() {
            "auto" => Some(Sz::Auto),
            "min-content" => Some(Sz::MinContent),
            "max-content" => Some(Sz::MaxContent),
            _ => None,
        },
        [Token::Number(n)] if *n == 0.0 => Some(Sz::Px(0.0)),
        [Token::Percentage(p)] => (!base.is_nan()).then(|| Sz::Px(base * p / 100.0)),
        [Token::Dimension(n, unit)] => {
            if unit.eq_ignore_ascii_case("fr") {
                Some(Sz::Fr(n.max(0.0)))
            } else {
                Length::from_unit(*n, unit).map(|l| Sz::Px(l.to_px(font)))
            }
        }
        _ => None,
    }
}

/// The tokens between the parenthesis that opens at `tokens[open]` and its
/// match, and the index just past it.
fn group(tokens: &[Token], open: usize) -> (&[Token], usize) {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate().skip(open) {
        match t {
            Token::LeftParen => depth += 1,
            Token::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return (&tokens[open + 1..i], i + 1);
                }
            }
            _ => {}
        }
    }
    (&tokens[open + 1..], tokens.len())
}

fn split_commas(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let (mut start, mut depth) = (0, 0usize);
    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::LeftParen => depth += 1,
            Token::RightParen => depth = depth.saturating_sub(1),
            Token::Comma if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

fn trim(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|t| *t != Token::Whitespace)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|t| *t != Token::Whitespace)
        .map_or(start, |e| e + 1);
    &tokens[start..end]
}

fn minmax(args: &[Token], base: f64, font: &blueice_css::LengthContext) -> Option<Track> {
    let parts = split_commas(args);
    let [a, b] = parts.as_slice() else {
        return None;
    };
    let min = size_of(trim(a), base, font)?;
    let max = size_of(trim(b), base, font)?;
    Some(Track {
        min: match min {
            Sz::Fr(_) => Sz::Auto,
            m => m,
        },
        max,
    })
}

fn parse_tracks(tokens: &[Token], base: f64, font: &blueice_css::LengthContext) -> Vec<ListEntry> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Whitespace => i += 1,
            Token::LeftBracket => {
                while i < tokens.len() && tokens[i] != Token::RightBracket {
                    i += 1;
                }
                i += 1;
            }
            Token::Ident(name) if matches!(tokens.get(i + 1), Some(Token::LeftParen)) => {
                let (args, next) = group(tokens, i + 1);
                match name.to_ascii_lowercase().as_str() {
                    "repeat" => {
                        let parts = split_commas(args);
                        if let [count, rest] = parts.as_slice() {
                            let inner: Vec<Track> = parse_tracks(trim(rest), base, font)
                                .into_iter()
                                .filter_map(|e| match e {
                                    ListEntry::Track(t) => Some(t),
                                    _ => None,
                                })
                                .collect();
                            match trim(count) {
                                [Token::Number(n)] => {
                                    for _ in 0..(*n as usize).min(1000) {
                                        out.extend(inner.iter().copied().map(ListEntry::Track));
                                    }
                                }
                                [Token::Ident(w)] if w == "auto-fill" || w == "auto-fit" => {
                                    out.push(ListEntry::AutoRepeat {
                                        tracks: inner,
                                        fit: w == "auto-fit",
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    "minmax" => {
                        if let Some(t) = minmax(args, base, font) {
                            out.push(ListEntry::Track(t));
                        }
                    }
                    "fit-content" => {
                        if let Some(Sz::Px(p)) = size_of(trim(args), base, font) {
                            out.push(ListEntry::Track(Track {
                                min: Sz::Auto,
                                max: Sz::Px(p),
                            }));
                        }
                    }
                    _ => out.push(ListEntry::Track(Track::AUTO)),
                }
                i = next;
            }
            other => {
                if let Some(size) = size_of(std::slice::from_ref(other), base, font) {
                    out.push(ListEntry::Track(match size {
                        Sz::Fr(_) => Track {
                            min: Sz::Auto,
                            max: size,
                        },
                        Sz::Px(_) => Track {
                            min: size,
                            max: size,
                        },
                        s => Track { min: s, max: s },
                    }));
                }
                i += 1;
            }
        }
    }
    out
}

/// The explicit tracks, with any `auto-fill`/`auto-fit` repeat resolved for
/// `available` space (`None`: indefinite, one repetition).
fn explicit_tracks(
    entries: Vec<ListEntry>,
    available: Option<f64>,
    gap: f64,
) -> (Vec<Track>, Vec<usize>) {
    let fixed: f64 = entries
        .iter()
        .map(|e| match e {
            ListEntry::Track(t) => track_floor(t),
            _ => 0.0,
        })
        .sum();
    let fixed_count = entries
        .iter()
        .filter(|e| matches!(e, ListEntry::Track(_)))
        .count();
    let mut out = Vec::new();
    let mut collapsible = Vec::new();
    for entry in entries {
        match entry {
            ListEntry::Track(t) => out.push(t),
            ListEntry::AutoRepeat { tracks, fit } => {
                if tracks.is_empty() {
                    continue;
                }
                let per: f64 =
                    tracks.iter().map(track_floor).sum::<f64>() + gap * (tracks.len() - 1) as f64;
                let count = match available {
                    Some(avail) if per + gap > 0.0 => {
                        let used = fixed + gap * fixed_count as f64;
                        (((avail - used + gap) / (per + gap)).floor() as i64).clamp(1, 1000)
                            as usize
                    }
                    _ => 1,
                };
                for _ in 0..count {
                    let start = out.len();
                    out.extend(tracks.iter().copied());
                    if fit {
                        collapsible.extend(start..out.len());
                    }
                }
            }
        }
    }
    (out, collapsible)
}

fn track_floor(t: &Track) -> f64 {
    match (t.min, t.max) {
        (Sz::Px(p), _) => p,
        (_, Sz::Px(p)) => p,
        _ => 0.0,
    }
}

// ---------------------------------------------------------------- placement

#[derive(Clone, Debug, PartialEq)]
enum Line {
    Auto,
    Num(i32),
    Span(usize),
    Name(String),
}

fn parse_line(tokens: &[Token]) -> Line {
    match trim(tokens) {
        [Token::Number(n)] => Line::Num(*n as i32),
        [Token::Ident(w)] if w == "auto" => Line::Auto,
        [Token::Ident(w), Token::Whitespace, Token::Number(n)]
        | [Token::Number(n), Token::Whitespace, Token::Ident(w)]
            if w == "span" =>
        {
            Line::Span((*n as usize).max(1))
        }
        [Token::Ident(w)] if w == "span" => Line::Span(1),
        [Token::Ident(name)] => Line::Name(name.clone()),
        _ => Line::Auto,
    }
}

fn parse_pair(value: Option<&Value>) -> (Line, Line) {
    let tokens = match value {
        Some(Value::Unparsed(t)) => t.as_slice(),
        Some(Value::Keyword(k)) if k != "auto" => {
            return (Line::Name(k.clone()), Line::Name(k.clone()))
        }
        Some(Value::Number(n)) => return (Line::Num(*n as i32), Line::Auto),
        _ => return (Line::Auto, Line::Auto),
    };
    let parts: Vec<&[Token]> = tokens
        .split(|t| *t == Token::Delim('/'))
        .map(trim)
        .collect();
    let start = parse_line(parts[0]);
    let end = match parts.get(1) {
        Some(p) => parse_line(p),
        None => match &start {
            Line::Name(n) => Line::Name(n.clone()),
            _ => Line::Auto,
        },
    };
    (start, end)
}

/// Named areas: `name -> (row_start, col_start, row_end, col_end)`, zero-based
/// track indices with exclusive ends.
fn parse_areas(value: Option<&Value>) -> Vec<(String, (usize, usize, usize, usize))> {
    let Some(Value::Unparsed(tokens)) = value else {
        return Vec::new();
    };
    let rows: Vec<Vec<&str>> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::QuotedString(s) => Some(s.split_whitespace().collect()),
            _ => None,
        })
        .collect();
    let mut areas: Vec<(String, (usize, usize, usize, usize))> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, name) in row.iter().enumerate() {
            if name.chars().all(|ch| ch == '.') {
                continue;
            }
            match areas.iter_mut().find(|(n, _)| n == name) {
                Some((_, b)) => {
                    b.0 = b.0.min(r);
                    b.1 = b.1.min(c);
                    b.2 = b.2.max(r + 1);
                    b.3 = b.3.max(c + 1);
                }
                None => areas.push((name.to_string(), (r, c, r + 1, c + 1))),
            }
        }
    }
    areas
}

struct Placement {
    row: Option<usize>,
    col: Option<usize>,
    row_span: usize,
    col_span: usize,
}

fn resolve_axis(
    start: &Line,
    end: &Line,
    explicit: usize,
    area: Option<(usize, usize)>,
) -> (Option<usize>, usize) {
    let index = |n: i32, is_end: bool| -> Option<usize> {
        let line = if n > 0 {
            n as usize
        } else if n < 0 {
            (explicit as i32 + 2 + n).max(1) as usize
        } else {
            return None;
        };
        let _ = is_end;
        Some(line - 1)
    };
    let from_name = |name: &str, is_end: bool| -> Option<usize> {
        let _ = name;
        area.map(|(s, e)| if is_end { e } else { s })
    };
    let s = match start {
        Line::Num(n) => index(*n, false),
        Line::Name(n) => from_name(n, false),
        _ => None,
    };
    let e = match end {
        Line::Num(n) => index(*n, true),
        Line::Name(n) => from_name(n, true),
        _ => None,
    };
    let span_of = |l: &Line| match l {
        Line::Span(n) => Some(*n),
        _ => None,
    };
    match (s, e) {
        (Some(s), Some(e)) if e > s => (Some(s), e - s),
        (Some(s), Some(e)) => (Some(e.min(s)), 1),
        (Some(s), None) => (Some(s), span_of(end).unwrap_or(1)),
        (None, Some(e)) => {
            let span = span_of(start).unwrap_or(1);
            (Some(e.saturating_sub(span)), span)
        }
        (None, None) => (None, span_of(start).or(span_of(end)).unwrap_or(1)),
    }
}

fn placement_of(
    ctx: &Ctx<'_>,
    node: NodeId,
    cols: usize,
    rows: usize,
    areas: &[(String, (usize, usize, usize, usize))],
) -> Placement {
    let style = ctx.style(node);
    let (mut rs, mut re) = parse_pair(style.raw("grid-row"));
    let (mut cs, mut ce) = parse_pair(style.raw("grid-column"));
    if let Some(value) = style.raw("grid-area") {
        match value {
            Value::Keyword(name) if name != "auto" => {
                let n = Line::Name(name.clone());
                (rs, re, cs, ce) = (n.clone(), n.clone(), n.clone(), n);
            }
            Value::Unparsed(tokens) => {
                let parts: Vec<Line> = tokens
                    .split(|t| *t == Token::Delim('/'))
                    .map(parse_line)
                    .collect();
                match parts.as_slice() {
                    [one @ Line::Name(_)] => {
                        (rs, re, cs, ce) = (one.clone(), one.clone(), one.clone(), one.clone())
                    }
                    [a, b, c, d] => (rs, cs, re, ce) = (a.clone(), b.clone(), c.clone(), d.clone()),
                    [a, b, c] => (rs, cs, re, ce) = (a.clone(), b.clone(), c.clone(), Line::Auto),
                    [a, b] => (rs, cs, re, ce) = (a.clone(), b.clone(), Line::Auto, Line::Auto),
                    [a] => (rs, re) = (a.clone(), Line::Auto),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let named = |line: &Line| match line {
        Line::Name(n) => areas.iter().find(|(a, _)| a == n).map(|(_, b)| *b),
        _ => None,
    };
    let row_area = named(&rs).or_else(|| named(&re)).map(|b| (b.0, b.2));
    let col_area = named(&cs).or_else(|| named(&ce)).map(|b| (b.1, b.3));
    let (row, row_span) = resolve_axis(&rs, &re, rows, row_area);
    let (col, col_span) = resolve_axis(&cs, &ce, cols, col_area);
    Placement {
        row,
        col,
        row_span,
        col_span,
    }
}

struct Occupancy {
    cells: Vec<Vec<bool>>,
}

impl Occupancy {
    fn free(&self, r: usize, c: usize, rs: usize, cs: usize) -> bool {
        (r..r + rs).all(|rr| {
            (c..c + cs).all(|cc| {
                !self
                    .cells
                    .get(rr)
                    .and_then(|row| row.get(cc))
                    .copied()
                    .unwrap_or(false)
            })
        })
    }
    fn mark(&mut self, r: usize, c: usize, rs: usize, cs: usize) {
        for rr in r..r + rs {
            if self.cells.len() <= rr {
                self.cells.resize(rr + 1, Vec::new());
            }
            if self.cells[rr].len() < c + cs {
                self.cells[rr].resize(c + cs, false);
            }
            for cc in c..c + cs {
                self.cells[rr][cc] = true;
            }
        }
    }
}

// ------------------------------------------------------------- track sizing

/// One item's demand on a run of tracks.
struct Contribution {
    start: usize,
    span: usize,
    min: f64,
    max: f64,
}

fn size_tracks(
    tracks: &[Track],
    gap: f64,
    available: Option<f64>,
    contributions: &[Contribution],
    stretch_auto: bool,
) -> Vec<f64> {
    let n = tracks.len();
    let mut base: Vec<f64> = tracks
        .iter()
        .map(|t| if let Sz::Px(p) = t.min { p } else { 0.0 })
        .collect();
    let mut limit: Vec<f64> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| match t.max {
            Sz::Px(p) => p.max(base[i]),
            _ => base[i],
        })
        .collect();
    let mut content_max = vec![0.0f64; n];
    let intrinsic_min = |t: &Track| matches!(t.min, Sz::Auto | Sz::MinContent | Sz::MaxContent);
    let intrinsic_max = |t: &Track| matches!(t.max, Sz::Auto | Sz::MinContent | Sz::MaxContent);

    for c in contributions.iter().filter(|c| c.span == 1) {
        let i = c.start;
        let t = &tracks[i];
        if intrinsic_min(t) {
            let want = if t.min == Sz::MaxContent {
                c.max
            } else {
                c.min
            };
            base[i] = base[i].max(want);
        }
        if intrinsic_max(t) {
            let want = if t.max == Sz::MinContent {
                c.min
            } else {
                c.max
            };
            limit[i] = limit[i].max(want);
        }
        content_max[i] = content_max[i].max(c.max);
    }
    for c in contributions.iter().filter(|c| c.span > 1) {
        let range = c.start..(c.start + c.span).min(n);
        let flexible: Vec<usize> = range
            .clone()
            .filter(|&i| intrinsic_min(&tracks[i]) || intrinsic_max(&tracks[i]))
            .collect();
        if flexible.is_empty() {
            continue;
        }
        let inner = gap * (range.len() - 1) as f64;
        for (need, target, only_intrinsic_min) in
            [(c.min, &mut base, true), (c.max, &mut limit, false)]
        {
            let have: f64 = range.clone().map(|i| target[i]).sum::<f64>() + inner;
            if need > have {
                let eligible: Vec<usize> = flexible
                    .iter()
                    .copied()
                    .filter(|&i| {
                        if only_intrinsic_min {
                            intrinsic_min(&tracks[i])
                        } else {
                            intrinsic_max(&tracks[i])
                        }
                    })
                    .collect();
                if eligible.is_empty() {
                    continue;
                }
                let share = (need - have) / eligible.len() as f64;
                for i in eligible {
                    target[i] += share;
                }
            }
        }
    }
    for i in 0..n {
        limit[i] = limit[i].max(base[i]);
        if matches!(tracks[i].max, Sz::Fr(_)) {
            limit[i] = base[i];
        }
    }

    let gaps = gap * n.saturating_sub(1) as f64;
    let mut size = base.clone();
    let is_fr = |i: usize| matches!(tracks[i].max, Sz::Fr(_));

    // Grow tracks toward their limits with the space left.
    if let Some(avail) = available {
        let mut free = avail - gaps - size.iter().sum::<f64>();
        for _ in 0..n + 1 {
            if free <= 0.001 {
                break;
            }
            let growable: Vec<usize> = (0..n)
                .filter(|&i| !is_fr(i) && size[i] + 0.001 < limit[i])
                .collect();
            if growable.is_empty() {
                break;
            }
            let share = free / growable.len() as f64;
            let mut used = 0.0;
            for i in growable {
                let add = share.min(limit[i] - size[i]);
                size[i] += add;
                used += add;
            }
            free -= used;
        }
    } else {
        for i in 0..n {
            if !is_fr(i) {
                size[i] = limit[i].max(size[i]);
            }
        }
    }

    // Flexible tracks.
    let flex_total: f64 = (0..n)
        .filter_map(|i| match tracks[i].max {
            Sz::Fr(f) => Some(f),
            _ => None,
        })
        .sum();
    if flex_total > 0.0 {
        match available {
            Some(avail) => {
                let mut settled = vec![false; n];
                let mut fr_size;
                loop {
                    let leftover = avail
                        - gaps
                        - (0..n)
                            .filter(|&i| !is_fr(i) || settled[i])
                            .map(|i| size[i])
                            .sum::<f64>();
                    let factor: f64 = (0..n)
                        .filter(|&i| is_fr(i) && !settled[i])
                        .map(|i| match tracks[i].max {
                            Sz::Fr(f) => f,
                            _ => 0.0,
                        })
                        .sum();
                    fr_size = leftover.max(0.0) / factor.max(1.0);
                    let mut changed = false;
                    for i in 0..n {
                        if is_fr(i) && !settled[i] {
                            let f = match tracks[i].max {
                                Sz::Fr(f) => f,
                                _ => 0.0,
                            };
                            if f * fr_size < base[i] {
                                size[i] = base[i];
                                settled[i] = true;
                                changed = true;
                            }
                        }
                    }
                    if !changed {
                        break;
                    }
                }
                for i in 0..n {
                    if is_fr(i) && !settled[i] {
                        if let Sz::Fr(f) = tracks[i].max {
                            size[i] = (f * fr_size).max(base[i]);
                        }
                    }
                }
            }
            None => {
                let fr_size = (0..n)
                    .filter(|&i| is_fr(i))
                    .map(|i| match tracks[i].max {
                        Sz::Fr(f) if f > 0.0 => base[i].max(content_max[i]) / f.max(1.0),
                        _ => 0.0,
                    })
                    .fold(0.0, f64::max);
                for i in 0..n {
                    if let Sz::Fr(f) = tracks[i].max {
                        size[i] = (f * fr_size).max(base[i]);
                    }
                }
            }
        }
    } else if stretch_auto {
        if let Some(avail) = available {
            let free = avail - gaps - size.iter().sum::<f64>();
            let autos: Vec<usize> = (0..n).filter(|&i| tracks[i].max == Sz::Auto).collect();
            if free > 0.001 && !autos.is_empty() {
                let share = free / autos.len() as f64;
                for i in autos {
                    size[i] += share;
                }
            }
        }
    }
    size
}

// -------------------------------------------------------------------- items

struct GridItem {
    source: Source,
    row: usize,
    col: usize,
    row_span: usize,
    col_span: usize,
    margin: [Option<f64>; 4],
    width_auto: bool,
    height_auto: bool,
    replaced: bool,
    justify: String,
    align: String,
}

impl GridItem {
    fn margin(&self, side: usize) -> f64 {
        self.margin[side].unwrap_or(0.0)
    }
}

fn lay_out(
    ctx: &Ctx<'_>,
    container: NodeId,
    source: &Source,
    cb: &Constraint,
    width: Option<f64>,
    height: Option<f64>,
) -> (Fragment, Vec<PendingAbs>) {
    match source {
        Source::Element(node) => {
            let placed = layout_block_level(ctx, *node, cb, Forced { width, height });
            (placed.fragment, placed.pending_abs)
        }
        Source::Text(nodes) => {
            let w = width.unwrap_or(cb.cb_width);
            let run = crate::inline::layout_run(ctx, container, nodes, w, false);
            let h = height.unwrap_or(run.height);
            (
                Fragment {
                    node: None,
                    kind: FragmentKind::Block,
                    width: w,
                    height: h,
                    children: run.lines,
                    ..Fragment::empty_block()
                },
                run.pending_abs,
            )
        }
    }
}

fn content_widths(ctx: &Ctx<'_>, container: NodeId, source: &Source) -> (f64, f64) {
    match source {
        Source::Element(node) => {
            let style = ctx.style(*node);
            let (min, max) = crate::intrinsic::content_widths(ctx, *node);
            if let Some(w) = style.px("width", f64::NAN) {
                let pb = style.padding(0.0).horizontal() + style.border(0.0).horizontal();
                let w = if style.is_border_box() { w } else { w + pb };
                return (w.min(min.max(w)), w);
            }
            (min, max)
        }
        Source::Text(nodes) => {
            let widest = |w: f64| crate::inline::layout_run(ctx, container, nodes, w, false).extent;
            (widest(0.0), widest(UNBOUNDED))
        }
    }
}

pub(crate) fn layout_grid(ctx: &Ctx<'_>, container: NodeId, input: &FlexInput) -> FlexLayout {
    let cstyle = ctx.style(container);
    let content_width = input.content_width;
    let measuring = content_width >= 1.0e5;
    let font = cstyle.length_context(content_width);
    let gap_of = |property: &str, base: f64| cstyle.px(property, base).unwrap_or(0.0).max(0.0);
    let col_gap = gap_of("column-gap", content_width);
    let row_gap = gap_of("row-gap", input.definite_height.unwrap_or(f64::NAN));

    let tokens_of = |property: &str| -> Vec<Token> {
        match cstyle.raw(property) {
            Some(Value::Unparsed(t)) => t.clone(),
            Some(Value::Keyword(k)) if k != "none" => vec![Token::Ident(k.clone())],
            Some(Value::Length(l)) => vec![Token::Dimension(
                match l {
                    Length::Px(n) => *n,
                    other => other.to_px(&font),
                },
                "px".to_string(),
            )],
            _ => Vec::new(),
        }
    };
    let col_avail = (!measuring).then_some(content_width);
    let (explicit_cols, collapse_cols) = explicit_tracks(
        parse_tracks(&tokens_of("grid-template-columns"), content_width, &font),
        col_avail,
        col_gap,
    );
    let (explicit_rows, _) = explicit_tracks(
        parse_tracks(
            &tokens_of("grid-template-rows"),
            input.definite_height.unwrap_or(f64::NAN),
            &font,
        ),
        input.definite_height,
        row_gap,
    );
    let auto_col = parse_tracks(&tokens_of("grid-auto-columns"), content_width, &font)
        .into_iter()
        .find_map(|e| match e {
            ListEntry::Track(t) => Some(t),
            _ => None,
        })
        .unwrap_or(Track::AUTO);
    let auto_row = parse_tracks(
        &tokens_of("grid-auto-rows"),
        input.definite_height.unwrap_or(f64::NAN),
        &font,
    )
    .into_iter()
    .find_map(|e| match e {
        ListEntry::Track(t) => Some(t),
        _ => None,
    })
    .unwrap_or(Track::AUTO);
    let areas = parse_areas(cstyle.raw("grid-template-areas"));
    let area_rows = areas.iter().map(|(_, b)| b.2).max().unwrap_or(0);
    let area_cols = areas.iter().map(|(_, b)| b.3).max().unwrap_or(0);
    let flow = cstyle
        .keyword("grid-auto-flow")
        .unwrap_or("row")
        .to_string();
    let flow_list = match cstyle.raw("grid-auto-flow") {
        Some(Value::List(parts)) => parts
            .iter()
            .filter_map(|p| match p {
                Value::Keyword(k) => Some(k.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => flow,
    };
    let column_flow = flow_list.contains("column");
    let dense = flow_list.contains("dense");
    let explicit_col_count = explicit_cols.len().max(area_cols);
    let explicit_row_count = explicit_rows.len().max(area_rows);

    // ---- gather items ----
    let mut sources = Vec::new();
    let mut abs_nodes = Vec::new();
    collect_items(
        ctx,
        container,
        &mut sources,
        &mut abs_nodes,
        &mut Vec::new(),
    );
    let mut result = FlexLayout {
        children: Vec::new(),
        height: 0.0,
        extent: 0.0,
        pending_abs: abs_nodes
            .into_iter()
            .map(|node| PendingAbs {
                node,
                x: 0.0,
                y: 0.0,
            })
            .collect(),
    };
    if sources.is_empty() {
        result.height = input.definite_height.unwrap_or(0.0);
        return result;
    }
    let default_justify = cstyle
        .keyword("justify-items")
        .unwrap_or("normal")
        .to_string();
    let default_align = cstyle
        .keyword("align-items")
        .unwrap_or("normal")
        .to_string();
    let mut items: Vec<(GridItem, Placement)> = sources
        .into_iter()
        .map(|source| {
            let (place, style_node) = match &source {
                Source::Element(n) => (
                    placement_of(ctx, *n, explicit_col_count, explicit_row_count, &areas),
                    Some(*n),
                ),
                Source::Text(_) => (
                    Placement {
                        row: None,
                        col: None,
                        row_span: 1,
                        col_span: 1,
                    },
                    None,
                ),
            };
            let mut item = GridItem {
                source,
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                margin: [Some(0.0); 4],
                width_auto: true,
                height_auto: true,
                replaced: false,
                justify: default_justify.clone(),
                align: default_align.clone(),
            };
            if let Some(node) = style_node {
                let style = ctx.style(node);
                let m = |name: &str| {
                    if style.is_auto(name) {
                        None
                    } else {
                        Some(style.side(name, content_width))
                    }
                };
                item.margin = [
                    m("margin-top"),
                    m("margin-right"),
                    m("margin-bottom"),
                    m("margin-left"),
                ];
                item.width_auto = style.raw("width").is_none() || style.is_auto("width");
                item.height_auto = style.raw("height").is_none() || style.is_auto("height");
                item.replaced = ctx.tag(node).is_some_and(crate::block::is_replaced_tag);
                if let Some(w) = style.keyword("justify-self") {
                    if w != "auto" {
                        item.justify = w.to_string();
                    }
                }
                if let Some(w) = style.keyword("align-self") {
                    if w != "auto" {
                        item.align = w.to_string();
                    }
                }
            }
            (item, place)
        })
        .collect();

    // ---- automatic placement ----
    let mut occupancy = Occupancy { cells: Vec::new() };
    let mut fixed_cols = explicit_col_count.max(1);
    let mut fixed_rows = explicit_row_count.max(1);
    for (item, place) in items.iter_mut() {
        item.row_span = place.row_span;
        item.col_span = place.col_span;
        if let (Some(r), Some(c)) = (place.row, place.col) {
            item.row = r;
            item.col = c;
            occupancy.mark(r, c, place.row_span, place.col_span);
            fixed_cols = fixed_cols.max(c + place.col_span);
            fixed_rows = fixed_rows.max(r + place.row_span);
        }
    }
    for (_, place) in items.iter() {
        if place.row.is_none() && !column_flow {
            fixed_cols = fixed_cols.max(place.col_span);
        }
        if place.col.is_none() && column_flow {
            fixed_rows = fixed_rows.max(place.row_span);
        }
        if let (Some(c), None) = (place.col, place.row) {
            fixed_cols = fixed_cols.max(c + place.col_span);
        }
        if let (Some(r), None) = (place.row, place.col) {
            fixed_rows = fixed_rows.max(r + place.row_span);
        }
    }
    let mut cursor = (0usize, 0usize); // (major, minor): row flow = (row, col)
    for (item, place) in items.iter_mut() {
        if place.row.is_some() && place.col.is_some() {
            continue;
        }
        if dense {
            cursor = (0, 0);
        }
        let (rs, cs) = (place.row_span, place.col_span);
        let mut spot = None;
        if !column_flow {
            let mut r = cursor.0;
            let mut c = cursor.1;
            if let Some(fc) = place.col {
                if fc < c {
                    r += 1;
                }
                c = fc;
            }
            if let Some(fr) = place.row {
                r = fr;
                c = if dense { 0 } else { c };
            }
            for _ in 0..10_000 {
                if place.col.is_some() {
                    if occupancy.free(r, c, rs, cs) {
                        spot = Some((r, c));
                        break;
                    }
                    if place.row.is_some() {
                        c += 1;
                        if c + cs > fixed_cols.max(c + cs) + 200 {
                            break;
                        }
                    } else {
                        r += 1;
                    }
                } else if c + cs <= fixed_cols && occupancy.free(r, c, rs, cs) {
                    spot = Some((r, c));
                    break;
                } else {
                    c += 1;
                    if c + cs > fixed_cols {
                        if place.row.is_some() {
                            fixed_cols = c + cs;
                        } else {
                            c = 0;
                            r += 1;
                        }
                    }
                }
            }
            let (r, c) = spot.unwrap_or((r, c));
            item.row = r;
            item.col = c;
            fixed_rows = fixed_rows.max(r + rs);
            cursor = (r, c + cs);
        } else {
            let mut c = cursor.0;
            let mut r = cursor.1;
            if let Some(fr) = place.row {
                if fr < r {
                    c += 1;
                }
                r = fr;
            }
            if let Some(fc) = place.col {
                c = fc;
                r = if dense { 0 } else { r };
            }
            for _ in 0..10_000 {
                if place.row.is_some() {
                    if occupancy.free(r, c, rs, cs) {
                        spot = Some((r, c));
                        break;
                    }
                    if place.col.is_some() {
                        r += 1;
                    } else {
                        c += 1;
                    }
                } else if r + rs <= fixed_rows && occupancy.free(r, c, rs, cs) {
                    spot = Some((r, c));
                    break;
                } else {
                    r += 1;
                    if r + rs > fixed_rows {
                        r = 0;
                        c += 1;
                    }
                }
            }
            let (r, c) = spot.unwrap_or((r, c));
            item.row = r;
            item.col = c;
            fixed_cols = fixed_cols.max(c + cs);
            cursor = (c, r + rs);
        }
        occupancy.mark(item.row, item.col, rs, cs);
    }
    let col_count = items
        .iter()
        .map(|(i, _)| i.col + i.col_span)
        .max()
        .unwrap_or(0)
        .max(explicit_cols.len());
    let row_count = items
        .iter()
        .map(|(i, _)| i.row + i.row_span)
        .max()
        .unwrap_or(0)
        .max(explicit_rows.len());
    let mut col_tracks = explicit_cols;
    col_tracks.resize(col_count, auto_col);
    let mut row_tracks = explicit_rows;
    row_tracks.resize(row_count, auto_row);

    // `auto-fit` tracks that hold no item collapse to nothing.
    let mut col_collapsed = vec![false; col_count];
    for &c in &collapse_cols {
        if c < col_count
            && !items
                .iter()
                .any(|(i, _)| i.col <= c && c < i.col + i.col_span)
        {
            col_collapsed[c] = true;
            col_tracks[c] = Track {
                min: Sz::Px(0.0),
                max: Sz::Px(0.0),
            };
        }
    }

    // ---- column sizing ----
    let cb_for_measure = Constraint {
        cb_width: content_width,
        cb_height: input.definite_height,
        root: false,
    };
    let col_contributions: Vec<Contribution> = items
        .iter()
        .map(|(item, _)| {
            let (min, max) = content_widths(ctx, container, &item.source);
            let margins = item.margin(1) + item.margin(3);
            Contribution {
                start: item.col,
                span: item.col_span,
                min: min + margins,
                max: max + margins,
            }
        })
        .collect();
    let col_size = size_tracks(&col_tracks, col_gap, col_avail, &col_contributions, true);
    let col_size: Vec<f64> = col_size
        .into_iter()
        .zip(&col_collapsed)
        .map(|(s, &c)| if c { 0.0 } else { s })
        .collect();
    let mut col_x = Vec::with_capacity(col_count);
    let mut x = 0.0;
    for (i, w) in col_size.iter().enumerate() {
        col_x.push(x);
        x += w;
        if i + 1 < col_count && !col_collapsed[i] {
            x += col_gap;
        }
    }
    let total_width = x;
    let area_width = |item: &GridItem| -> f64 {
        let end = item.col + item.col_span;
        let mut w = col_x[end - 1] + col_size[end - 1] - col_x[item.col];
        if col_collapsed[end - 1] {
            w = (col_x[end - 1] - col_x[item.col]).max(0.0);
        }
        w
    };

    // ---- natural heights, then row sizing ----
    let mut natural: Vec<f64> = Vec::with_capacity(items.len());
    for (item, _) in &items {
        let area_w = area_width(item);
        let margins = item.margin(1) + item.margin(3);
        let forced_w = fit_width(ctx, container, item, area_w - margins);
        let (fragment, _) = lay_out(
            ctx,
            container,
            &item.source,
            &cb_for_measure,
            forced_w,
            None,
        );
        natural.push(fragment.height + item.margin(0) + item.margin(2));
    }
    let row_contributions: Vec<Contribution> = items
        .iter()
        .zip(&natural)
        .map(|((item, _), h)| Contribution {
            start: item.row,
            span: item.row_span,
            min: *h,
            max: *h,
        })
        .collect();
    let row_size = size_tracks(
        &row_tracks,
        row_gap,
        input.definite_height,
        &row_contributions,
        false,
    );
    let mut row_y = Vec::with_capacity(row_count);
    let mut y = 0.0;
    for (i, h) in row_size.iter().enumerate() {
        row_y.push(y);
        y += h;
        if i + 1 < row_count {
            y += row_gap;
        }
    }
    let total_height = y;

    // ---- place items ----
    let mut max_right = 0.0f64;
    for (item, _) in items {
        let area_w = area_width(&item);
        let end_row = item.row + item.row_span;
        let area_h = row_y[end_row - 1] + row_size[end_row - 1] - row_y[item.row];
        let area_x = col_x[item.col];
        let area_y = row_y[item.row];
        let (ml, mr, mt, mb) = (
            item.margin[3],
            item.margin[1],
            item.margin[0],
            item.margin[2],
        );
        let avail_w = area_w - ml.unwrap_or(0.0) - mr.unwrap_or(0.0);
        let avail_h = area_h - mt.unwrap_or(0.0) - mb.unwrap_or(0.0);

        let stretch_x = item.width_auto
            && !item.replaced
            && matches!(item.justify.as_str(), "normal" | "stretch");
        let forced_w = if stretch_x {
            Some(avail_w.max(0.0))
        } else {
            fit_width(ctx, container, &item, avail_w)
        };
        let cb = Constraint {
            cb_width: area_w,
            cb_height: Some(area_h),
            root: false,
        };
        let (mut fragment, mut pending) =
            lay_out(ctx, container, &item.source, &cb, forced_w, None);
        let stretch_y = item.height_auto
            && !item.replaced
            && mt.is_some()
            && mb.is_some()
            && matches!(item.align.as_str(), "normal" | "stretch");
        if stretch_y && (avail_h - fragment.height).abs() > 0.01 && avail_h > fragment.height {
            let relaid = lay_out(ctx, container, &item.source, &cb, forced_w, Some(avail_h));
            fragment = relaid.0;
            pending = relaid.1;
        }

        let place = |auto_start: bool,
                     auto_end: bool,
                     start_margin: f64,
                     free: f64,
                     mode: &str,
                     normal_start: bool|
         -> f64 {
            let free = free.max(0.0);
            if auto_start || auto_end {
                return match (auto_start, auto_end) {
                    (true, true) => free / 2.0,
                    (true, false) => free,
                    _ => 0.0,
                } + start_margin;
            }
            let off = match mode {
                "end" | "flex-end" | "self-end" | "right" => free,
                "center" => free / 2.0,
                "normal" if !normal_start => 0.0,
                _ => 0.0,
            };
            off + start_margin
        };
        let free_x = avail_w - fragment.width;
        let free_y = avail_h - fragment.height;
        let off_x = place(
            ml.is_none(),
            mr.is_none(),
            ml.unwrap_or(0.0),
            free_x,
            &item.justify,
            item.replaced,
        );
        let off_y = place(
            mt.is_none(),
            mb.is_none(),
            mt.unwrap_or(0.0),
            free_y,
            &item.align,
            item.replaced,
        );
        fragment.x = area_x + off_x;
        fragment.y = area_y + off_y;
        max_right = max_right.max(fragment.x + fragment.width + mr.unwrap_or(0.0));
        for p in pending.drain(..) {
            result.pending_abs.push(PendingAbs {
                node: p.node,
                x: p.x + fragment.x,
                y: p.y + fragment.y,
            });
        }
        result.children.push(fragment);
    }
    result.extent = max_right.max(total_width);
    result.height = match input.definite_height {
        Some(h) => h,
        None => total_height
            .max(input.min_height)
            .min(input.max_height.unwrap_or(f64::INFINITY)),
    };
    result
}

/// The width an item that does not stretch takes in `available` room:
/// its own `width` (left to the box), otherwise fit-content.
fn fit_width(ctx: &Ctx<'_>, container: NodeId, item: &GridItem, available: f64) -> Option<f64> {
    if !item.width_auto {
        return None;
    }
    let (min, max) = content_widths(ctx, container, &item.source);
    if matches!(item.justify.as_str(), "normal" | "stretch") && !item.replaced {
        return Some(available.max(0.0));
    }
    Some(max.min(min.max(available)).max(0.0))
}

#[cfg(test)]
mod tests {
    use crate::block::layout_block;
    use crate::fragment::Fragment;
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::{NodeData, NodeId};

    fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    fn grid(items: &str, css: &str, width: f64) -> Fragment {
        let html = format!("<div class=g>{items}</div>");
        let doc = blueice_html::parse(&html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!(
            "body {{ margin: 0; }} .g {{ display: grid; }} i {{ display: block; }} {css}"
        ))
        .rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, width).children.remove(0)
    }

    fn boxes(f: &Fragment) -> Vec<(f64, f64, f64, f64)> {
        f.children
            .iter()
            .map(|c| (c.x.round(), c.y.round(), c.width.round(), c.height.round()))
            .collect()
    }

    const FOUR: &str = "<i style='height:10px'></i><i style='height:20px'></i><i style='height:10px'></i><i style='height:10px'></i>";

    #[test]
    fn fixed_and_fraction_tracks_share_the_width() {
        let f = grid(FOUR, ".g { grid-template-columns: 100px 1fr 2fr; }", 400.0);
        let b = boxes(&f);
        assert_eq!(b[0], (0.0, 0.0, 100.0, 10.0));
        assert_eq!(b[1], (100.0, 0.0, 100.0, 20.0));
        assert_eq!(b[2], (200.0, 0.0, 200.0, 10.0));
        assert_eq!(b[3], (0.0, 20.0, 100.0, 10.0));
    }

    #[test]
    fn repeat_and_gap_space_the_tracks() {
        let f = grid(
            FOUR,
            ".g { grid-template-columns: repeat(2, 1fr); gap: 10px 20px; }",
            420.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0].2, 200.0);
        assert_eq!(b[1].0, 220.0);
        assert_eq!(
            b[2].1, 30.0,
            "row 2 starts after row 1 (20) and the 10px gap"
        );
        assert_eq!(f.height.round(), 40.0);
    }

    #[test]
    fn minmax_floors_a_fraction_track() {
        let f = grid(
            "<i style='width:150px;height:5px'></i><i style='height:5px'></i>",
            ".g { grid-template-columns: minmax(150px, 1fr) 1fr; }",
            200.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0].2, 150.0);
        assert_eq!(b[1].2, 50.0);
    }

    #[test]
    fn auto_fill_makes_as_many_tracks_as_fit() {
        let f = grid(
            FOUR,
            ".g { grid-template-columns: repeat(auto-fill, minmax(100px, 1fr)); }",
            350.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0].2.round(), 117.0, "three 100px tracks grow to 350/3");
        assert_eq!(b[3].1, 20.0, "the fourth item wraps to a second row");
    }

    #[test]
    fn auto_fit_collapses_empty_tracks() {
        let f = grid(
            "<i style='height:5px'></i><i style='height:5px'></i>",
            ".g { grid-template-columns: repeat(auto-fit, minmax(100px, 1fr)); }",
            500.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0].2, 250.0, "two filled tracks share the row");
        assert_eq!(b[1].0, 250.0);
    }

    #[test]
    fn lines_and_spans_place_items() {
        let f = grid(
            "<i class=a style='height:10px'></i><i class=b style='height:10px'></i>",
            ".g { grid-template-columns: repeat(4, 50px); } \
             .a { grid-column: 2 / span 2; } .b { grid-column: -2 / -1; grid-row: 2; }",
            400.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0], (50.0, 0.0, 100.0, 10.0));
        assert_eq!(b[1], (150.0, 10.0, 50.0, 10.0));
    }

    #[test]
    fn named_areas_position_items() {
        let f = grid(
            "<i class=h style='height:10px'></i><i class=s style='height:30px'></i><i class=m style='height:30px'></i>",
            ".g { grid-template-columns: 100px 1fr; \
               grid-template-areas: 'head head' 'side main'; } \
             .h { grid-area: head; } .s { grid-area: side; } .m { grid-area: main; }",
            300.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0], (0.0, 0.0, 300.0, 10.0));
        assert_eq!(b[1], (0.0, 10.0, 100.0, 30.0));
        assert_eq!(b[2], (100.0, 10.0, 200.0, 30.0));
    }

    #[test]
    fn auto_rows_size_to_content_and_fixed_rows_hold() {
        let f = grid(
            "<i></i><i></i><i style='height:10px'></i>",
            ".g { grid-template-columns: 1fr 1fr; grid-template-rows: 50px; }",
            200.0,
        );
        let b = boxes(&f);
        assert_eq!(b[0].3, 50.0, "the item stretches to its fixed row");
        assert_eq!(b[2].1, 50.0);
        assert_eq!(b[2].3, 10.0, "the implicit second row is auto");
    }

    #[test]
    fn column_flow_fills_columns_first() {
        let f = grid(
            FOUR,
            ".g { grid-template-rows: repeat(2, 20px); grid-auto-flow: column; grid-auto-columns: 30px; }",
            400.0,
        );
        let b = boxes(&f);
        assert_eq!((b[0].0, b[0].1), (0.0, 0.0));
        assert_eq!((b[1].0, b[1].1), (0.0, 20.0));
        assert_eq!((b[2].0, b[2].1), (30.0, 0.0));
    }

    #[test]
    fn alignment_places_items_in_their_areas() {
        let f = grid(
            "<i style='width:20px;height:10px'></i>",
            ".g { grid-template-columns: 100px; grid-template-rows: 50px; justify-items: center; align-items: end; }",
            400.0,
        );
        assert_eq!(boxes(&f)[0], (40.0, 40.0, 20.0, 10.0));
    }

    #[test]
    fn auto_margins_centre_an_item() {
        let f = grid(
            "<i style='width:20px;height:10px;margin:auto'></i>",
            ".g { grid-template-columns: 100px; grid-template-rows: 50px; }",
            400.0,
        );
        assert_eq!(boxes(&f)[0], (40.0, 20.0, 20.0, 10.0));
    }

    #[test]
    fn dense_packing_backfills_holes() {
        let items = "<i class=w style='height:5px'></i><i style='height:5px'></i><i style='height:5px'></i>";
        let css = ".g { grid-template-columns: repeat(2, 50px); } .w { grid-column: span 2; } .w + i { grid-column: 2; }";
        let sparse = grid(items, css, 400.0);
        let dense = grid(
            items,
            &format!("{css} .g {{ grid-auto-flow: row dense; }}"),
            400.0,
        );
        assert_eq!(boxes(&sparse)[2].1, 10.0);
        assert_eq!(boxes(&dense)[2].0, 0.0, "dense fills the hole in row two");
    }

    #[test]
    fn a_grid_shrinks_to_fit_when_inline() {
        let html = "<div style='display:inline-grid;grid-template-columns:30px 40px'><i style='height:5px'></i><i style='height:5px'></i></div>";
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse("body{margin:0} i{display:block}").rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        let f = layout_block(&doc, body, &styles, 500.0);
        let line = &f.children[0];
        assert_eq!(line.children[0].width.round(), 70.0);
    }

    #[test]
    fn fraction_tracks_use_content_when_measured() {
        let html = "<div style='display:flex'><div class=g><i style='width:30px;height:5px'></i><i style='width:40px;height:5px'></i></div></div>";
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(
            "body{margin:0} i{display:block} .g{display:grid;grid-template-columns:1fr 1fr}",
        )
        .rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        let f = layout_block(&doc, body, &styles, 500.0);
        let g = &f.children[0].children[0];
        assert!(
            g.width <= 80.5,
            "a grid in a flex item is content-sized, got {}",
            g.width
        );
    }
}
