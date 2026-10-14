// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_paint::{Color, Frame, PaintCommand, Rect};
use blueice_raster::rasterize;

#[test]
fn nested_native_control_clips_intersect_and_restore_for_later_content() {
    let frame = Frame {
        width: 20.0,
        height: 20.0,
        commands: vec![
            PaintCommand::PushClip {
                rect: Rect {
                    x: 2.0,
                    y: 2.0,
                    width: 10.0,
                    height: 10.0,
                },
            },
            PaintCommand::PushClip {
                rect: Rect {
                    x: 5.0,
                    y: 5.0,
                    width: 10.0,
                    height: 10.0,
                },
            },
            PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                color: Color::Rgba(255, 0, 0, 255),
            },
            PaintCommand::PopClip,
            PaintCommand::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 3.0,
                    width: 1.0,
                    height: 1.0,
                },
                color: Color::Rgba(0, 255, 0, 255),
            },
            PaintCommand::PopClip,
            PaintCommand::Rect {
                rect: Rect {
                    x: 16.0,
                    y: 16.0,
                    width: 1.0,
                    height: 1.0,
                },
                color: Color::Rgba(0, 0, 255, 255),
            },
        ],
    };
    let image = rasterize(&frame);
    assert_eq!(image.get_pixel(6, 6), [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(13, 13), [255, 255, 255, 255]);
    assert_eq!(image.get_pixel(3, 3), [0, 255, 0, 255]);
    assert_eq!(image.get_pixel(16, 16), [0, 0, 255, 255]);
}

#[test]
fn clipping_applies_to_text_glyphs_and_empty_intersections() {
    let frame = Frame {
        width: 40.0,
        height: 20.0,
        commands: vec![
            PaintCommand::PushClip {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 8.0,
                    height: 20.0,
                },
            },
            PaintCommand::Text {
                x: 0.0,
                y: 0.0,
                text: "MMMM".into(),
                color: Color::Rgba(0, 0, 0, 255),
                font_size_px: 16.0,
                bold: false,
                italic: false,
            },
            PaintCommand::PushClip {
                rect: Rect {
                    x: 30.0,
                    y: 0.0,
                    width: 8.0,
                    height: 20.0,
                },
            },
            PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 40.0,
                    height: 20.0,
                },
                color: Color::Rgba(255, 0, 0, 255),
            },
            PaintCommand::PopClip,
            PaintCommand::PopClip,
        ],
    };
    let image = rasterize(&frame);
    assert!((0..20).any(|y| (0..8).any(|x| image.get_pixel(x, y)[0] < 255)));
    assert!((0..20).all(|y| (8..40).all(|x| image.get_pixel(x, y) == [255, 255, 255, 255])));
}
