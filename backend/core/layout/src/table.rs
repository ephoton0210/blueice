// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS 2 table layout (automatic algorithm, separated borders): rows and row
//! groups are collected from the table's children, cells are placed on a grid
//! honouring `colspan`/`rowspan`, columns are sized between their cells'
//! min-content and max-content widths, and rows take the height of their
//! tallest cell. `table-layout: fixed` is treated as automatic and collapsed
//! borders as zero spacing; neither is a claim of conformance.

use crate::block::{layout_block_level, Constraint, Forced, PendingAbs};
use crate::ctx::{Ctx, Display};
use crate::fragment::{Fragment, FragmentKind};
use blueice_css::Value;
use blueice_dom::{NodeData, NodeId};

/// A forced width at or beyond this means "measure with unlimited room".
const UNBOUNDED_FROM: f64 = 1.0e5;

pub(crate) struct TableInput {
    /// Content-box width the table may take.
    pub available: f64,
    /// The table's own content-box width when its `width` is not auto.
    pub specified: Option<f64>,
    /// A width imposed from outside (shrink-to-fit measuring, a flex item).
    pub forced: Option<f64>,
    pub definite_height: Option<f64>,
}

pub(crate) struct TableLayout {
    pub children: Vec<Fragment>,
    /// Content-box width the table ended up with.
    pub width: f64,
    pub height: f64,
    pub pending_abs: Vec<PendingAbs>,
}

struct Cell {
    node: NodeId,
    row: usize,
    col: usize,
    rowspan: usize,
    colspan: usize,
    min: f64,
    max: f64,
    percent: Option<f64>,
}

struct Row {
    node: Option<NodeId>,
    group: Option<usize>,
    cells: Vec<NodeId>,
}

struct Group {
    node: NodeId,
}

fn span_attr(ctx: &Ctx<'_>, node: NodeId, name: &str) -> usize {
    ctx.attr(node, name)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map_or(1, |n| n.clamp(1, 1000))
}

fn is_row_group(display: Display) -> bool {
    display == Display::TableRowGroup
}

/// Rows in visual order: header groups first, footer groups last.
fn collect(ctx: &Ctx<'_>, table: NodeId) -> (Vec<Row>, Vec<Group>, Vec<NodeId>) {
    let mut head: Vec<(Vec<Row>, Option<Group>)> = Vec::new();
    let mut body: Vec<(Vec<Row>, Option<Group>)> = Vec::new();
    let mut foot: Vec<(Vec<Row>, Option<Group>)> = Vec::new();
    let mut captions = Vec::new();
    let mut loose: Vec<NodeId> = Vec::new();
    let mut loose_rows: Vec<Row> = Vec::new();

    fn row_of(ctx: &Ctx<'_>, node: NodeId) -> Row {
        let mut cells = Vec::new();
        for child in ctx.children(node) {
            if matches!(ctx.node(child), NodeData::Element { .. }) {
                let d = ctx.style(child).display();
                if d != Display::None && d != Display::TableColumn {
                    cells.push(child);
                }
            }
        }
        Row {
            node: Some(node),
            group: None,
            cells,
        }
    }
    let flush_loose = |loose: &mut Vec<NodeId>, rows: &mut Vec<Row>| {
        if !loose.is_empty() {
            rows.push(Row {
                node: None,
                group: None,
                cells: std::mem::take(loose),
            });
        }
    };
    for child in ctx.children(table) {
        if !matches!(ctx.node(child), NodeData::Element { .. }) {
            continue;
        }
        let style = ctx.style(child);
        let display = style.display();
        match display {
            Display::None | Display::TableColumn => {}
            Display::TableCaption => captions.push(child),
            Display::TableRow => {
                flush_loose(&mut loose, &mut loose_rows);
                loose_rows.push(row_of(ctx, child));
            }
            d if is_row_group(d) => {
                flush_loose(&mut loose, &mut loose_rows);
                let rows: Vec<Row> = ctx
                    .children(child)
                    .into_iter()
                    .filter(|&r| {
                        matches!(ctx.node(r), NodeData::Element { .. })
                            && ctx.style(r).display() == Display::TableRow
                    })
                    .map(|r| row_of(ctx, r))
                    .collect();
                let entry = (rows, Some(Group { node: child }));
                match style.style.map(|s| s.display.as_str()) {
                    Some("table-header-group") => head.push(entry),
                    Some("table-footer-group") => foot.push(entry),
                    _ => body.push(entry),
                }
            }
            _ => loose.push(child),
        }
        if !loose_rows.is_empty() {
            body.push((std::mem::take(&mut loose_rows), None));
        }
    }
    flush_loose(&mut loose, &mut loose_rows);
    if !loose_rows.is_empty() {
        body.push((loose_rows, None));
    }
    let mut rows = Vec::new();
    let mut groups = Vec::new();
    for (mut group_rows, group) in head.into_iter().chain(body).chain(foot) {
        let index = group.map(|g| {
            groups.push(g);
            groups.len() - 1
        });
        for r in &mut group_rows {
            r.group = index;
        }
        rows.extend(group_rows);
    }
    (rows, groups, captions)
}

