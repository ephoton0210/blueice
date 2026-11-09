// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_paint::{Color, Frame, PaintCommand, Rect};
use blueice_raster::rasterize;

const RED: Color = Color::Rgba(255, 0, 0, 255);
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn frame(commands: Vec<PaintCommand>) -> Frame {
    Frame {
        width: 40.0,
        height: 40.0,
        commands,
    }
}

#[test]
fn content_is_cut_off_outside_the_rounded_corners_and_kept_inside_them() {
    let pixmap = rasterize(&frame(vec![
        PaintCommand::PushRoundClip {
            rect: rect(10.0, 10.0, 20.0, 20.0),
            radii: [(8.0, 8.0); 4],
        },
        PaintCommand::Rect {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            color: RED,
        },
        PaintCommand::PopClip,
    ]));
    // The very corner of the box is outside the arc; the middle and the straight edges are in.
    assert_eq!(pixmap.get_pixel(10, 10), WHITE);
    assert_eq!(pixmap.get_pixel(29, 10), WHITE);
    assert_eq!(pixmap.get_pixel(10, 29), WHITE);
    assert_eq!(pixmap.get_pixel(29, 29), WHITE);
    assert_eq!(pixmap.get_pixel(20, 20), [255, 0, 0, 255]);
    assert_eq!(pixmap.get_pixel(20, 10), [255, 0, 0, 255]);
    assert_eq!(pixmap.get_pixel(10, 20), [255, 0, 0, 255]);
    // Inside the arc at 45 degrees (distance 7.07 of 8 from the corner circle's centre).
    assert_eq!(pixmap.get_pixel(13, 13), [255, 0, 0, 255]);
    // Drawing never reaches past the clip's rectangle.
    assert_eq!(pixmap.get_pixel(5, 5), WHITE);
    assert_eq!(pixmap.get_pixel(35, 20), WHITE);
}

#[test]
fn the_edge_of_the_arc_is_antialiased() {
    let pixmap = rasterize(&frame(vec![
        PaintCommand::PushRoundClip {
            rect: rect(10.0, 10.0, 20.0, 20.0),
            radii: [(8.0, 8.0); 4],
        },
        PaintCommand::Rect {
            rect: rect(10.0, 10.0, 20.0, 20.0),
            color: RED,
        },
        PaintCommand::PopClip,
    ]));
    // A pixel straddling the arc is a blend: neither white nor pure red.
    let on_arc = (0..8)
        .map(|i| pixmap.get_pixel(10 + i, 10 + i))
        .find(|p| *p != WHITE && *p != [255, 0, 0, 255]);
    let edge = on_arc.expect("a partially covered pixel on the diagonal");
    assert!(edge[1] > 0 && edge[1] < 255 && edge[0] == 255, "{edge:?}");
}

#[test]
fn rounded_clips_nest_with_plain_clips_and_restore_in_order() {
    let pixmap = rasterize(&frame(vec![
        PaintCommand::PushRoundClip {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            radii: [(10.0, 10.0); 4],
        },
        PaintCommand::PushClip {
            rect: rect(0.0, 0.0, 20.0, 40.0),
        },
        PaintCommand::PushRoundClip {
            rect: rect(0.0, 0.0, 20.0, 20.0),
            radii: [(10.0, 10.0); 4],
        },
        PaintCommand::Rect {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            color: RED,
        },
        PaintCommand::PopClip,
        PaintCommand::PopClip,
        PaintCommand::Rect {
            rect: rect(20.0, 30.0, 10.0, 10.0),
            color: Color::Rgba(0, 0, 255, 255),
        },
        PaintCommand::PopClip,
    ]));
    assert_eq!(pixmap.get_pixel(0, 0), WHITE, "outside both arcs");
    assert_eq!(pixmap.get_pixel(10, 10), [255, 0, 0, 255]);
    assert_eq!(pixmap.get_pixel(30, 10), WHITE, "outside the plain clip");
    assert_eq!(pixmap.get_pixel(10, 30), WHITE, "outside the inner rounded clip");
    // After the inner clips pop, the outer rounded clip still applies to later drawing.
    assert_eq!(pixmap.get_pixel(25, 35), [0, 0, 255, 255]);
    assert_eq!(pixmap.get_pixel(39, 39), WHITE);
}

#[test]
fn a_box_with_no_radius_or_a_zero_size_changes_nothing() {
    let pixmap = rasterize(&frame(vec![
        PaintCommand::PushRoundClip {
            rect: rect(10.0, 10.0, 20.0, 20.0),
            radii: [(0.0, 0.0); 4],
        },
        PaintCommand::Rect {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            color: RED,
        },
        PaintCommand::PopClip,
        PaintCommand::PushRoundClip {
            rect: rect(0.0, 0.0, 0.0, 0.0),
            radii: [(5.0, 5.0); 4],
        },
        PaintCommand::PopClip,
    ]));
    assert_eq!(pixmap.get_pixel(10, 10), [255, 0, 0, 255]);
}
