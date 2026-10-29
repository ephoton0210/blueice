// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Anti-aliased shapes (rounded rectangles and rings, gradients, blurred
//! shadows) drawn with `tiny-skia` straight into the frame buffer, which is
//! always opaque, so tiny-skia's premultiplied pixels and the buffer's plain
//! RGBA coincide.

use blueice_css::{Gradient, GradientKind, LinearDirection, StopPosition};
use blueice_paint::{Color, Radii, Rect};
use tiny_skia as sk;

/// An integer pixel clip, `[left, right) x [top, bottom)`.
#[derive(Clone, Copy)]
pub(crate) struct Clip {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

const KAPPA: f32 = 0.552_284_8;

fn rect_ok(r: Rect) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()) && r.width > 0.0 && r.height > 0.0
}

/// A rounded rectangle with elliptical corners.
pub(crate) fn rrect_path(rect: Rect, radii: &Radii) -> Option<sk::Path> {
    if !rect_ok(rect) {
        return None;
    }
    let (x, y) = (rect.x as f32, rect.y as f32);
    let (w, h) = (rect.width as f32, rect.height as f32);
    let r = |i: usize| (radii[i].0 as f32, radii[i].1 as f32);
    let (tl, tr, br, bl) = (r(0), r(1), r(2), r(3));
    let mut pb = sk::PathBuilder::new();
    pb.move_to(x + tl.0, y);
    pb.line_to(x + w - tr.0, y);
    pb.cubic_to(
        x + w - tr.0 + tr.0 * KAPPA,
        y,
        x + w,
        y + tr.1 - tr.1 * KAPPA,
        x + w,
        y + tr.1,
    );
    pb.line_to(x + w, y + h - br.1);
    pb.cubic_to(
        x + w,
        y + h - br.1 + br.1 * KAPPA,
        x + w - br.0 + br.0 * KAPPA,
        y + h,
        x + w - br.0,
        y + h,
    );
    pb.line_to(x + bl.0, y + h);
    pb.cubic_to(
        x + bl.0 - bl.0 * KAPPA,
        y + h,
        x,
        y + h - bl.1 + bl.1 * KAPPA,
        x,
        y + h - bl.1,
    );
    pb.line_to(x, y + tl.1);
    pb.cubic_to(
        x,
        y + tl.1 - tl.1 * KAPPA,
        x + tl.0 - tl.0 * KAPPA,
        y,
        x + tl.0,
        y,
    );
    pb.close();
    pb.finish()
}

fn color(c: Color) -> Option<sk::Color> {
    match c {
        Color::Rgba(r, g, b, a) => Some(sk::Color::from_rgba8(r, g, b, a)),
        Color::CurrentColor => None,
    }
}

fn mask_for(width: u32, height: u32, clip: Clip) -> Option<sk::Mask> {
    if clip.left <= 0
        && clip.top <= 0
        && clip.right >= i64::from(width)
        && clip.bottom >= i64::from(height)
    {
        return None;
    }
    let mut mask = sk::Mask::new(width, height)?;
    let rect = sk::Rect::from_ltrb(
        clip.left as f32,
        clip.top as f32,
        clip.right.max(clip.left) as f32,
        clip.bottom.max(clip.top) as f32,
    )?;
    let path = sk::PathBuilder::from_rect(rect);
    mask.fill_path(
        &path,
        sk::FillRule::Winding,
        false,
        sk::Transform::identity(),
    );
    Some(mask)
}

pub(crate) struct Canvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
}

