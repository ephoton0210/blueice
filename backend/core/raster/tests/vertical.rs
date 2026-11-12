// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text in a vertical line: lying on its side, or upright one character below the other.

use blueice_paint::{Color, Frame, PaintCommand, TextOrient};
use blueice_raster::rasterize;
use std::collections::BTreeSet;

fn run(text: &str, orient: TextOrient, x: f64, y: f64, breadth: f64) -> Frame {
    Frame {
        width: 160.0,
        height: 160.0,
        commands: vec![PaintCommand::Text {
            x,
            y,
            text: text.to_string(),
            color: Color::Rgba(0, 0, 0, 255),
            font_size_px: 24.0,
            bold: false,
            italic: false,
            face: None,
            rtl: false,
            orient,
            breadth,
        }],
    }
}

fn ink(frame: &Frame) -> BTreeSet<(i64, i64)> {
    let pixmap = rasterize(frame);
    let mut out = BTreeSet::new();
    for y in 0..pixmap.height as usize {
        for x in 0..pixmap.width as usize {
            if pixmap.pixels[(y * pixmap.width as usize + x) * 4] < 128 {
                out.insert((x as i64, y as i64));
            }
        }
    }
    out
}

/// The set moved to start at the origin, with its width and height.
fn normalized(set: &BTreeSet<(i64, i64)>) -> (BTreeSet<(i64, i64)>, i64, i64) {
    let (x0, y0) = (set.iter().map(|p| p.0).min().unwrap(), set.iter().map(|p| p.1).min().unwrap());
    let moved: BTreeSet<_> = set.iter().map(|&(x, y)| (x - x0, y - y0)).collect();
    let (w, h) = (moved.iter().map(|p| p.0).max().unwrap() + 1, moved.iter().map(|p| p.1).max().unwrap() + 1);
    (moved, w, h)
}

#[test]
fn sideways_text_is_the_horizontal_text_turned_a_quarter_clockwise() {
    let flat = ink(&run("Lag", TextOrient::Horizontal, 10.0, 10.0, 0.0));
    let turned = ink(&run("Lag", TextOrient::Sideways, 10.0, 10.0, 40.0));
    let (flat, fw, fh) = normalized(&flat);
    let (turned, tw, th) = normalized(&turned);
    assert_eq!((tw, th), (fh, fw), "width and height trade places");
    // a point (x, y) of the flat shape lands at (fh - 1 - y, x) when turned clockwise
    let expected: BTreeSet<_> = flat.iter().map(|&(x, y)| (fh - 1 - y, x)).collect();
    let common = expected.intersection(&turned).count();
    assert!(
        common * 100 >= expected.len() * 93,
        "{common} of {} pixels agree",
        expected.len()
    );
}

#[test]
fn sideways_text_sits_inside_its_box_with_the_tops_of_the_letters_to_the_right() {
    let set = ink(&run("Hi", TextOrient::Sideways, 20.0, 30.0, 40.0));
    assert!(set.iter().all(|&(x, y)| (20..=60).contains(&x) && y >= 30), "inside the column");
    let (min_x, max_x) = (set.iter().map(|p| p.0).min().unwrap(), set.iter().map(|p| p.0).max().unwrap());
    assert!(max_x - min_x >= 12, "the letters are as tall as a capital sideways");
    // the baseline is nearer the left than the right of the column: capitals reach rightward
    assert!(max_x > 20 + 20, "capitals extend toward the right edge");
}

#[test]
fn upright_characters_are_stacked_one_below_the_other_and_centred_in_the_column() {
    let one = ink(&run("\u{65e5}", TextOrient::Upright, 20.0, 20.0, 40.0));
    let two = ink(&run("\u{65e5}\u{672c}", TextOrient::Upright, 20.0, 20.0, 40.0));
    let (one_top, one_bottom) = (one.iter().map(|p| p.1).min().unwrap(), one.iter().map(|p| p.1).max().unwrap());
    let two_bottom = two.iter().map(|p| p.1).max().unwrap();
    assert!(two_bottom > one_bottom + 15, "the second character is below the first");
    assert!(one_top >= 20 && two_bottom <= 20 + 2 * 24 + 2, "within two ems from the top");
    let (left, right) = (one.iter().map(|p| p.0).min().unwrap(), one.iter().map(|p| p.0).max().unwrap());
    let middle = (left + right) as f64 / 2.0;
    assert!((middle - 40.0).abs() <= 3.0, "centred in the 40px column that starts at 20: {middle}");
}

