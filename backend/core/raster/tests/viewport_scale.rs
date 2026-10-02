// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_paint::{Color, Frame, PaintCommand, Rect};
use blueice_raster::rasterize_viewport;

#[test]
fn viewport_scales_coordinates_scroll_and_nested_clips_before_rasterizing() {
    let frame = Frame {
        width: 100.0,
        height: 100000.0,
        commands: vec![
            PaintCommand::PushClip {
                rect: Rect {
                    x: 2.0,
                    y: 10.0,
                    width: 3.0,
                    height: 4.0,
                },
            },
            PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100000.0,
                },
                color: Color::Rgba(30, 90, 150, 255),
            },
            PaintCommand::PopClip,
        ],
    };
    let image = rasterize_viewport(&frame, 10.0, 20, 16, 2.0).unwrap();
    assert_eq!((image.width, image.height), (20, 16));
    assert_eq!(image.pixels.len(), 20 * 16 * 4);
    assert_eq!(image.get_pixel(4, 0), [30, 90, 150, 255]);
    assert_eq!(image.get_pixel(9, 7), [30, 90, 150, 255]);
    for (x, y) in [(3, 0), (10, 0), (4, 8)] {
        assert_eq!(image.get_pixel(x, y), [255; 4]);
    }
}

#[test]
fn high_density_text_is_newly_rasterized_instead_of_enlarging_low_density_pixels() {
    let frame = Frame {
        width: 100.0,
        height: 40.0,
        commands: vec![PaintCommand::Text {
            x: 4.0,
            y: 4.0,
            text: "Ice".into(),
            color: Color::Rgba(0, 0, 0, 255),
            font_size_px: 16.0,
            bold: false,
            italic: false,
        }],
    };
    let low = rasterize_viewport(&frame, 0.0, 100, 40, 1.0).unwrap();
    let high = rasterize_viewport(&frame, 0.0, 200, 80, 2.0).unwrap();
    assert!(high.pixels.chunks_exact(4).any(|p| p[0] > 0 && p[0] < 255));
    assert!(
        (0..80).any(|y| (0..200).any(|x| high.get_pixel(x, y) != low.get_pixel(x / 2, y / 2))),
        "2x fonts must not be a nearest-neighbor image enlargement"
    );
}

#[test]
fn viewport_rejects_nonfinite_or_excessive_allocation_before_drawing() {
    let frame = Frame {
        width: 1.0,
        height: 1.0,
        commands: vec![],
    };
    for (scroll, width, height, scale) in [
        (0.0, 0, 10, 1.0),
        (0.0, 10, 10, 0.0),
        (0.0, 4097, 10, 1.0),
        (0.0, 10, 4097, 2.0),
        (f64::NAN, 10, 10, 1.0),
        (0.0, 10, 10, f64::INFINITY),
    ] {
        assert!(rasterize_viewport(&frame, scroll, width, height, scale).is_err());
    }
}