impl Canvas<'_> {
    fn with(&mut self, clip: Clip, draw: impl FnOnce(&mut sk::PixmapMut<'_>, Option<&sk::Mask>)) {
        let mask = match mask_for(self.width, self.height, clip) {
            Some(m) => Some(m),
            None if clip.right <= clip.left || clip.bottom <= clip.top => return,
            None => None,
        };
        if let Some(mut pm) = sk::PixmapMut::from_bytes(self.pixels, self.width, self.height) {
            draw(&mut pm, mask.as_ref());
        }
    }

    pub fn fill_rrect(&mut self, rect: Rect, radii: &Radii, c: Color, clip: Clip) {
        let (Some(path), Some(col)) = (rrect_path(rect, radii), color(c)) else {
            return;
        };
        let mut paint = sk::Paint::default();
        paint.set_color(col);
        paint.anti_alias = true;
        self.with(clip, |pm, mask| {
            pm.fill_path(
                &path,
                &paint,
                sk::FillRule::Winding,
                sk::Transform::identity(),
                mask,
            )
        });
    }

    pub fn border_ring(&mut self, rect: Rect, radii: &Radii, width: f64, c: Color, clip: Clip) {
        let (Some(outer), Some(col)) = (rrect_path(rect, radii), color(c)) else {
            return;
        };
        let inner_rect = Rect {
            x: rect.x + width,
            y: rect.y + width,
            width: rect.width - 2.0 * width,
            height: rect.height - 2.0 * width,
        };
        let inner_radii: Radii =
            std::array::from_fn(|i| ((radii[i].0 - width).max(0.0), (radii[i].1 - width).max(0.0)));
        let mut pb = sk::PathBuilder::new();
        pb.push_path(&outer);
        if let Some(inner) = rrect_path(inner_rect, &inner_radii) {
            pb.push_path(&inner);
        }
        let Some(path) = pb.finish() else { return };
        let mut paint = sk::Paint::default();
        paint.set_color(col);
        paint.anti_alias = true;
        self.with(clip, |pm, mask| {
            pm.fill_path(
                &path,
                &paint,
                sk::FillRule::EvenOdd,
                sk::Transform::identity(),
                mask,
            )
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn shadow(
        &mut self,
        rect: Rect,
        radii: &Radii,
        dx: f64,
        dy: f64,
        blur: f64,
        spread: f64,
        c: Color,
        clip: Clip,
    ) {
        let Some(col) = color(c) else { return };
        if !rect_ok(rect) {
            return;
        }
        let sigma = (blur / 2.0).max(0.0);
        let margin = (sigma * 3.0).ceil() + 2.0;
        let shape = Rect {
            x: rect.x + dx - spread,
            y: rect.y + dy - spread,
            width: (rect.width + 2.0 * spread).max(0.0),
            height: (rect.height + 2.0 * spread).max(0.0),
        };
        let grown: Radii = std::array::from_fn(|i| {
            (
                (radii[i].0 + spread).max(0.0),
                (radii[i].1 + spread).max(0.0),
            )
        });
        // The scratch area: the shadow's extent, limited to the clipped canvas.
        let left = ((shape.x - margin).floor() as i64).max(clip.left.max(0));
        let top = ((shape.y - margin).floor() as i64).max(clip.top.max(0));
        let right = (((shape.x + shape.width + margin).ceil()) as i64)
            .min(clip.right.min(i64::from(self.width)));
        let bottom = (((shape.y + shape.height + margin).ceil()) as i64)
            .min(clip.bottom.min(i64::from(self.height)));
        if right <= left || bottom <= top {
            return;
        }
        let (sw, sh) = ((right - left) as u32, (bottom - top) as u32);
        let Some(mut scratch) = sk::Pixmap::new(sw, sh) else {
            return;
        };
        let local = |r: Rect| Rect {
            x: r.x - left as f64,
            y: r.y - top as f64,
            ..r
        };
        let mut paint = sk::Paint::default();
        paint.set_color(col);
        paint.anti_alias = true;
        if let Some(path) = rrect_path(local(shape), &grown) {
            scratch.fill_path(
                &path,
                &paint,
                sk::FillRule::Winding,
                sk::Transform::identity(),
                None,
            );
        }
        if sigma > 0.3 {
            box_blur(&mut scratch, sigma);
        }
        // The shadow is never visible beneath the box itself.
        if let Some(path) = rrect_path(local(rect), radii) {
            let clear = sk::Paint {
                blend_mode: sk::BlendMode::Clear,
                anti_alias: true,
                ..sk::Paint::default()
            };
            scratch.fill_path(
                &path,
                &clear,
                sk::FillRule::Winding,
                sk::Transform::identity(),
                None,
            );
        }
        self.with(clip, |pm, mask| {
            pm.draw_pixmap(
                left as i32,
                top as i32,
                scratch.as_ref(),
                &sk::PixmapPaint::default(),
                sk::Transform::identity(),
                mask,
            )
        });
    }

    /// `image` scaled into `rect` at `alpha` opacity, smoothly resampled.
    pub fn image(&mut self, rect: Rect, image: &blueice_image::RasterImage, alpha: u8, clip: Clip) {
        if !rect_ok(rect) || image.width == 0 || image.height == 0 {
            return;
        }
        let Some(source) =
            sk::PixmapRef::from_bytes(image.premultiplied(), image.width, image.height)
        else {
            return;
        };
        let (sx, sy) = (
            rect.width as f32 / image.width as f32,
            rect.height as f32 / image.height as f32,
        );
        let quality = if sx < 1.0 || sy < 1.0 {
            sk::FilterQuality::Bilinear
        } else {
            sk::FilterQuality::Bicubic
        };
        let pattern = sk::Pattern::new(
            source,
            sk::SpreadMode::Pad,
            quality,
            f32::from(alpha) / 255.0,
            sk::Transform::from_row(sx, 0.0, 0.0, sy, rect.x as f32, rect.y as f32),
        );
        let paint = sk::Paint {
            shader: pattern,
            anti_alias: false,
            ..sk::Paint::default()
        };
        let Some(dest) = sk::Rect::from_xywh(
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
        ) else {
            return;
        };
        self.with(clip, |pm, mask| {
            pm.fill_rect(dest, &paint, sk::Transform::identity(), mask)
        });
    }

    pub fn gradient(&mut self, rect: Rect, radii: &Radii, gradient: &Gradient, clip: Clip) {
        if !rect_ok(rect) {
            return;
        }
        let Some(path) = rrect_path(rect, radii) else {
            return;
        };
        let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let (shader, line_length) = match &gradient.kind {
            GradientKind::Linear(direction) => {
                let angle = match direction {
                    LinearDirection::Angle(a) => a.to_radians(),
                    LinearDirection::Corner { right, bottom } => {
                        // The 50% line runs through the other two corners.
                        let (w, h) = (rect.width, rect.height);
                        let dir_x = if *right { 1.0 } else { -1.0 };
                        let dir_y = if *bottom { 1.0 } else { -1.0 };
                        // Perpendicular to the diagonal joining the other corners.
                        (dir_x * h).atan2(-dir_y * w)
                    }
                };
                let (dx, dy) = (angle.sin(), -angle.cos());
                let length = (rect.width * dx).abs() + (rect.height * dy).abs();
                let start = sk::Point::from_xy(
                    (cx - dx * length / 2.0) as f32,
                    (cy - dy * length / 2.0) as f32,
                );
                let end = sk::Point::from_xy(
                    (cx + dx * length / 2.0) as f32,
                    (cy + dy * length / 2.0) as f32,
                );
                let Some(stops) = stops(gradient, length) else {
                    return;
                };
                (
                    sk::LinearGradient::new(
                        start,
                        end,
                        stops,
                        sk::SpreadMode::Pad,
                        sk::Transform::identity(),
                    ),
                    length,
                )
            }
            GradientKind::Radial { circle, x, y } => {
                let (ox, oy) = (rect.x + rect.width * x, rect.y + rect.height * y);
                let far_x = (ox - rect.x).max(rect.x + rect.width - ox);
                let far_y = (oy - rect.y).max(rect.y + rect.height - oy);
                let (rx, ry) = if *circle {
                    let r = far_x.hypot(far_y);
                    (r, r)
                } else {
                    (
                        far_x * std::f64::consts::SQRT_2,
                        far_y * std::f64::consts::SQRT_2,
                    )
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return;
                }
                let Some(stops) = stops(gradient, rx) else {
                    return;
                };
                let squash = (ry / rx) as f32;
                let centre = sk::Point::from_xy(ox as f32, oy as f32);
                (
                    sk::RadialGradient::new(
                        centre,
                        centre,
                        rx as f32,
                        stops,
                        sk::SpreadMode::Pad,
                        sk::Transform::from_row(
                            1.0,
                            0.0,
                            0.0,
                            squash,
                            0.0,
                            oy as f32 * (1.0 - squash),
                        ),
                    ),
                    rx,
                )
            }
        };
        let _ = line_length;
        let Some(shader) = shader else { return };
        let paint = sk::Paint {
            shader,
            anti_alias: true,
            ..sk::Paint::default()
        };
        self.with(clip, |pm, mask| {
            pm.fill_path(
                &path,
                &paint,
                sk::FillRule::Winding,
                sk::Transform::identity(),
                mask,
            )
        });
    }
}

/// Stops with every position resolved to `0..=1` along a line of `length` px.
fn stops(gradient: &Gradient, length: f64) -> Option<Vec<sk::GradientStop>> {
    let n = gradient.stops.len();
    let mut at: Vec<Option<f64>> = gradient
        .stops
        .iter()
        .map(|s| match &s.position {
            Some(StopPosition::Percent(p)) => Some(p / 100.0),
            Some(StopPosition::Px(px)) if length > 0.0 => Some(px / length),
            _ => None,
        })
        .collect();
    if at[0].is_none() {
        at[0] = Some(0.0);
    }
    if at[n - 1].is_none() {
        at[n - 1] = Some(1.0);
    }
    // Never decreasing.
    let mut high = f64::NEG_INFINITY;
    for p in at.iter_mut().flatten() {
        high = high.max(*p);
        *p = high;
    }
    // Evenly spread each run of unpositioned stops between its neighbours.
    let mut i = 0;
    while i < n {
        if at[i].is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while at[end].is_none() {
            end += 1;
        }
        let (a, b) = (at[start]?, at[end]?);
        for (k, slot) in at.iter_mut().enumerate().take(end).skip(start + 1) {
            *slot = Some(a + (b - a) * (k - start) as f64 / (end - start) as f64);
        }
        i = end;
    }
    gradient
        .stops
        .iter()
        .zip(at)
        .map(|(s, p)| {
            Some(sk::GradientStop::new(
                p?.clamp(0.0, 1.0) as f32,
                color(s.color)?,
            ))
        })
        .collect()
}

/// Three box-blur passes per axis approximate a Gaussian of `sigma`.
fn box_blur(pixmap: &mut sk::Pixmap, sigma: f64) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    // Radius of each of three passes (Kovesi's recipe).
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut wl = ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - 3.0 * (wl * wl) as f64 - 12.0 * wl as f64 - 9.0)
        / (-4.0 * wl as f64 - 4.0))
        .round() as i64;
    let sizes = [0, 1, 2].map(|i| if i < m { wl } else { wu });
    let data = pixmap.data_mut();
    let mut scratch = vec![0u8; data.len()];
    for size in sizes {
        let radius = ((size - 1) / 2).max(0) as usize;
        if radius == 0 {
            continue;
        }
        blur_axis(data, &mut scratch, w, h, radius, true);
        blur_axis(&scratch, data, w, h, radius, false);
    }
}

fn blur_axis(src: &[u8], dst: &mut [u8], w: usize, h: usize, radius: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let stride = |line: usize, i: usize| {
        if horizontal {
            (line * w + i) * 4
        } else {
            (i * w + line) * 4
        }
    };
    let window = (2 * radius + 1) as u32;
    for line in 0..lines {
        for channel in 0..4 {
            let mut sum: u32 = 0;
            for i in 0..=radius.min(len - 1) {
                sum += u32::from(src[stride(line, i) + channel]);
            }
            for i in 0..len {
                dst[stride(line, i) + channel] = (sum / window) as u8;
                let add = i + radius + 1;
                if add < len {
                    sum += u32::from(src[stride(line, add) + channel]);
                }
                if i >= radius {
                    sum -= u32::from(src[stride(line, i - radius) + channel]);
                }
            }
        }
    }
}