#[test]
fn upright_punctuation_takes_its_vertical_form() {
    // an opening corner bracket is drawn as its vertical presentation form
    let drawn = ink(&run("\u{300c}", TextOrient::Upright, 20.0, 20.0, 40.0));
    let vertical_form = ink(&run("\u{fe41}", TextOrient::Upright, 20.0, 20.0, 40.0));
    let plain = ink(&run("\u{300c}", TextOrient::Horizontal, 20.0, 20.0, 40.0));
    assert_eq!(drawn, vertical_form);
    assert_ne!(drawn, plain);
}

#[test]
fn counter_clockwise_text_is_the_horizontal_text_turned_a_quarter_the_other_way() {
    let flat = ink(&run("Lag", TextOrient::Horizontal, 10.0, 10.0, 0.0));
    // text that runs up starts at the bottom: y is the bottom of its box
    let turned = ink(&run("Lag", TextOrient::SidewaysCcw, 10.0, 110.0, 40.0));
    let (flat, fw, fh) = normalized(&flat);
    let (turned, tw, th) = normalized(&turned);
    assert_eq!((tw, th), (fh, fw), "width and height trade places");
    // a point (x, y) of the flat shape lands at (y, fw - 1 - x) when turned counter-clockwise
    let expected: BTreeSet<_> = flat.iter().map(|&(x, y)| (y, fw - 1 - x)).collect();
    let common = expected.intersection(&turned).count();
    assert!(common * 100 >= expected.len() * 93, "{common} of {} pixels agree", expected.len());
}

#[test]
fn counter_clockwise_text_climbs_from_its_start_and_its_letters_face_left() {
    let set = ink(&run("Hi", TextOrient::SidewaysCcw, 20.0, 120.0, 40.0));
    assert!(set.iter().all(|&(x, y)| (20..=60).contains(&x) && y <= 120), "inside the column, above the start");
    let (min_x, max_x) = (set.iter().map(|p| p.0).min().unwrap(), set.iter().map(|p| p.0).max().unwrap());
    assert!(min_x < 20 + 12, "capitals reach toward the left edge: {min_x}");
    assert!(max_x - min_x >= 12);
    let (min_y, max_y) = (set.iter().map(|p| p.1).min().unwrap(), set.iter().map(|p| p.1).max().unwrap());
    assert!(max_y > 120 - 6, "the H starts right at the start point");
    assert!(min_y < 120 - 20, "and the text climbs");
}

#[test]
fn combined_characters_are_squeezed_into_one_em_and_centred_in_the_column() {
    let wide = ink(&run("2026", TextOrient::Horizontal, 10.0, 10.0, 0.0));
    let (_, wide_w, _) = normalized(&wide);
    assert!(wide_w > 40, "four digits are wider than an em on a line: {wide_w}");
    let set = ink(&run("2026", TextOrient::Combined, 20.0, 20.0, 40.0));
    let (left, right) = (set.iter().map(|p| p.0).min().unwrap(), set.iter().map(|p| p.0).max().unwrap());
    let (top, bottom) = (set.iter().map(|p| p.1).min().unwrap(), set.iter().map(|p| p.1).max().unwrap());
    assert!(right - left + 1 <= 24, "within one em (24px) across: {}", right - left + 1);
    assert!(top >= 20 && bottom <= 20 + 24, "within the one-em cell down the page: {top}..{bottom}");
    let middle = (left + right) as f64 / 2.0;
    assert!((middle - 40.0).abs() <= 3.0, "centred in the 40px column that starts at 20: {middle}");
    // a run that already fits is not shrunk: one digit is narrower than an em
    let one = ink(&run("5", TextOrient::Combined, 20.0, 20.0, 40.0));
    let flat = ink(&run("5", TextOrient::Horizontal, 10.0, 10.0, 0.0));
    assert_eq!(normalized(&one).1, normalized(&flat).1);
}
