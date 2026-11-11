// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Canvas paths: the geometry of `fill()` and `stroke()`. A path is a list of subpaths of
//! user-space points (curves are already flattened by the script side); filling turns them
//! into polygons and a coverage mask, stroking first turns them into the polygons a line of
//! the given width covers. The mask is anti-aliased by sampling each pixel row four times
//! with exact horizontal overlap.

pub(crate) type Point = (f64, f64);

/// One subpath: its points and whether it was closed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Subpath {
    pub points: Vec<Point>,
    pub closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Join {
    Miter,
    Round,
    Bevel,
}

pub(crate) struct StrokeStyle {
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
}

/// A mask of coverage 0..=255 over the pixel box starting at `(x, y)`.
pub(crate) struct Mask {
    pub x: i64,
    pub y: i64,
    pub w: usize,
    pub h: usize,
    pub coverage: Vec<u8>,
}

fn area(poly: &[Point]) -> f64 {
    let mut sum = 0.0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        sum += a.0 * b.1 - b.0 * a.1;
    }
    sum / 2.0
}

/// Every polygon wound the same way, so that overlapping ones add up under the non-zero rule.
fn upright(mut poly: Vec<Point>) -> Vec<Point> {
    if area(&poly) < 0.0 {
        poly.reverse();
    }
    poly
}

fn disk(centre: Point, radius: f64) -> Vec<Point> {
    let n = ((radius * 4.0).ceil() as usize).clamp(8, 48);
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
        })
        .collect()
}

/// The polygons that fill `paths`: each subpath is closed implicitly.
pub(crate) fn fill_polygons(paths: &[Subpath]) -> Vec<Vec<Point>> {
    paths.iter().filter(|p| p.points.len() >= 3).map(|p| p.points.clone()).collect()
}

/// The most dashes one stroke is cut into, so a tiny pattern on a long path stays cheap.
const MAX_DASHES: usize = 100_000;

/// `paths` cut into the dashes of `pattern` (alternating on and off lengths), starting
/// `offset` into the pattern. An odd-length pattern is repeated; an empty one, one with a
/// negative or non-finite length, or one of no total length leaves the paths solid.
pub(crate) fn dash_paths(paths: &[Subpath], pattern: &[f64], offset: f64) -> Vec<Subpath> {
    if pattern.is_empty() || pattern.iter().any(|d| !d.is_finite() || *d < 0.0) || !offset.is_finite() {
        return paths.to_vec();
    }
    let mut items: Vec<f64> = pattern.to_vec();
    if items.len() % 2 == 1 {
        items.extend_from_slice(pattern);
    }
    let total: f64 = items.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return paths.to_vec();
    }
    let mut out: Vec<Subpath> = Vec::new();
    for path in paths {
        let mut pts = path.points.clone();
        if path.closed && pts.len() > 1 {
            pts.push(pts[0]);
        }
        if pts.len() < 2 {
            out.push(path.clone());
            continue;
        }
        // Where in the pattern the path begins.
        let mut index = 0;
        let mut left = items[0];
        let mut skip = offset.rem_euclid(total);
        while skip > 0.0 {
            if skip < left {
                left -= skip;
                break;
            }
            skip -= left;
            index = (index + 1) % items.len();
            left = items[index];
        }
        let mut current: Vec<Point> = Vec::new();
        let flush = |current: &mut Vec<Point>, item: f64, out: &mut Vec<Subpath>| {
            if current.len() >= 2 || (current.len() == 1 && item == 0.0) {
                out.push(Subpath { points: std::mem::take(current), closed: false });
            }
            current.clear();
        };
        if index % 2 == 0 {
            current.push(pts[0]);
        }
        'path: for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            if len <= 0.0 {
                continue;
            }
            let mut at = 0.0;
            while len - at > left {
                at += left;
                let t = at / len;
                let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                if index % 2 == 0 {
                    current.push(p);
                    flush(&mut current, items[index], &mut out);
                } else {
                    current.push(p);
                }
                if out.len() >= MAX_DASHES {
                    break 'path;
                }
                index = (index + 1) % items.len();
                left = items[index];
            }
            left -= len - at;
            if index % 2 == 0 {
                current.push(b);
            }
        }
        if index % 2 == 0 {
            flush(&mut current, items[index], &mut out);
        }
        if out.len() >= MAX_DASHES {
            break;
        }
    }
    out
}

