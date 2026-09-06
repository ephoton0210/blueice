// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration/fixture tests against the public `blueice_html::parse`
//! API (per `testing/TEST_PLAN.md`'s pyramid: cross-crate, exercising
//! the real entry point rather than internal module functions).

use blueice_dom::{Document, NodeData, NodeId};
use blueice_html::parse;

fn find_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
    if let NodeData::Element { tag_name, .. } = doc.data(root) {
        if tag_name == tag {
            return Some(root);
        }
    }
    doc.children(root).find_map(|c| find_by_tag(doc, c, tag))
}

fn text_content(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    collect_text(doc, id, &mut out);
    out
}

fn collect_text(doc: &Document, id: NodeId, out: &mut String) {
    match doc.data(id) {
        NodeData::Text { data } => out.push_str(data),
        _ => {
            for c in doc.children(id) {
                collect_text(doc, c, out);
            }
        }
    }
}

/// A single, realistic small page exercising most of the MVP HTML scope
/// (`phase-2-mvp-scope/PLAN.md`) at once: document structure, a form,
/// a list, a table, inline formatting, and a `<script>` that must not
/// disturb tree construction.
const FIXTURE_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <title>Fixture &amp; Friends</title>
  <meta charset="utf-8">
  <style>body { color: red; }</style>
</head>
<body>
  <h1>Welcome</h1>
  <p>This is <b>bold and <i>italic</b> just italic</i> text.
  <ul>
    <li>one
    <li>two
    <li>three
  </ul>
  <form action="/submit" method="post">
    <label for="name">Name</label>
    <input type="text" name="name" placeholder="Jane">
    <button type="submit">Go</button>
  </form>
  <table>
    stray text before any row
    <tr><td>a</td><td>b</td>
    <tr><td>c</td><td>d</td>
  </table>
  <script>if (1 < 2) { console.log("<not-a-tag>"); }</script>
</body>
</html>
"#;

#[test]
fn fixture_page_parses_into_a_sane_tree_via_the_public_api() {
    let doc = parse(FIXTURE_PAGE);

    let html = doc.children(doc.root()).next().expect("html element");
    assert!(matches!(doc.data(html), NodeData::Element { tag_name, .. } if tag_name == "html"));

    let title = find_by_tag(&doc, html, "title").expect("title");
    assert_eq!(text_content(&doc, title), "Fixture & Friends", "entity in title must resolve");

    let style = find_by_tag(&doc, html, "style").expect("style");
    assert!(find_by_tag(&doc, style, "body").is_none(), "style content is opaque, not tree-constructed");

    let script = find_by_tag(&doc, html, "script").expect("script");
    assert!(find_by_tag(&doc, script, "not-a-tag").is_none());
    assert!(text_content(&doc, script).contains("<not-a-tag>"), "script content is opaque text, unescaped");

    let ul = find_by_tag(&doc, html, "ul").expect("ul");
    let items: Vec<_> = doc.children(ul).filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="li")).collect();
    assert_eq!(items.len(), 3, "each <li> auto-closes the previous one");

    let form = find_by_tag(&doc, html, "form").expect("form");
    assert!(find_by_tag(&doc, form, "input").is_some());
    assert!(find_by_tag(&doc, form, "button").is_some());

    let table = find_by_tag(&doc, html, "table").expect("table");
    let body = find_by_tag(&doc, html, "body").expect("body");
    // the stray text directly inside <table> before any row must be
    // foster-parented out, as a sibling before the table
    let table_pos = doc.children(body).position(|c| c == table).unwrap();
    assert!(table_pos > 0, "foster-parented text should precede the table as body's child");

    let rows: Vec<_> = {
        let tbody = doc.children(table).find(|&c| matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="tbody")).unwrap();
        doc.children(tbody).filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="tr")).collect()
    };
    assert_eq!(rows.len(), 2, "second <tr> implicitly closes the first");

    // misnested <b>/<i> around "bold and italic" must still be
    // adoption-agency-corrected into two properly nested runs
    let p = find_by_tag(&doc, html, "p").expect("p");
    assert!(text_content(&doc, p).contains("bold and italic"));
    assert!(text_content(&doc, p).contains("just italic"));
}

#[test]
fn minimal_fragment_still_gets_implicit_structure() {
    let doc = parse("hello");
    let html = find_by_tag(&doc, doc.root(), "html").expect("implicit html");
    let body = find_by_tag(&doc, html, "body").expect("implicit body");
    assert_eq!(text_content(&doc, body), "hello");
}

#[test]
fn empty_input_still_produces_a_full_implicit_skeleton() {
    let doc = parse("");
    let html = find_by_tag(&doc, doc.root(), "html").expect("implicit html");
    assert!(find_by_tag(&doc, html, "head").is_some());
    assert!(find_by_tag(&doc, html, "body").is_some());
}