fn spacing(ctx: &Ctx<'_>, table: NodeId) -> (f64, f64) {
    let style = ctx.style(table);
    if style.keyword("border-collapse") == Some("collapse") {
        return (0.0, 0.0);
    }
    let one = |v: &Value| {
        v.to_px(&style.length_context(f64::NAN))
            .unwrap_or(0.0)
            .max(0.0)
    };
    match style.raw("border-spacing") {
        Some(Value::List(parts)) if parts.len() == 2 => (one(&parts[0]), one(&parts[1])),
        Some(v) => (one(v), one(v)),
        None => (0.0, 0.0),
    }
}

pub(crate) fn layout_table(ctx: &Ctx<'_>, table: NodeId, input: &TableInput) -> TableLayout {
    let (rows, groups, captions) = collect(ctx, table);
    let (space_x, space_y) = spacing(ctx, table);

    // ---- place cells on the grid ----
    let mut cells: Vec<Cell> = Vec::new();
    let mut occupied: Vec<Vec<bool>> = vec![Vec::new(); rows.len()];
    let mut columns = 0usize;
    for (r, row) in rows.iter().enumerate() {
        let mut col = 0usize;
        for &node in &row.cells {
            while occupied[r].get(col).copied().unwrap_or(false) {
                col += 1;
            }
            let colspan = span_attr(ctx, node, "colspan");
            let rowspan = span_attr(ctx, node, "rowspan").min(rows.len() - r);
            for rr in r..r + rowspan {
                if occupied[rr].len() < col + colspan {
                    occupied[rr].resize(col + colspan, false);
                }
                for cc in col..col + colspan {
                    occupied[rr][cc] = true;
                }
            }
            let style = ctx.style(node);
            let (mut min, mut max) = crate::intrinsic::content_widths(ctx, node);
            let percent = match style.raw("width") {
                Some(Value::Percentage(p)) => Some(*p),
                _ => None,
            };
            if percent.is_none() {
                if let Some(w) = style.px("width", f64::NAN) {
                    let pb = style.padding(0.0).horizontal() + style.border(0.0).horizontal();
                    let w = if style.is_border_box() { w } else { w + pb };
                    max = w.max(min);
                    min = min.min(max);
                }
            }
            cells.push(Cell {
                node,
                row: r,
                col,
                rowspan,
                colspan,
                min,
                max,
                percent,
            });
            columns = columns.max(col + colspan);
            col += colspan;
        }
    }

    let mut result = TableLayout {
        children: Vec::new(),
        width: 0.0,
        height: 0.0,
        pending_abs: Vec::new(),
    };
    if columns == 0 {
        result.width = input
            .specified
            .or(input.forced)
            .unwrap_or(0.0)
            .min(input.available);
        return result;
    }

    // ---- column widths ----
    let mut col_min = vec![0.0f64; columns];
    let mut col_max = vec![0.0f64; columns];
    let mut col_pct = vec![0.0f64; columns];
    for cell in cells.iter().filter(|c| c.colspan == 1) {
        col_min[cell.col] = col_min[cell.col].max(cell.min);
        col_max[cell.col] = col_max[cell.col].max(cell.max);
        if let Some(p) = cell.percent {
            col_pct[cell.col] = col_pct[cell.col].max(p);
        }
    }
    for cell in cells.iter().filter(|c| c.colspan > 1) {
        let range = cell.col..cell.col + cell.colspan;
        let inner = space_x * (cell.colspan - 1) as f64;
        for (need, widths) in [(cell.min, &mut col_min), (cell.max, &mut col_max)] {
            let have: f64 = widths[range.clone()].iter().sum::<f64>() + inner;
            if need > have {
                let extra = (need - have) / cell.colspan as f64;
                for w in &mut widths[range.clone()] {
                    *w += extra;
                }
            }
        }
    }
    for i in 0..columns {
        col_max[i] = col_max[i].max(col_min[i]);
    }
    let total_space = space_x * (columns + 1) as f64;
    let min_total: f64 = col_min.iter().sum::<f64>() + total_space;
    let max_total: f64 = col_max.iter().sum::<f64>() + total_space;

    let table_width = if let Some(w) = input.specified {
        w.max(min_total)
    } else if let Some(w) = input.forced {
        if w >= UNBOUNDED_FROM {
            max_total
        } else {
            w.max(min_total)
        }
    } else {
        max_total.min(input.available).max(min_total)
    };
    let distributable = (table_width - total_space).max(0.0);
    let min_sum: f64 = col_min.iter().sum();
    let max_sum: f64 = col_max.iter().sum();
    let mut col_width: Vec<f64> = if distributable >= max_sum {
        let extra = distributable - max_sum;
        if max_sum > 0.0 && (input.specified.is_some() || input.forced.is_some()) {
            col_max.iter().map(|m| m + extra * m / max_sum).collect()
        } else if input.specified.is_some() || input.forced.is_some() {
            col_max.iter().map(|m| m + extra / columns as f64).collect()
        } else {
            col_max.clone()
        }
    } else if distributable > min_sum && max_sum > min_sum {
        let t = (distributable - min_sum) / (max_sum - min_sum);
        (0..columns)
            .map(|i| col_min[i] + (col_max[i] - col_min[i]) * t)
            .collect()
    } else {
        col_min.clone()
    };
    // Percentage columns claim their share of the table when there is room.
    let pct_total: f64 = col_pct.iter().sum();
    if pct_total > 0.0
        && pct_total <= 100.0
        && (input.specified.is_some() || input.forced.is_some() || distributable >= max_sum)
    {
        let wanted: Vec<f64> = (0..columns)
            .map(|i| {
                if col_pct[i] > 0.0 {
                    (distributable * col_pct[i] / 100.0).max(col_min[i])
                } else {
                    col_width[i]
                }
            })
            .collect();
        let pct_width: f64 = (0..columns)
            .filter(|&i| col_pct[i] > 0.0)
            .map(|i| wanted[i])
            .sum();
        let rest_width: f64 = (0..columns)
            .filter(|&i| col_pct[i] == 0.0)
            .map(|i| col_min[i])
            .sum();
        if pct_width + rest_width <= distributable + 0.01 {
            let rest_have: f64 = (0..columns)
                .filter(|&i| col_pct[i] == 0.0)
                .map(|i| col_width[i])
                .sum();
            let rest_room = distributable - pct_width;
            for i in 0..columns {
                if col_pct[i] > 0.0 {
                    col_width[i] = wanted[i];
                } else if rest_have > 0.0 {
                    col_width[i] = (col_width[i] * rest_room / rest_have).max(col_min[i]);
                }
            }
        }
    }
    let used_width: f64 = col_width.iter().sum::<f64>() + total_space;
    let table_width = if input.specified.is_some() || input.forced.is_some() {
        table_width.max(used_width.min(table_width))
    } else {
        used_width
    };
    result.width = table_width;

    let mut col_x = Vec::with_capacity(columns);
    let mut cursor = space_x;
    for w in &col_width {
        col_x.push(cursor);
        cursor += w + space_x;
    }
    let span_width = |cell: &Cell| -> f64 {
        col_width[cell.col..cell.col + cell.colspan]
            .iter()
            .sum::<f64>()
            + space_x * (cell.colspan - 1) as f64
    };

    // ---- captions above the table ----
    let mut top = 0.0f64;
    let caption_constraint = Constraint {
        cb_width: table_width,
        cb_height: None,
        root: false,
    };
    for &caption in &captions {
        let placed = layout_block_level(
            ctx,
            caption,
            &caption_constraint,
            Forced {
                width: Some(table_width),
                height: None,
            },
        );
        let mut fragment = placed.fragment;
        fragment.x = 0.0;
        fragment.y = top;
        top += fragment.height;
        result
            .pending_abs
            .extend(placed.pending_abs.into_iter().map(|p| PendingAbs {
                node: p.node,
                x: p.x,
                y: p.y + fragment.y,
            }));
        result.children.push(fragment);
    }

    // ---- lay cells out at their column widths and measure rows ----
    let cell_constraint = Constraint {
        cb_width: table_width,
        cb_height: None,
        root: false,
    };
    let mut laid: Vec<(crate::block::Placed, f64)> = Vec::with_capacity(cells.len());
    for cell in &cells {
        let width = span_width(cell);
        let placed = layout_block_level(
            ctx,
            cell.node,
            &cell_constraint,
            Forced {
                width: Some(width),
                height: None,
            },
        );
        let height = placed.fragment.height;
        laid.push((placed, height));
    }
    let mut row_height = vec![0.0f64; rows.len()];
    for (i, cell) in cells.iter().enumerate() {
        let style = ctx.style(cell.node);
        let specified = style.px("height", f64::NAN).map_or(0.0, |h| {
            if style.is_border_box() {
                h
            } else {
                h + style.padding(0.0).vertical() + style.border(0.0).vertical()
            }
        });
        let wanted = laid[i].1.max(specified);
        if cell.rowspan == 1 {
            row_height[cell.row] = row_height[cell.row].max(wanted);
        }
    }
    for (r, row) in rows.iter().enumerate() {
        if let Some(node) = row.node {
            let style = ctx.style(node);
            if let Some(h) = style.px("height", f64::NAN) {
                row_height[r] = row_height[r].max(h);
            }
        }
    }
    for (i, cell) in cells.iter().enumerate().filter(|(_, c)| c.rowspan > 1) {
        let range = cell.row..cell.row + cell.rowspan;
        let have: f64 =
            row_height[range.clone()].iter().sum::<f64>() + space_y * (cell.rowspan - 1) as f64;
        if laid[i].1 > have {
            row_height[cell.row + cell.rowspan - 1] += laid[i].1 - have;
        }
    }
    let rows_total: f64 = row_height.iter().sum::<f64>() + space_y * (rows.len() + 1) as f64;
    if let Some(definite) = input.definite_height {
        let wanted = definite - top;
        if wanted > rows_total && !rows.is_empty() {
            let extra = (wanted - rows_total) / rows.len() as f64;
            for h in &mut row_height {
                *h += extra;
            }
        }
    }
    let mut row_y = Vec::with_capacity(rows.len());
    let mut y = top + space_y;
    for h in &row_height {
        row_y.push(y);
        y += h + space_y;
    }
    let total_height = y;

    // ---- fragments: groups contain rows contain cells ----
    let mut row_fragments: Vec<Fragment> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| Fragment {
            node: row.node,
            kind: FragmentKind::Block,
            x: space_x,
            y: row_y[r],
            width: table_width - 2.0 * space_x,
            height: row_height[r],
            ..Fragment::empty_block()
        })
        .collect();
    for (cell, (placed, natural)) in cells.iter().zip(laid) {
        let mut fragment = placed.fragment;
        let slot: f64 = row_height[cell.row..cell.row + cell.rowspan]
            .iter()
            .sum::<f64>()
            + space_y * (cell.rowspan - 1) as f64;
        let style = ctx.style(cell.node);
        let shift = match style.keyword("vertical-align") {
            Some("middle") => (slot - natural).max(0.0) / 2.0,
            Some("bottom") => (slot - natural).max(0.0),
            _ => 0.0,
        };
        if shift > 0.0 {
            for child in &mut fragment.children {
                child.y += shift;
            }
        }
        fragment.height = slot.max(fragment.height);
        fragment.x = col_x[cell.col] - space_x;
        fragment.y = 0.0;
        let origin_x = space_x + fragment.x;
        let origin_y = row_y[cell.row];
        result
            .pending_abs
            .extend(placed.pending_abs.into_iter().map(|p| PendingAbs {
                node: p.node,
                x: p.x + origin_x,
                y: p.y + origin_y,
            }));
        row_fragments[cell.row].children.push(fragment);
    }
    // Group fragments wrap their rows.
    let mut group_fragments: Vec<Option<Fragment>> = vec![None; groups.len()];
    let mut loose: Vec<Fragment> = Vec::new();
    for (r, mut fragment) in row_fragments.into_iter().enumerate() {
        match rows[r].group {
            Some(g) => {
                let group = group_fragments[g].get_or_insert_with(|| Fragment {
                    node: Some(groups[g].node),
                    kind: FragmentKind::Block,
                    x: space_x,
                    y: fragment.y,
                    width: table_width - 2.0 * space_x,
                    height: 0.0,
                    ..Fragment::empty_block()
                });
                fragment.x = 0.0;
                fragment.y -= group.y;
                group.height = fragment.y + fragment.height;
                group.children.push(fragment);
            }
            None => loose.push(fragment),
        }
    }
    for group in group_fragments.into_iter().flatten() {
        result.children.push(group);
    }
    result.children.extend(loose);
    result.height = total_height;
    result
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

    /// The table fragment, the first child of the body, at `width`.
    fn table(html: &str, css: &str, width: f64) -> Fragment {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!(
            "body,p {{ margin: 0; }} table {{ border-spacing: 0; }} td, th {{ padding: 0; }} \
             i {{ display: block; height: 20px; }} {css}"
        ))
        .rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, width).children.remove(0)
    }

    /// Cells in document order as `(x, y, width, height)` relative to the table.
    fn cells(f: &Fragment) -> Vec<(f64, f64, f64, f64)> {
        let mut out = Vec::new();
        for group in &f.children {
            for row in &group.children {
                for cell in &row.children {
                    out.push((
                        (group.x + row.x + cell.x).round(),
                        (group.y + row.y + cell.y).round(),
                        cell.width.round(),
                        cell.height.round(),
                    ));
                }
            }
        }
        out
    }

    const TWO_BY_TWO: &str =
        "<table><tr><td><i style='width:50px'></i></td><td><i style='width:80px'></i></td></tr>\
        <tr><td><i style='width:70px'></i></td><td><i style='width:10px'></i></td></tr></table>";

    #[test]
    fn columns_take_the_widest_cell_and_rows_the_tallest() {
        let f = table(TWO_BY_TWO, "", 800.0);
        assert_eq!(
            cells(&f),
            vec![
                (0.0, 0.0, 70.0, 20.0),
                (70.0, 0.0, 80.0, 20.0),
                (0.0, 20.0, 70.0, 20.0),
                (70.0, 20.0, 80.0, 20.0)
            ]
        );
        assert_eq!((f.width.round(), f.height.round()), (150.0, 40.0));
    }

    #[test]
    fn border_spacing_surrounds_and_separates_cells() {
        let f = table(TWO_BY_TWO, "table { border-spacing: 5px; }", 800.0);
        let c = cells(&f);
        assert_eq!(c[0], (5.0, 5.0, 70.0, 20.0));
        assert_eq!(c[1], (80.0, 5.0, 80.0, 20.0));
        assert_eq!(c[2].1, 30.0);
        assert_eq!((f.width.round(), f.height.round()), (165.0, 55.0));
    }

    #[test]
    fn a_specified_width_shares_the_extra_room_in_proportion() {
        let f = table(TWO_BY_TWO, "table { width: 300px; }", 800.0);
        assert_eq!(f.width.round(), 300.0);
        let c = cells(&f);
        assert_eq!(c[0].2 + c[1].2, 300.0);
        assert!(c[1].2 > c[0].2, "the wider column stays wider");
    }

    #[test]
    fn a_narrow_container_squeezes_columns_toward_their_minimum() {
        let f = table(TWO_BY_TWO, "", 100.0);
        assert_eq!(
            f.width.round(),
            150.0,
            "block content cannot shrink below its width"
        );
    }

    #[test]
    fn colspan_widens_the_columns_it_covers() {
        let html = "<table><tr><td colspan=2><i style='width:200px'></i></td></tr>\
            <tr><td><i style='width:20px'></i></td><td><i style='width:30px'></i></td></tr></table>";
        let f = table(html, "", 800.0);
        let c = cells(&f);
        assert_eq!(c[0], (0.0, 0.0, 200.0, 20.0));
        assert_eq!(c[1].2 + c[2].2, 200.0);
    }

    #[test]
    fn rowspan_takes_the_rows_below_it() {
        let html = "<table><tr><td rowspan=2><i style='height:70px;width:10px'></i></td>\
            <td><i style='width:20px'></i></td></tr><tr><td><i style='width:20px'></i></td></tr></table>";
        let f = table(html, "", 800.0);
        let c = cells(&f);
        assert_eq!(c[0].3, 70.0, "the spanning cell covers both rows");
        assert_eq!(c[1].0, 10.0);
        assert_eq!(
            c[2].0, 10.0,
            "the second row's cell skips the occupied column"
        );
        assert_eq!(c[2].1, c[1].3, "second row starts under the first");
    }

    #[test]
    fn cells_align_vertically_within_the_row() {
        let html = "<table><tr><td><i style='height:60px;width:10px'></i></td>\
            <td class=m><i style='width:10px'></i></td></tr></table>";
        let f = table(html, ".m { vertical-align: middle; }", 800.0);
        let row = &f.children[0].children[0];
        let cell = &row.children[1];
        assert_eq!(cell.height.round(), 60.0, "cells fill the row");
        assert_eq!(cell.children[0].y.round(), 20.0, "content centred");
    }

    #[test]
    fn header_rows_come_first_and_footer_rows_last() {
        let html = "<table><tfoot><tr><td><i style='width:10px'></i></td></tr></tfoot>\
            <tbody><tr><td><i style='width:10px;height:30px'></i></td></tr></tbody>\
            <thead><tr><td><i style='width:10px;height:10px'></i></td></tr></thead></table>";
        let f = table(html, "", 800.0);
        let c = cells(&f);
        assert_eq!(
            c.iter().map(|c| c.1).collect::<Vec<_>>(),
            vec![0.0, 10.0, 40.0]
        );
    }

    #[test]
    fn cells_outside_a_row_form_an_anonymous_row() {
        let html = "<div style='display:table'><div style='display:table-cell'><i style='width:30px'></i></div>\
            <div style='display:table-cell'><i style='width:40px'></i></div></div>";
        let f = table(html, "", 800.0);
        assert_eq!(f.width.round(), 70.0);
    }

    #[test]
    fn captions_sit_above_the_rows() {
        let html = "<table><caption><i style='height:15px'></i></caption>\
            <tr><td><i style='width:50px'></i></td></tr></table>";
        let f = table(html, "", 800.0);
        let c = cells(&f);
        assert_eq!(c[0].1, 15.0);
        assert_eq!(f.height.round(), 35.0);
    }

    #[test]
    fn a_table_with_auto_margins_centres() {
        let f = table(TWO_BY_TWO, "table { margin: 0 auto; }", 800.0);
        assert_eq!(f.x.round(), 325.0);
    }

    #[test]
    fn percentage_columns_take_their_share() {
        let html =
            "<table style='width:400px'><tr><td style='width:25%'><i style='width:10px'></i></td>\
            <td><i style='width:10px'></i></td></tr></table>";
        let f = table(html, "", 800.0);
        assert_eq!(cells(&f)[0].2, 100.0);
    }
}
