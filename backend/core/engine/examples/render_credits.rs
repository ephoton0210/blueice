//! Manual-verification tool for the built-in Help/About/Credits page
//! (`phase-4-human-rendering-path/PLAN.md`), mirroring
//! `blueice-raster`'s `examples/render_demo.rs` -- renders it to a PNG
//! so it can be checked by eye the same way the Phase 2 demo page was.
//! Takes an optional locale as the second CLI argument (default `en`).

fn main() {
    let locale = std::env::args()
        .nth(2)
        .unwrap_or_else(|| blueice_i18n::DEFAULT_LOCALE.to_string());
    let mut page = blueice_engine::Page::new(700.0, 1400.0);
    let url = format!("{}?lang={locale}", blueice_engine::credits::CREDITS_URL);
    page.load_html_str(&blueice_engine::credits::credits_html(&locale), Some(url));
    let pixmap = page.render_visible();
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "credits.png".to_string());
    pixmap.save_png(std::path::Path::new(&path)).unwrap();
    println!("saved {}x{} to {}", pixmap.width, pixmap.height, path);
}