/// The polygons whose union is the stroke of `paths`.
pub(crate) fn stroke_polygons(paths: &[Subpath], style: &StrokeStyle) -> Vec<Vec<Point>> {
    let half = style.width / 2.0;
    let mut out: Vec<Vec<Point>> = Vec::new();
    if half.is_nan() || half <= 0.0 || !half.is_finite() {
        return out;
    }
    for path in paths {
        // Drop repeated points: a zero-length segment has no direction.
        let mut pts: Vec<Point> = Vec::new();
        for &p in &path.points {
            if pts.last().is_none_or(|l| (l.0 - p.0).hypot(l.1 - p.1) > 1e-9) {
                pts.push(p);
            }
        }
        if path.closed && pts.len() > 1 && (pts[0].0 - pts[pts.len() - 1].0).hypot(pts[0].1 - pts[pts.len() - 1].1) <= 1e-9 {
            pts.pop();
        }
        if pts.len() == 1 {
            // A point: a dot with round caps, a square with square caps, nothing with butt.
            match style.cap {
                Cap::Round => out.push(disk(pts[0], half)),
                Cap::Square => {
                    let (x, y) = pts[0];
                    out.push(vec![(x - half, y - half), (x + half, y - half), (x + half, y + half), (x - half, y + half)]);
                }
                Cap::Butt => {}
            }
            continue;
        }
        if pts.len() < 2 {
            continue;
        }
        let count = if path.closed { pts.len() } else { pts.len() - 1 };
        let seg = |i: usize| (pts[i], pts[(i + 1) % pts.len()]);
        for i in 0..count {
            let (a, b) = seg(i);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = dx.hypot(dy);
            let (ux, uy) = (dx / len, dy / len);
            let (nx, ny) = (-uy * half, ux * half);
            // Square caps stick out past the end points of an open path.
            let (mut a, mut b) = (a, b);
            if !path.closed && style.cap == Cap::Square {
                if i == 0 {
                    a = (a.0 - ux * half, a.1 - uy * half);
                }
                if i == count - 1 {
                    b = (b.0 + ux * half, b.1 + uy * half);
                }
            }
            out.push(upright(vec![(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]));
        }
        if !path.closed && style.cap == Cap::Round {
            out.push(disk(pts[0], half));
            out.push(disk(pts[pts.len() - 1], half));
        }
        // Joins at every vertex where two segments meet.
        let joins: Vec<usize> = if path.closed { (0..pts.len()).collect() } else { (1..pts.len() - 1).collect() };
        for v in joins {
            let prev = pts[(v + pts.len() - 1) % pts.len()];
            let (p, next) = (pts[v], pts[(v + 1) % pts.len()]);
            let d1 = ((p.0 - prev.0), (p.1 - prev.1));
            let d2 = ((next.0 - p.0), (next.1 - p.1));
            let (l1, l2) = (d1.0.hypot(d1.1), d2.0.hypot(d2.1));
            let (u1, u2) = ((d1.0 / l1, d1.1 / l1), (d2.0 / l2, d2.1 / l2));
            if style.join == Join::Round {
                out.push(disk(p, half));
                continue;
            }
            for side in [1.0, -1.0] {
                let n1 = (-u1.1 * half * side, u1.0 * half * side);
                let n2 = (-u2.1 * half * side, u2.0 * half * side);
                let (a, b) = ((p.0 + n1.0, p.1 + n1.1), (p.0 + n2.0, p.1 + n2.1));
                let mut poly = vec![p, a];
                if style.join == Join::Miter {
                    // The offset lines meet at p + (n1 + n2) / (1 + cos); the miter is as long as
                    // 1 / sin(half the angle between the segments).
                    let cos = u1.0 * u2.0 + u1.1 * u2.1;
                    let ratio = 1.0 / ((1.0 + cos) / 2.0).max(1e-9).sqrt();
                    if ratio <= style.miter_limit {
                        let k = 1.0 + cos;
                        poly.push((p.0 + (n1.0 + n2.0) / k, p.1 + (n1.1 + n2.1) / k));
                    }
                }
                poly.push(b);
                out.push(upright(poly));
            }
        }
    }
    out
}

/// The coverage of `polygons` (device space), clipped to a `width` by `height` bitmap.
pub(crate) fn rasterize(polygons: &[Vec<Point>], even_odd: bool, width: u32, height: u32) -> Option<Mask> {
    let mut edges: Vec<(Point, Point)> = Vec::new();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for poly in polygons {
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
                return None;
            }
            min_x = min_x.min(a.0);
            max_x = max_x.max(a.0);
            min_y = min_y.min(a.1);
            max_y = max_y.max(a.1);
            if a.1 != b.1 {
                edges.push((a, b));
            }
        }
    }
    if edges.is_empty() {
        return None;
    }
    let x0 = (min_x.floor() as i64).max(0);
    let y0 = (min_y.floor() as i64).max(0);
    let x1 = (max_x.ceil() as i64).min(i64::from(width));
    let y1 = (max_y.ceil() as i64).min(i64::from(height));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut acc = vec![0.0_f32; w * h];
    const SAMPLES: usize = 4;
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for row in 0..h {
        for s in 0..SAMPLES {
            let y = (y0 as usize + row) as f64 + (s as f64 + 0.5) / SAMPLES as f64;
            crossings.clear();
            for &(a, b) in &edges {
                let (lo, hi, dir) = if a.1 < b.1 { (a, b, 1) } else { (b, a, -1) };
                if y >= lo.1 && y < hi.1 {
                    let t = (y - lo.1) / (hi.1 - lo.1);
                    crossings.push((lo.0 + t * (hi.0 - lo.0), dir));
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                let inside = if even_odd { (winding & 1) != 0 } else { winding != 0 };
                if !inside {
                    continue;
                }
                let (xa, xb) = (pair[0].0.max(x0 as f64), pair[1].0.min(x1 as f64));
                if xb <= xa {
                    continue;
                }
                let first = xa.floor() as i64;
                let last = (xb.ceil() as i64).min(x1);
                for px in first..last {
                    let overlap = (xb.min((px + 1) as f64) - xa.max(px as f64)).max(0.0);
                    acc[row * w + (px - x0) as usize] += (overlap / SAMPLES as f64) as f32;
                }
            }
        }
    }
    let coverage = acc.iter().map(|&c| (c.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    Some(Mask { x: x0, y: y0, w, h, coverage })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Subpath {
        Subpath { points: vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)], closed: true }
    }

    fn at(m: &Mask, x: i64, y: i64) -> u8 {
        if x < m.x || y < m.y || x >= m.x + m.w as i64 || y >= m.y + m.h as i64 {
            return 0;
        }
        m.coverage[(y - m.y) as usize * m.w + (x - m.x) as usize]
    }

    #[test]
    fn a_square_covers_its_pixels_and_partial_edges_get_partial_coverage() {
        let m = rasterize(&fill_polygons(&[square(2.0, 2.0, 4.0)]), false, 20, 20).unwrap();
        assert_eq!(at(&m, 3, 3), 255);
        assert_eq!(at(&m, 1, 3), 0);
        assert_eq!(at(&m, 6, 3), 0);
        let half = rasterize(&fill_polygons(&[square(2.5, 2.0, 4.0)]), false, 20, 20).unwrap();
        assert!((i32::from(at(&half, 2, 3)) - 128).abs() <= 2, "{}", at(&half, 2, 3));
        assert!((i32::from(at(&half, 6, 3)) - 128).abs() <= 2);
        assert_eq!(at(&half, 4, 3), 255);
    }

    #[test]
    fn nested_squares_make_a_hole_only_by_the_even_odd_rule_or_opposite_winding() {
        let mut inner = square(5.0, 5.0, 4.0);
        let outer = square(2.0, 2.0, 10.0);
        let polys = fill_polygons(&[outer.clone(), inner.clone()]);
        let nz = rasterize(&polys, false, 20, 20).unwrap();
        let eo = rasterize(&polys, true, 20, 20).unwrap();
        assert_eq!(at(&nz, 6, 6), 255, "same winding fills the middle");
        assert_eq!(at(&eo, 6, 6), 0, "even-odd leaves a hole");
        assert_eq!(at(&eo, 3, 3), 255);
        inner.points.reverse();
        let opposite = rasterize(&fill_polygons(&[outer, inner]), false, 20, 20).unwrap();
        assert_eq!(at(&opposite, 6, 6), 0, "opposite winding cancels");
    }

    fn style(width: f64, cap: Cap, join: Join) -> StrokeStyle {
        StrokeStyle { width, cap, join, miter_limit: 10.0 }
    }

    #[test]
    fn a_stroked_line_is_as_wide_as_the_line_width_and_caps_change_its_ends() {
        let line = [Subpath { points: vec![(4.0, 10.0), (14.0, 10.0)], closed: false }];
        let cover = |cap| rasterize(&stroke_polygons(&line, &style(4.0, cap, Join::Miter)), false, 30, 30).unwrap();
        let butt = cover(Cap::Butt);
        assert_eq!((at(&butt, 8, 7), at(&butt, 8, 8), at(&butt, 8, 11), at(&butt, 8, 12)), (0, 255, 255, 0));
        assert_eq!(at(&butt, 3, 10), 0);
        assert_eq!(at(&butt, 14, 10), 0);
        let square = cover(Cap::Square);
        assert_eq!(at(&square, 3, 10), 255, "a square cap goes half a line past the end");
        assert_eq!(at(&square, 15, 10), 255);
        let round = cover(Cap::Round);
        assert_eq!(at(&round, 3, 10), 255);
        assert!(at(&round, 2, 8) < 128 && at(&square, 2, 8) == 255, "a round cap has no corner, a square one does");
    }

    #[test]
    fn joins_fill_the_outside_of_a_corner_by_the_join_style() {
        let corner = [Subpath { points: vec![(5.0, 20.0), (5.0, 5.0), (20.0, 5.0)], closed: false }];
        let join = |j| rasterize(&stroke_polygons(&corner, &style(6.0, Cap::Butt, j)), false, 30, 30).unwrap();
        // The outer corner pixel lies at (2, 2): inside a miter, outside a bevel.
        assert_eq!(at(&join(Join::Miter), 2, 2), 255);
        assert_eq!(at(&join(Join::Bevel), 2, 2), 0);
        let round = at(&join(Join::Round), 2, 2);
        assert!(round < 255, "a round join cuts the corner: {round}");
        // A sharp turn is beveled when it passes the miter limit.
        let sharp = [Subpath { points: vec![(5.0, 5.0), (25.0, 8.0), (5.0, 11.0)], closed: false }];
        let limited = StrokeStyle { width: 4.0, cap: Cap::Butt, join: Join::Miter, miter_limit: 2.0 };
        let m = rasterize(&stroke_polygons(&sharp, &limited), false, 40, 20).unwrap();
        assert_eq!(at(&m, 28, 8), 0, "the tip past the limit is not drawn");
    }

    #[test]
    fn a_closed_stroke_joins_its_ends_and_a_point_is_a_dot_only_with_a_cap() {
        let closed = stroke_polygons(&[square(5.0, 5.0, 10.0)], &style(2.0, Cap::Butt, Join::Miter));
        let m = rasterize(&closed, false, 30, 30).unwrap();
        assert_eq!(at(&m, 5, 5), 255, "the start corner is joined");
        assert_eq!(at(&m, 10, 10), 0, "the inside is empty");
        let dot = [Subpath { points: vec![(10.0, 10.0)], closed: false }];
        assert!(stroke_polygons(&dot, &style(6.0, Cap::Butt, Join::Miter)).is_empty());
        let round = rasterize(&stroke_polygons(&dot, &style(6.0, Cap::Round, Join::Miter)), false, 20, 20).unwrap();
        assert_eq!(at(&round, 10, 10), 255);
    }

    #[test]
    fn degenerate_input_makes_no_mask() {
        assert!(rasterize(&[], false, 10, 10).is_none());
        assert!(rasterize(&fill_polygons(&[square(-50.0, -50.0, 10.0)]), false, 10, 10).is_none(), "wholly outside");
        assert!(stroke_polygons(&[square(0.0, 0.0, 5.0)], &style(0.0, Cap::Butt, Join::Miter)).is_empty());
        assert!(stroke_polygons(&[square(0.0, 0.0, 5.0)], &style(f64::NAN, Cap::Butt, Join::Miter)).is_empty());
        assert!(rasterize(&[vec![(0.0, 0.0), (f64::NAN, 1.0), (2.0, 2.0)]], false, 10, 10).is_none());
    }

    #[test]
    fn dashing_cuts_a_line_into_the_pieces_the_pattern_asks_for() {
        let line = [Subpath { points: vec![(0.0, 10.0), (20.0, 10.0)], closed: false }];
        let xs = |d: &[Subpath]| d.iter().map(|s| (s.points[0].0, s.points[s.points.len() - 1].0)).collect::<Vec<_>>();
        assert_eq!(xs(&dash_paths(&line, &[4.0, 2.0], 0.0)), vec![(0.0, 4.0), (6.0, 10.0), (12.0, 16.0), (18.0, 20.0)]);
        assert_eq!(xs(&dash_paths(&line, &[4.0, 2.0], 3.0)), vec![(0.0, 1.0), (3.0, 7.0), (9.0, 13.0), (15.0, 19.0)]);
        assert_eq!(xs(&dash_paths(&line, &[5.0], 0.0)), vec![(0.0, 5.0), (10.0, 15.0)], "a one-item pattern is dash and gap alike");
        assert_eq!(dash_paths(&line, &[], 0.0), line.to_vec(), "no pattern is a solid line");
        assert_eq!(dash_paths(&line, &[0.0, 0.0], 0.0), line.to_vec(), "a pattern with no length is a solid line");
    }

    #[test]
    fn a_dash_follows_a_corner_and_a_closed_path_is_dashed_all_the_way_round() {
        let corner = [Subpath { points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], closed: false }];
        let d = dash_paths(&corner, &[15.0, 100.0], 0.0);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].points, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)], "the dash bends round the corner");
        let d = dash_paths(&[square(0.0, 0.0, 10.0)], &[10.0, 10.0], 0.0);
        assert_eq!(d.len(), 2, "a perimeter of 40 holds two 10-long dashes");
        let m = rasterize(&stroke_polygons(&d, &style(2.0, Cap::Butt, Join::Miter)), false, 20, 20).unwrap();
        assert_eq!(at(&m, 5, 0), 255);
        assert_eq!(at(&m, 5, 10), 255, "the third quarter of the perimeter is a dash");
        assert_eq!(at(&m, 10, 5), 0, "and the second a gap");
    }

    #[test]
    fn a_pattern_of_tiny_dashes_is_bounded() {
        let long = [Subpath { points: vec![(0.0, 0.0), (1.0e7, 0.0)], closed: false }];
        assert!(dash_paths(&long, &[0.01, 0.01], 0.0).len() <= 100_000);
        assert!(dash_paths(&long, &[f64::NAN, 1.0], 0.0) == long.to_vec());
    }
}
