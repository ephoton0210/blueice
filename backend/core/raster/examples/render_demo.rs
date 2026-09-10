use blueice_css::{cascade, ua_stylesheet, Origin};
use blueice_layout::{layout, Constraints};
use blueice_paint::paint;
use blueice_raster::rasterize;

fn main() {
    let html = r#"
        <html><head><title>Demo</title></head>
        <body>
        <h1>Welcome to BlueIce</h1>
        <p>This is a <b>bold</b> and <i>italic</i> test paragraph with enough words in it that it should wrap onto more than one line at this width.</p>
        <div style="background-color: #eeeeee; border: 2px solid #333333; padding: 10px; margin-top: 10px;">
          <p>A box with background and border.</p>
        </div>
        </body></html>
    "#;
    let css = "body { color: #222222; } h1 { color: #1a5fb4; }";
    let doc = blueice_html::parse(html);
    let ua = ua_stylesheet();
    let author = blueice_css::parse(css).rules;
    let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
    let fragment = layout(
        &doc,
        doc.root(),
        &styles,
        Constraints {
            available_width: 500.0,
        },
    );
    let frame = paint(&fragment, &styles);
    let pixmap = rasterize(&frame);
    let path = std::path::Path::new("/tmp/claude-1000/-home-ephoton-git-blueice/2d9fafe2-9d4b-4664-b00a-6644ade23bc5/scratchpad/demo.png");
    pixmap.save_png(path).unwrap();
    println!(
        "saved {}x{} to {}",
        pixmap.width,
        pixmap.height,
        path.display()
    );
}
