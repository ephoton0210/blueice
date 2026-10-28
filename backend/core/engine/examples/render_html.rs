// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Renders an HTML file to a PNG with the same `render` + `rasterize` path the
//! engine tests use, for comparing a saved page against a browser by eye:
//! `cargo run -p blueice-engine --example render_html -- page.html out.png [width] [sheet.css...]`
//! (width defaults to 1280). Any further arguments are stylesheet files applied
//! before the page's own `<style>` tags; linked stylesheets are not fetched.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: render_html <page.html> <out.png> [viewport-width]");
        std::process::exit(2);
    };
    let html = std::fs::read_to_string(input).expect("readable HTML file");
    let width: f64 = args
        .get(3)
        .map_or(1280.0, |w| w.parse().expect("numeric viewport width"));
    let css: String = args
        .iter()
        .skip(4)
        .map(|path| std::fs::read_to_string(path).expect("readable stylesheet"))
        .collect::<Vec<_>>()
        .join("\n");
    let started = std::time::Instant::now();
    let frame = blueice_engine::render(&html, &css, width);
    eprintln!("laid out and painted in {:?}", started.elapsed());
    let pixmap = blueice_raster::rasterize(&frame);
    eprintln!("{}x{}", pixmap.width, pixmap.height);
    pixmap
        .save_png(std::path::Path::new(output))
        .expect("writable PNG path");
}
