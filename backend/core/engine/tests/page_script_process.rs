// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A document's scripts in a process of their own, driven through the page's
//! public interface with the real `blueice-core` binary as the child.

use blueice_engine::Page;
use blueice_paint::PaintCommand;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn setup() {
    blueice_engine::enable_web_scripts(true);
    blueice_engine::enable_async_scripts(true);
    blueice_engine::enable_script_process(Some(PathBuf::from(env!("CARGO_BIN_EXE_blueice-core"))));
}

fn page(html: &str) -> Page {
    setup();
    let mut page = Page::new(400.0, 300.0);
    page.load_html_str(html, Some("https://example.com/p".to_string()));
    page
}

fn texts(page: &Page) -> Vec<String> {
    page.render()
        .commands
        .iter()
        .filter_map(|c| match c {
            PaintCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn until(page: &mut Page, what: &str, mut done: impl FnMut(&Page) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(page) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {:?}", texts(page));
        page.tick_scripts();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn scripts_run_in_a_process_and_their_changes_are_painted() {
    let page = page(
        "<body><p id=a>old</p><script>document.getElementById('a').textContent = 'one';</script>\
         <script>document.getElementById('a').textContent += ' two';</script></body>",
    );
    assert_eq!(page.script_backend(), "process");
    assert_ne!(page.script_process_id(), Some(std::process::id()));
    assert_eq!(texts(&page), vec!["one", "two"]);
}

#[test]
fn timers_keep_running_in_the_process_and_each_change_comes_back() {
    let mut page = page(
        "<body><p id=a>0</p><script>let n = 0; (function again() { n += 1; \
         document.getElementById('a').textContent = String(n); if (n < 5) setTimeout(again, 0); })();\
         </script></body>",
    );
    until(&mut page, "the counter to reach 5", |p| texts(p) == vec!["5"]);
}

#[test]
fn scripts_see_the_pages_layout_and_styles_by_asking_for_them() {
    let mut page = page(
        "<body style='margin:0'><div id=box style='width:200px;height:50px;display:flex;color:#ff0000'>x</div>\
         <p id=out>pending</p><script>\
         const box = document.getElementById('box');\
         setTimeout(() => {\
           const r = box.getBoundingClientRect();\
           document.getElementById('out').textContent = \
             [r.width, r.height, getComputedStyle(box).display, getComputedStyle(box).color].join('|');\
         }, 0);</script></body>",
    );
    until(&mut page, "the script to read its layout", |p| {
        texts(p).iter().any(|t| t.contains('|'))
    });
    let out = texts(&page).into_iter().find(|t| t.contains('|')).unwrap();
    assert!(out.starts_with("200|50|flex|"), "{out}");
}

#[test]
fn a_long_script_turn_does_not_hold_the_page() {
    let mut page = page(
        "<body><p id=a>old</p><script>\
         setTimeout(function () { var end = Date.now() + 600; while (Date.now() < end) {} \
           document.getElementById('a').textContent = 'new'; }, 0);</script></body>",
    );
    std::thread::sleep(Duration::from_millis(20));
    let begun = Instant::now();
    page.tick_scripts();
    assert!(begun.elapsed() < Duration::from_millis(300), "starting the turn returned at once");
    let asked = Instant::now();
    page.scroll_by(10.0);
    assert_eq!(texts(&page), vec!["old"]);
    assert!(asked.elapsed() < Duration::from_millis(200), "painting did not wait: {:?}", asked.elapsed());
    until(&mut page, "the long turn's result", |p| texts(p) == vec!["new"]);
    assert!(begun.elapsed() >= Duration::from_millis(500));
}

#[test]
fn a_dead_scripts_process_costs_the_page_its_scripts_and_nothing_else() {
    let mut page = page(
        "<body><p id=a>kept</p><script>\
         setInterval(() => { document.getElementById('a').textContent = String(Date.now()); }, 5);\
         </script></body>",
    );
    assert_eq!(page.script_backend(), "process");
    let pid = page.script_process_id().expect("a process");
    // Kill it as a crash would.
    std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while page.script_backend() == "process" && Instant::now() < deadline {
        page.tick_scripts();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(page.script_backend(), "lost", "the page noticed");
    // The page still paints what it last had, and scrolls.
    assert_eq!(texts(&page).len(), 1);
    page.scroll_by(5.0);
    assert!(!page.render_visible().pixels.is_empty());
    // And a new document gets scripts again.
    page.load_html_str(
        "<body><p id=b>fresh</p><script>document.getElementById('b').textContent = 'alive';</script></body>",
        Some("https://example.com/next".to_string()),
    );
    assert_eq!(page.script_backend(), "process");
    assert_eq!(texts(&page), vec!["alive"]);
}
