// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rounded clipping without touching any drawing operation.
//!
//! Whatever is drawn while a rounded clip is open is already confined to its rectangle. Only the
//! four corner squares can contain pixels outside the rounded shape, so those are copied when
//! the clip opens; when it closes, every pixel of them that is outside the shape goes back to
//! what it was, and one the edge cuts through is blended back by how much of it is outside.

use crate::Pixmap;
use blueice_paint::{Radii, Rect};

/// A pixel is sampled this many times along each axis to measure how much of it is inside.
const SAMPLES: u32 = 4;

struct Corner {
    /// The square of pixels, by its top-left pixel and size.
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    /// The ellipse's centre and radii, in pixels.
    centre: (f64, f64),
    radius: (f64, f64),
    /// The pixels as they were: RGBA, row-major.
    saved: Vec<u8>,
}

pub(crate) struct Corners(Vec<Corner>);

impl Corners {
    /// The corner squares of `rect` as they are now; `None` when nothing is rounded.
    pub(crate) fn save(pixmap: &Pixmap, rect: Rect, radii: &Radii) -> Option<Corners> {
        if !(rect.width > 0.0 && rect.height > 0.0) {
            return None;
        }
        // Neighbouring radii on one side may not add up to more than that side.
        let fit = |a: (f64, f64), b: (f64, f64), horizontal: bool| {
            let (sum, side) = if horizontal {
                (a.0 + b.0, rect.width)
            } else {
                (a.1 + b.1, rect.height)
            };
            if sum > side && sum > 0.0 {
                side / sum
            } else {
                1.0
            }
        };
        let [tl, tr, br, bl] = *radii;
        let factor = fit(tl, tr, true)
            .min(fit(bl, br, true))
            .min(fit(tl, bl, false))
            .min(fit(tr, br, false));
        let scaled = |r: (f64, f64)| (r.0.max(0.0) * factor, r.1.max(0.0) * factor);
        let (tl, tr, br, bl) = (scaled(tl), scaled(tr), scaled(br), scaled(bl));
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        // (square's top-left in page coordinates, ellipse centre, radii)
        let corners = [
            ((rect.x, rect.y), (rect.x + tl.0, rect.y + tl.1), tl),
            ((right - tr.0, rect.y), (right - tr.0, rect.y + tr.1), tr),
            ((right - br.0, bottom - br.1), (right - br.0, bottom - br.1), br),
            ((rect.x, bottom - bl.1), (rect.x + bl.0, bottom - bl.1), bl),
        ];
        let mut saved = Vec::new();
        for ((left, top), centre, radius) in corners {
            if radius.0 <= 0.0 || radius.1 <= 0.0 {
                continue;
            }
            let x0 = left.floor() as i64;
            let y0 = top.floor() as i64;
            let x1 = (left + radius.0).ceil() as i64;
            let y1 = (top + radius.1).ceil() as i64;
            let (x0, y0) = (x0.max(0), y0.max(0));
            let (x1, y1) = (x1.min(i64::from(pixmap.width)), y1.min(i64::from(pixmap.height)));
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let mut pixels = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
            for y in y0..y1 {
                let start = ((y * i64::from(pixmap.width) + x0) * 4) as usize;
                pixels.extend_from_slice(&pixmap.pixels[start..start + ((x1 - x0) * 4) as usize]);
            }
            saved.push(Corner {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
                centre,
                radius,
                saved: pixels,
            });
        }
        (!saved.is_empty()).then_some(Corners(saved))
    }

    /// Puts back every pixel (or share of one) of the corner squares that is outside the
    /// rounded shape.
    pub(crate) fn restore_outside_the_arcs(&self, pixmap: &mut Pixmap) {
        for corner in &self.0 {
            for row in 0..corner.height {
                for column in 0..corner.width {
                    let inside = corner.coverage(corner.x + column, corner.y + row);
                    if inside >= 1.0 {
                        continue;
                    }
                    let at = (((corner.y + row) * i64::from(pixmap.width) + corner.x + column) * 4) as usize;
                    let from = ((row * corner.width + column) * 4) as usize;
                    for channel in 0..4 {
                        let before = f64::from(corner.saved[from + channel]);
                        let now = f64::from(pixmap.pixels[at + channel]);
                        pixmap.pixels[at + channel] = (before + (now - before) * inside).round() as u8;
                    }
                }
            }
        }
    }
}

impl Corner {
    /// The share of pixel (`px`, `py`) that lies inside the rounded shape. Only the quarter of the
    /// ellipse in this corner's square matters: a sample on the box side of the centre is inside.
    fn coverage(&self, px: i64, py: i64) -> f64 {
        let mut inside = 0;
        for j in 0..SAMPLES {
            for i in 0..SAMPLES {
                let sx = px as f64 + (f64::from(i) + 0.5) / f64::from(SAMPLES);
                let sy = py as f64 + (f64::from(j) + 0.5) / f64::from(SAMPLES);
                let dx = sx - self.centre.0;
                let dy = sy - self.centre.1;
                // The corner is on the outer side of the centre on both axes; on the inner side
                // of either axis the box is straight.
                let beyond_x = (dx < 0.0) == (self.x as f64 + self.width as f64 / 2.0 < self.centre.0);
                let beyond_y = (dy < 0.0) == (self.y as f64 + self.height as f64 / 2.0 < self.centre.1);
                let out = beyond_x
                    && beyond_y
                    && (dx / self.radius.0).powi(2) + (dy / self.radius.1).powi(2) > 1.0;
                if !out {
                    inside += 1;
                }
            }
        }
        f64::from(inside) / f64::from(SAMPLES * SAMPLES)
    }
}
