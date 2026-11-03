// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A scroll paints and rasterizes only the visible band. It must show exactly
//! what drawing the whole page and taking that band would: nothing that
//! overflows, is positioned or casts a shadow into the window may go missing.

use blueice_engine::Page;

fn tall_page() -> String {
    let mut html = String::from("<body style=\"margin:0\">");
    for i in 0..300 {
        html.push_str(&format!(
            "<div style=\"padding:6px;border:1px solid #888;background:#eef\"><h3>Item {i}</h3><p>Some text <a href=\"/x{i}\">link {i}</a> and more words.</p></div>"
        ));
    }
    // A short box whose content overflows far below it, a card with a large
    // shadow, and an absolutely positioned box well away from its parent.
    html.push_str(
        "<div style=\"height:10px;background:#fcc\"><div style=\"height:900px;width:200px;background:#c33\">overflowing</div></div>\
         <div style=\"margin:40px;height:40px;box-shadow:0 0 120px 40px #000;background:#fff\">card</div>\
         <div style=\"position:relative;height:5px\"><div style=\"position:absolute;top:700px;left:30px;width:90px;height:30px;background:#0a0\">abs</div></div>\
         <div style=\"height:1500px\">tail</div>",
    );
    html
}

#[test]
fn a_scrolled_view_equals_the_same_band_of_the_whole_page() {
    let mut page = Page::new(800.0, 500.0);
    page.load_html_str(&tall_page(), Some("https://example.test/".into()));
    let whole = page.render();
    let total = (whole.height - 500.0).max(0.0);
    assert!(total > 5000.0, "the page is tall: {total}");
    // Positions in the long list, and ones that only see the overflowing box,
    // the shadow's halo and the far-away absolutely positioned box.
    for fraction in [0.0, 0.2, 0.5, 0.8, 1.0] {
        let target = (total * fraction).round();
        page.scroll_by(-1.0e9);
        page.scroll_by(target);
        let band = page.render_visible();
        let expected = blueice_raster::rasterize_viewport(&whole, target, 800, 500, 1.0).unwrap();
        assert_eq!(band.pixels, expected.pixels, "scrolled to {target}");
    }
}

#[test]
fn scrolling_to_where_only_overflow_or_a_shadow_reaches_still_draws_it() {
    let mut page = Page::new(800.0, 500.0);
    page.load_html_str(&tall_page(), Some("https://example.test/".into()));
    let whole = page.render();
    // Find the page offset of the overflowing red box from the display list:
    // the 900px-tall rect is the only one that tall.
    let overflow_top = whole
        .commands
        .iter()
        .find_map(|c| match c {
            blueice_paint::PaintCommand::Rect { rect, .. } if (rect.height - 900.0).abs() < 0.5 => {
                Some(rect.y)
            }
            _ => None,
        })
        .expect("the overflowing box is painted");
    page.scroll_by(overflow_top + 600.0);
    let band = page.render_visible();
    let expected =
        blueice_raster::rasterize_viewport(&whole, page.scroll_y(), 800, 500, 1.0).unwrap();
    assert_eq!(band.pixels, expected.pixels);
    assert!(
        band.pixels.chunks(4).any(|p| p[..3] == [0xcc, 0x33, 0x33]),
        "the overflow below its 10px parent is visible"
    );
}
