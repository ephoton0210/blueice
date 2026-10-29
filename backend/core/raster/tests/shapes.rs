// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_css::{ColorStop, Gradient, GradientKind, LinearDirection, StopPosition};
use blueice_paint::{Color, Frame, PaintCommand, Rect};
use blueice_raster::{rasterize, Pixmap};

fn at(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * p.width + x) * 4) as usize;
    [
        p.pixels[i],
        p.pixels[i + 1],
        p.pixels[i + 2],
        p.pixels[i + 3],
    ]
}

fn frame(w: f64, h: f64, commands: Vec<PaintCommand>) -> Frame {
    Frame {
        width: w,
        height: h,
        commands,
    }
}

const RED: Color = Color::Rgba(255, 0, 0, 255);
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

#[test]
fn a_rounded_rect_leaves_its_corners_unpainted() {
    let p = rasterize(&frame(
        100.0,
        100.0,
        vec![PaintCommand::RoundRect {
            rect: rect(10.0, 10.0, 80.0, 80.0),
            radii: [(30.0, 30.0); 4],
            color: RED,
        }],
    ));
    assert_eq!(at(&p, 50, 50), [255, 0, 0, 255]);
    assert_eq!(at(&p, 11, 11), WHITE, "the corner is cut away");
    assert_eq!(
        at(&p, 50, 11),
        [255, 0, 0, 255],
        "the straight edge is painted"
    );
}

#[test]
fn a_ring_paints_the_border_and_not_the_inside() {
    let p = rasterize(&frame(
        100.0,
        100.0,
        vec![PaintCommand::RoundBorder {
            rect: rect(10.0, 10.0, 80.0, 80.0),
            radii: [(10.0, 10.0); 4],
            width: 4.0,
            color: RED,
        }],
    ));
    assert_eq!(at(&p, 50, 12), [255, 0, 0, 255]);
    assert_eq!(at(&p, 50, 50), WHITE);
}

fn two_stop(direction: LinearDirection) -> Gradient {
    Gradient {
        kind: GradientKind::Linear(direction),
        stops: vec![
            ColorStop {
                color: Color::Rgba(0, 0, 0, 255),
                position: None,
            },
            ColorStop {
                color: Color::Rgba(255, 255, 255, 255),
                position: Some(StopPosition::Percent(100.0)),
            },
        ],
    }
}

#[test]
fn a_linear_gradient_runs_along_its_direction() {
    let p = rasterize(&frame(
        100.0,
        20.0,
        vec![PaintCommand::Gradient {
            rect: rect(0.0, 0.0, 100.0, 20.0),
            radii: [(0.0, 0.0); 4],
            gradient: two_stop(LinearDirection::Angle(90.0)),
        }],
    ));
    let left = at(&p, 2, 10)[0];
    let middle = at(&p, 50, 10)[0];
    let right = at(&p, 97, 10)[0];
    assert!(
        left < 20 && right > 235,
        "dark to light left to right: {left} {right}"
    );
    assert!(
        (100..160).contains(&middle),
        "mid grey in the middle: {middle}"
    );
}

#[test]
fn a_downward_gradient_changes_with_y_only() {
    let p = rasterize(&frame(
        20.0,
        100.0,
        vec![PaintCommand::Gradient {
            rect: rect(0.0, 0.0, 20.0, 100.0),
            radii: [(0.0, 0.0); 4],
            gradient: two_stop(LinearDirection::Angle(180.0)),
        }],
    ));
    assert!(at(&p, 2, 2)[0] < 20);
    assert!(at(&p, 2, 97)[0] > 235);
    assert_eq!(at(&p, 2, 50), at(&p, 17, 50));
}

#[test]
fn a_radial_gradient_is_centred() {
    let p = rasterize(&frame(
        100.0,
        100.0,
        vec![PaintCommand::Gradient {
            rect: rect(0.0, 0.0, 100.0, 100.0),
            radii: [(0.0, 0.0); 4],
            gradient: Gradient {
                kind: GradientKind::Radial {
                    circle: true,
                    x: 0.5,
                    y: 0.5,
                },
                ..two_stop(LinearDirection::Angle(0.0))
            },
        }],
    ));
    assert!(at(&p, 50, 50)[0] < 15);
    assert!(at(&p, 1, 1)[0] > 235);
}

#[test]
fn a_shadow_darkens_around_the_box_but_not_under_it() {
    let p = rasterize(&frame(
        120.0,
        120.0,
        vec![PaintCommand::Shadow {
            rect: rect(40.0, 40.0, 40.0, 40.0),
            radii: [(0.0, 0.0); 4],
            dx: 0.0,
            dy: 0.0,
            blur: 10.0,
            spread: 0.0,
            color: Color::Rgba(0, 0, 0, 255),
        }],
    ));
    let edge = at(&p, 38, 60)[0];
    assert!(edge < 200, "darkest just outside the edge: {edge}");
    assert!(at(&p, 30, 60)[0] > edge, "fades with distance");
    assert_eq!(at(&p, 60, 60), WHITE, "nothing under the box");
    assert_eq!(at(&p, 2, 2), WHITE);
}

#[test]
fn shapes_respect_clips() {
    let p = rasterize(&frame(
        100.0,
        100.0,
        vec![
            PaintCommand::PushClip {
                rect: rect(0.0, 0.0, 50.0, 100.0),
            },
            PaintCommand::RoundRect {
                rect: rect(0.0, 0.0, 100.0, 100.0),
                radii: [(5.0, 5.0); 4],
                color: RED,
            },
            PaintCommand::PopClip,
        ],
    ));
    assert_eq!(at(&p, 25, 50), [255, 0, 0, 255]);
    assert_eq!(at(&p, 75, 50), WHITE);
}

fn red_blue() -> std::sync::Arc<blueice_image::RasterImage> {
    // 2x1: red, blue.
    std::sync::Arc::new(blueice_image::RasterImage::new(
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 255],
    ))
}

#[test]
fn an_image_is_scaled_into_its_rect() {
    let p = rasterize(&frame(
        40.0,
        20.0,
        vec![PaintCommand::Image {
            rect: rect(0.0, 0.0, 40.0, 20.0),
            image: red_blue(),
            alpha: 255,
        }],
    ));
    let left = at(&p, 4, 10);
    let right = at(&p, 36, 10);
    assert!(left[0] > 200 && left[2] < 60, "red on the left: {left:?}");
    assert!(
        right[2] > 200 && right[0] < 60,
        "blue on the right: {right:?}"
    );
}

#[test]
fn image_alpha_blends_and_clips_apply() {
    let p = rasterize(&frame(
        40.0,
        20.0,
        vec![
            PaintCommand::PushClip {
                rect: rect(0.0, 0.0, 20.0, 20.0),
            },
            PaintCommand::Image {
                rect: rect(0.0, 0.0, 40.0, 20.0),
                image: red_blue(),
                alpha: 128,
            },
            PaintCommand::PopClip,
        ],
    ));
    let faded = at(&p, 4, 10);
    assert!(
        faded[0] > 240 && (100..160).contains(&faded[1]),
        "half over white: {faded:?}"
    );
    assert_eq!(at(&p, 30, 10), WHITE, "outside the clip");
}
