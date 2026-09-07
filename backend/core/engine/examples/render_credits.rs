//! Manual-verification tool for the built-in Help/About/Credits page
//! (`phase-4-human-rendering-path/PLAN.md`), mirroring
//! `blueice-raster`'s `examples/render_demo.rs` -- renders it to a PNG
//! so it can be checked by eye the same way the Phase 2 demo page was.

fn main() {
    let mut page = blueice_engine::Page::new(700.0, 900.0);
    page.navigate(blueice_engine::credits::CREDITS_URL).expect("built-in page must not hit the network");
    let pixmap = page.render_visible();
    let path = std::env::args().nth(1).unwrap_or_else(|| "credits.png".to_string());
    pixmap.save_png(std::path::Path::new(&path)).unwrap();
    println!("saved {}x{} to {}", pixmap.width, pixmap.height, path);
}
