// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn children_tags(doc: &Document, id: NodeId) -> Vec<String> {
    doc.children(id)
        .filter_map(|c| match doc.data(c) {
            NodeData::Element { tag_name, .. } => Some(tag_name.clone()),
            _ => None,
        })
        .collect()
}

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

// implicit html/head/body creation, explicit-doctype parsing,
// attribute preservation, and void-element non-nesting are now
// covered by development/browser_core/testing/fixtures/basic.dat,
// exercised end to end through the public API by tests/fixtures.rs
// -- see TEST_PLAN.md's Definition of Done on not keeping duplicate
// coverage of the same input through the same interface.

#[test]
fn script_content_is_not_tree_constructed() {
    let doc = parse("<body><script>var x = document.createElement('p');</script></body>");
    let script = find_by_tag(&doc, doc.root(), "script").unwrap();
    assert!(find_by_tag(&doc, script, "p").is_none());
    assert_eq!(
        text_content(&doc, script),
        "var x = document.createElement('p');"
    );
}

#[test]
fn an_unclosed_script_at_eof_still_gets_an_implicit_body() {
    // Regression test for a real bug the WPT tree-construction
    // corpus run found (`tests/wpt_corpus.rs`, alone responsible
    // for over half of one file's 153 failures): EOF inside the
    // "Text" insertion mode (an unclosed <script>/<title>/<style>/
    // <textarea>) must be *reprocessed* in the restored original
    // insertion mode per spec, not just consumed -- otherwise the
    // implicit-<body>-insertion cascade a real top-level EOF
    // triggers never runs, and every element that document would
    // otherwise get (starting with <body> itself) silently goes
    // missing.
    let doc = parse("<!doctype html><script>");
    assert!(
        find_by_tag(&doc, doc.root(), "body").is_some(),
        "an unclosed <script> at EOF must not suppress the implicit <body>"
    );
}

#[test]
fn an_unclosed_textarea_at_eof_also_gets_an_implicit_body() {
    // Same bug, different RCDATA element -- proves the fix isn't
    // specific to <script>'s own content model.
    let doc = parse("<textarea>abc");
    assert!(find_by_tag(&doc, doc.root(), "body").is_some());
    let textarea = find_by_tag(&doc, doc.root(), "textarea").unwrap();
    assert_eq!(text_content(&doc, textarea), "abc");
}

#[test]
fn p_auto_closes_on_new_p() {
    let doc = parse("<p>one<p>two");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["p".to_string(), "p".to_string()]
    );
}

#[test]
fn p_auto_closes_on_div() {
    let doc = parse("<p>one<div>two</div>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["p".to_string(), "div".to_string()]
    );
}

#[test]
fn li_auto_closes_previous_li() {
    let doc = parse("<ul><li>a<li>b<li>c</ul>");
    let ul = find_by_tag(&doc, doc.root(), "ul").unwrap();
    let items = children_tags(&doc, ul);
    assert_eq!(
        items,
        vec!["li".to_string(), "li".to_string(), "li".to_string()]
    );
}

#[test]
fn heading_end_tag_closes_any_open_heading() {
    let doc = parse("<h1>title</h2>next");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    // </h2> should close the open <h1>, and "next" becomes a body-level sibling
    assert_eq!(children_tags(&doc, body), vec!["h1".to_string()]);
    assert_eq!(text_content(&doc, body), "titlenext");
}

// Basic adoption-agency (simple and block-furthest-block cases, plus
// the html5lib-tests-verified <a> cases), the anchor-can't-nest-in-
// itself case, plain foster parenting, and basic table structure are
// now covered by adoption-agency.dat, foster-parenting.dat, and
// tables.dat (see tests/fixtures.rs). The block-furthest-block case
// in particular is why those fixtures exist: this crate's own
// char/tag-level assertions here missed a real bug (the adoption
// agency algorithm inserting the new formatting element on the wrong
// side of furthest_block in the stack, causing runaway nesting up to
// the 8-iteration cap) that an exact whole-tree-shape comparison
// caught immediately. See the fix and its comment in
// `adoption_agency` above.

#[test]
fn textarea_rcdata_is_not_tree_constructed_and_resolves_entities() {
    let doc = parse("<textarea>a &amp; <b>not-a-tag</b></textarea>");
    let ta = find_by_tag(&doc, doc.root(), "textarea").unwrap();
    assert!(find_by_tag(&doc, ta, "b").is_none());
    assert_eq!(text_content(&doc, ta), "a & <b>not-a-tag</b>");
}

#[test]
fn comments_and_doctype_produce_no_dom_nodes() {
    let doc = parse(
        "<!DOCTYPE html><!-- top --><html><!-- in html --><body><!-- in body -->x</body></html>",
    );
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(doc.children(body).count(), 1);
    assert_eq!(text_content(&doc, body), "x");
}

#[test]
fn form_element_pointer_prevents_nested_forms() {
    let doc = parse("<form><input><form><input></form></form>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["form".to_string()]);
    let form = doc.children(body).next().unwrap();
    // both inputs land inside the single form; the nested <form> start tag is ignored
    assert_eq!(
        children_tags(&doc, form),
        vec!["input".to_string(), "input".to_string()]
    );
}

#[test]
fn table_inside_p_closes_the_p_in_standards_mode_but_not_in_quirks_mode() {
    // WPT tests3.dat#22 (with doctype -> standards mode): <p> is
    // closed, <table> becomes its sibling.
    let doc = parse("<!doctype html><p><table></table>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["p".to_string(), "table".to_string()]
    );

    // WPT tests3.dat#23 (no doctype -> quirks mode): <table> nests
    // inside the still-open <p> instead.
    let doc = parse("<p><table></table>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["p".to_string()]);
    let p = doc.children(body).next().unwrap();
    assert_eq!(children_tags(&doc, p), vec!["table".to_string()]);
}

#[test]
fn a_stray_end_tag_p_while_in_table_mode_in_quirks_mode_synthesizes_an_empty_p_before_the_table() {
    // WPT tests20.dat#41 (no doctype -> quirks mode):
    // `<p><table></p>`. The </p> reaches "in table" mode (table
    // nested inside p per the quirks-mode carve-out above), falls
    // through to "in body" rules with foster-parenting active, finds
    // no <p> in button scope (the open <table> is itself a scope
    // boundary), and per spec's "missing open p" convention inserts
    // a fresh, empty <p> -- foster-parented to land right before the
    // table -- then immediately closes it.
    let doc = parse("<p><table></p>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["p".to_string()]);
    let outer_p = doc.children(body).next().unwrap();
    assert_eq!(
        children_tags(&doc, outer_p),
        vec!["p".to_string(), "table".to_string()]
    );
    let synthesized_p = doc.children(outer_p).next().unwrap();
    assert_eq!(doc.children(synthesized_p).count(), 0);
}

#[test]
fn form_directly_inside_table_inserts_as_the_tables_own_child_not_foster_parented() {
    // WPT tests20.dat#46: `<!doctype html><table><form><form>`.
    let doc = parse("<table><form><form>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["table".to_string()]);
    let table = doc.children(body).next().unwrap();
    // Exactly one <form>, empty: the second start tag is ignored
    // outright since the form element pointer is already set.
    assert_eq!(children_tags(&doc, table), vec!["form".to_string()]);
    let form = doc.children(table).next().unwrap();
    assert_eq!(doc.children(form).count(), 0);
}

#[test]
fn form_in_table_pointer_stays_set_after_the_table_closes_ignoring_a_later_form() {
    // WPT tests20.dat#47: `<!doctype html><table><form></table><form>`.
    // The form element pointer set by the first <form> is never
    // cleared (no </form> end tag appears anywhere in this input),
    // so the second, post-</table> <form> is ignored outright too.
    let doc = parse("<table><form></table><form>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["table".to_string()]);
    let table = doc.children(body).next().unwrap();
    assert_eq!(children_tags(&doc, table), vec!["form".to_string()]);
}

#[test]
fn form_directly_inside_table_nested_in_an_outer_form_still_gets_the_in_table_carve_out() {
    // WPT tests16.dat#196: `<!doctype html><form><table></form><form></table></form>`.
    // The `</form>` right after `<table>` can't close the outer
    // <form> (the ordinary "has an element in scope" algorithm's
    // <table> boundary blocks it), so it only clears the form
    // element pointer -- letting the second <form>, now inside "in
    // table" mode, get inserted as the table's own child via this
    // carve-out (rather than being ignored like the previous test).
    let doc = parse("<form><table></form><form></table></form>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["form".to_string()]);
    let outer_form = doc.children(body).next().unwrap();
    assert_eq!(children_tags(&doc, outer_form), vec!["table".to_string()]);
    let table = doc.children(outer_form).next().unwrap();
    assert_eq!(children_tags(&doc, table), vec!["form".to_string()]);
    let inner_form = doc.children(table).next().unwrap();
    assert_eq!(doc.children(inner_form).count(), 0);
}

#[test]
fn unknown_elements_parse_generically() {
    let doc = parse("<foo-bar>x</foo-bar>");
    let el = find_by_tag(&doc, doc.root(), "foo-bar").unwrap();
    assert_eq!(text_content(&doc, el), "x");
}

#[test]
fn stray_end_tags_before_html_head_and_after_head_are_ignored() {
    // exercises BeforeHtml/BeforeHead/AfterHead's "ignore this specific
    // end tag" arms (anything other than head/body/html/br)
    let doc = parse("</foo><html></bar><head></baz></head></qux><body>x</body></html>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "x");
}

#[test]
fn explicit_head_close_then_stray_end_tag_in_in_head() {
    let doc = parse("<head></style></head><body>x</body>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "x");
}

#[test]
fn option_auto_closes_previous_option() {
    let doc = parse("<select><option>a<option>b</select>");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    assert_eq!(
        children_tags(&doc, select),
        vec!["option".to_string(), "option".to_string()]
    );
}

#[test]
fn new_heading_start_tag_closes_a_still_open_heading() {
    let doc = parse("<h1>a<h2>b</h2>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["h1".to_string(), "h2".to_string()]
    );
    assert_eq!(text_content(&doc, body), "ab");
}

#[test]
fn form_end_tag_with_no_matching_open_form_is_ignored() {
    let doc = parse("<body></form>x</body>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "x");
}

#[test]
fn stray_formatting_end_tag_with_nothing_open_is_ignored() {
    let doc = parse("<p>text</b>more</p>");
    let p = find_by_tag(&doc, doc.root(), "p").unwrap();
    assert_eq!(text_content(&doc, p), "textmore");
}

#[test]
fn adoption_agency_fe_already_removed_from_stack_is_ignored() {
    // `</div>`'s generic pop walks straight through the still-open
    // `<b>`, dropping it from the stack of open elements without
    // going through adoption agency -- so by the time `</b>` arrives,
    // `<b>` is still in the active formatting list but no longer on
    // the stack (the fe-not-in-stack branch).
    let doc = parse("<div><b>x</div></b>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["div".to_string()]);
    let div = doc.children(body).next().unwrap();
    assert_eq!(children_tags(&doc, div), vec!["b".to_string()]);
    assert_eq!(text_content(&doc, div), "x");
}

#[test]
fn adoption_agency_ignores_formatting_element_blocked_out_of_scope() {
    // `<b>` is still open and still active, but by the time `</b>`
    // arrives, a `<table>` boundary sits between it and the top of
    // the stack -- "in scope" fails, so the token is ignored and `b`
    // keeps wrapping the whole table.
    let doc = parse("<b><table><tr><td></b>x</td></tr></table>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let b = find_by_tag(&doc, body, "b").unwrap();
    let table = find_by_tag(&doc, b, "table").unwrap();
    let td = find_by_tag(&doc, table, "td").unwrap();
    assert_eq!(text_content(&doc, td), "x");
}

#[test]
fn a_start_tag_removes_a_stale_open_a_the_adoption_agency_left_behind() {
    // WPT `tests1.dat#90`: `<a><table><a></table><p><a><div><a>`.
    // When the second `<a>` arrives, the first is blocked "not in
    // scope" by the intervening `<table>` (the previous test's same
    // out-of-scope path), so `adoption_agency("a")` itself is a
    // no-op. `<a>`'s own start-tag rule (distinct from the generic
    // formatting-element handling `<b>`/etc. share) then
    // unconditionally removes that stale, blocked `<a>` from the
    // stack and active-formatting list anyway -- without this, the
    // first `<a>` stays open forever and wrongly keeps swallowing
    // every later sibling as its own descendant.
    let doc = parse("<a><table><a></table><p><a><div><a>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["a".to_string(), "p".to_string(), "div".to_string()]
    );
    let outer_a = doc.children(body).next().unwrap();
    assert_eq!(
        children_tags(&doc, outer_a),
        vec!["a".to_string(), "table".to_string()]
    );
    let p = doc.children(body).nth(1).unwrap();
    assert_eq!(children_tags(&doc, p), vec!["a".to_string()]);
    let div = doc.children(body).nth(2).unwrap();
    assert_eq!(children_tags(&doc, div), vec!["a".to_string()]);
}

#[test]
fn a_start_tag_blocked_out_of_scope_by_a_table_cell_still_clones_correctly_afterward() {
    // WPT `tests1.dat#77`: a variant of the previous test where the
    // blocked `<a>` sits across a `<table>`/`<td>` boundary with real
    // attributes and foster-parented text -- confirms the fix
    // generalizes beyond the minimal repro (attribute preservation on
    // the clone, and a *second* independent adoption-agency run for
    // the third `<a>` after the table closes).
    let doc = parse(r#"<a href="blah">aba<table><a href="foo">br<tr><td></td></tr>x</table>aoe"#);
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["a".to_string(), "a".to_string()]
    );
    let outer_a = doc.children(body).next().unwrap();
    assert_eq!(
        children_tags(&doc, outer_a),
        vec!["a".to_string(), "a".to_string(), "table".to_string()]
    );
    let trailing_a = doc.children(body).nth(1).unwrap();
    assert_eq!(text_content(&doc, trailing_a), "aoe");
    // Both clones inside `outer_a` (after its leading "aba" text
    // node), and the independent trailing `<a>`, all preserve the
    // `href="foo"` attribute from the `<a>` that triggered this
    // fix's cleanup path.
    let inner_clones = doc
        .children(outer_a)
        .filter(|&c| matches!(doc.data(c), NodeData::Element { tag_name, .. } if tag_name == "a"));
    for a in inner_clones.chain(std::iter::once(trailing_a)) {
        let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(a)
        else {
            panic!("expected an element")
        };
        assert_eq!(tag_name, "a");
        assert_eq!(attributes, &[("href".to_string(), "foo".to_string())]);
    }
}

#[test]
fn adoption_agency_ages_out_formatting_elements_deep_in_the_chain() {
    // Five levels of formatting elements between `<b>` and the block
    // that becomes the furthest block: the innermost ones clone
    // normally, but per the (simplified) Noah's-Ark-adjacent aging
    // rule, entries past the third inner-loop iteration are dropped
    // rather than cloned. The exact resulting shape is an
    // implementation detail; what must hold is that no text is lost
    // or duplicated and the parser doesn't panic.
    let doc = parse("<b><i><em><strong><u><div>x</b>y</div>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "xy");
}

#[test]
fn adoption_agency_replaces_an_intervening_formatting_element_in_place_on_the_stack() {
    // `<i>` sits *between* `<b>` (the formatting element being
    // adopted) and `<p>` (the furthest block) -- the inner loop must
    // clone it and keep the clone at `<i>`'s own stack position
    // (not just drop it), since that clone is what ends up wrapping
    // the reparented furthest block. Regression for a bug where the
    // inner loop only ever removed stack entries, never replaced
    // them, silently discarding every intervening formatting clone.
    let doc = parse("<b>1<i>2<p>3</b>4");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let b = find_by_tag(&doc, body, "b").unwrap();
    let inner_i = find_by_tag(&doc, b, "i").unwrap();
    assert_eq!(text_content(&doc, inner_i), "2");
    // A second, cloned <i> is body's own child, wrapping <p>.
    let outer_i = doc
        .children(body)
        .find(|&c| c != b && matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="i"))
        .unwrap();
    let p = find_by_tag(&doc, outer_i, "p").unwrap();
    assert_eq!(children_tags(&doc, p), vec!["b".to_string()]);
    assert_eq!(text_content(&doc, p), "34");
}

#[test]
fn noahs_ark_clause_caps_identical_nested_formatting_elements_at_three() {
    let doc = parse("<p><b><b><b><b><p>x");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let paragraphs: Vec<_> = doc
        .children(body)
        .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="p"))
        .collect();
    assert_eq!(paragraphs.len(), 2);
    let second_p = paragraphs[1];
    // Reconstruction under the second <p> must only recreate three
    // nested <b>s (the fourth was aged out of the active-formatting
    // list when the fourth <b> was originally opened), not four.
    let mut depth = 0;
    let mut node = second_p;
    loop {
        let Some(child) = doc.children(node).next() else {
            break;
        };
        if !matches!(doc.data(child), NodeData::Element{tag_name,..} if tag_name=="b") {
            break;
        }
        depth += 1;
        node = child;
    }
    assert_eq!(depth, 3);
    assert_eq!(text_content(&doc, second_p), "x");
}

#[test]
fn a_formatting_element_from_before_a_table_cell_is_not_reconstructed_inside_it() {
    // `<a>` opened directly inside `<table>` (before any row) gets
    // foster-parented into the DOM but stays on the stack of open
    // elements; entering `<td>` must insert an active-formatting-
    // elements marker so that later content inside the cell doesn't
    // reconstruct a clone of that `<a>` -- "2" must be plain text,
    // not wrapped in a spurious `<a>`.
    let doc = parse("<table><a>1<td>2</td>3</table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let td = find_by_tag(&doc, table, "td").unwrap();
    assert_eq!(children_tags(&doc, td), Vec::<String>::new(), "td must have no element children -- \"2\" must be plain text, not wrapped in a reconstructed <a>");
    assert_eq!(text_content(&doc, td), "2");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let as_: Vec<_> = doc
        .children(body)
        .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="a"))
        .collect();
    assert_eq!(as_.len(), 2, "the <a> is reconstructed once table content resumes after the cell closes (\"3\"), landing back in front of the table");
}

#[test]
fn clearing_the_afe_marker_on_cell_close_does_not_remove_an_earlier_formatting_element() {
    // Clearing "up to the last marker" must stop *at* the marker --
    // an active formatting element pushed before the marker (here,
    // <a>, opened before the cell) must survive the clear and still
    // be available for reconstruction afterward.
    let doc = parse("<table><tr><td><b>x</td><td>y");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let tds: Vec<_> = {
        let mut out = Vec::new();
        for tr in doc.children(table).flat_map(|tb| doc.children(tb)) {
            if matches!(doc.data(tr), NodeData::Element{tag_name,..} if tag_name=="tr") {
                out.extend(doc.children(tr).filter(
                    |&c| matches!(doc.data(c), NodeData::Element{tag_name,..} if tag_name=="td"),
                ));
            }
        }
        out
    };
    assert_eq!(tds.len(), 2);
    assert_eq!(text_content(&doc, tds[0]), "x");
    assert_eq!(
        text_content(&doc, tds[1]),
        "y",
        "the second cell must not reconstruct <b> from the first cell"
    );
}

#[test]
fn table_structure_only_tags_are_ignored_outright_in_plain_body_content() {
    // These tags have no valid meaning directly in "in body" content
    // -- only inside a real table, where the table-family insertion
    // modes handle them. A stray `<col>` after `</table>` has
    // already closed the table must simply vanish, not become an
    // ordinary body-level element.
    let doc = parse("<table></table><col><tbody><td>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["table".to_string()]);
}

#[test]
fn a_second_body_start_tag_merges_attributes_without_nesting() {
    let doc = parse("<body foo='bar'><body foo='baz' yo='mama'>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert!(
        doc.children(body).next().is_none(),
        "a second <body> must not nest a new body element"
    );
    let NodeData::Element { attributes, .. } = doc.data(body) else {
        panic!("expected an element")
    };
    assert!(
        attributes.contains(&("foo".to_string(), "bar".to_string())),
        "the original attribute value must survive (not be overwritten)"
    );
    assert!(
        attributes.contains(&("yo".to_string(), "mama".to_string())),
        "the new attribute must be merged in"
    );
}

#[test]
fn a_stray_end_br_tag_inserts_a_br_element_instead_of_closing_anything() {
    let doc = parse("<body></br foo=\"bar\">");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(children_tags(&doc, body), vec!["br".to_string()]);
}

#[test]
fn a_title_appearing_directly_in_body_still_gets_rcdata_treatment() {
    // Without this, a stray `</body>` inside <title>'s text would be
    // tokenized as a real (and disruptive) end tag instead of
    // staying literal RCDATA content up to the actual </title>.
    let doc = parse("<!DOCTYPE html><body><title>test</body></title>");
    let title = find_by_tag(&doc, doc.root(), "title").unwrap();
    assert_eq!(text_content(&doc, title), "test</body>");
}

#[test]
fn whitespace_leading_a_mixed_character_run_in_column_group_mode_stays_in_the_colgroup() {
    let doc = parse("<table><colgroup> foo</colgroup></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let colgroup = find_by_tag(&doc, table, "colgroup").unwrap();
    assert_eq!(text_content(&doc, colgroup), " ");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert!(
        text_content(&doc, body).starts_with("foo"),
        "the non-whitespace remainder must be foster-parented in front of the table, not lost"
    );
}

#[test]
fn a_raw_null_character_directly_inside_a_table_is_dropped_not_shown() {
    let doc = parse("<body><table>\u{0}filler\u{0}text\u{0}");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "fillertext");
}

#[test]
fn table_direct_thead_tbody_tags_without_implicit_tr_path() {
    let doc = parse(
        "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>d</td></tr></tbody></table>",
    );
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    assert_eq!(
        children_tags(&doc, table),
        vec!["thead".to_string(), "tbody".to_string()]
    );
}

#[test]
fn col_directly_under_table_gets_an_implicit_colgroup() {
    let doc = parse("<table><col><col></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    assert_eq!(children_tags(&doc, table), vec!["colgroup".to_string()]);
    let colgroup = doc.children(table).next().unwrap();
    assert_eq!(
        children_tags(&doc, colgroup),
        vec!["col".to_string(), "col".to_string()]
    );
}

#[test]
fn colgroup_closes_implicitly_on_other_content() {
    let doc = parse("<table><colgroup><col><tbody><tr><td>x</td></tr></tbody></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    assert_eq!(
        children_tags(&doc, table),
        vec!["colgroup".to_string(), "tbody".to_string()]
    );
}

#[test]
fn whitespace_directly_inside_colgroup_is_kept() {
    let doc = parse("<table><colgroup>\n<col></colgroup></table>");
    let colgroup = find_by_tag(&doc, doc.root(), "colgroup").unwrap();
    assert!(doc
        .children(colgroup)
        .any(|c| matches!(doc.data(c), NodeData::Text { .. })));
}

#[test]
fn whitespace_before_and_after_the_body_end_tag_merges_into_one_text_node() {
    // Regression test for a real gap `phase-15-chromium-
    // differential-testing/PLAN.md`'s DOM diff against real
    // Chromium found (invisible in any rendered output, since
    // both shapes are pure whitespace, which is exactly why no
    // fixture's #paint/#layout section had caught it): whitespace
    // between </div> and </body>, and whitespace between </body>
    // and </html> (reprocessed under "in body" rules per the
    // "after body" insertion mode), both insert into <body> and
    // must land in the *same* Text node, not two adjacent ones,
    // per HTML5's "insert a character" algorithm.
    let doc = parse("<html><body><div></div>\n</body>\n</html>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let text_children: Vec<_> = doc
        .children(body)
        .filter(|&c| matches!(doc.data(c), NodeData::Text { .. }))
        .collect();
    assert_eq!(
        text_children.len(),
        1,
        "the two whitespace runs must merge into a single Text node, not stay as two siblings"
    );
    assert_eq!(
        text_content(&doc, body).matches('\n').count(),
        2,
        "both newlines must still be present in the merged node's data"
    );
}

#[test]
fn character_tokens_separated_by_a_comment_do_not_merge() {
    // Corrected by the WPT tree-construction corpus run
    // (`tests/wpt_corpus.rs`, `comments01.dat`): an earlier version
    // of this fix over-generalized and merged text across a
    // dropped comment too, reasoning that since blueice_dom never
    // materializes Comment nodes, nothing sits between the two
    // runs. That reasoning was wrong -- a *real* browser's actual
    // Comment node physically blocks the merge, so "FOO<!--
    // BAR -->BAZ" must produce two separate Text nodes ("FOO",
    // "BAZ"), not one ("FOOBAZ"), even though BlueIce itself never
    // keeps the comment around afterward.
    let doc = parse("<p>a<!--x-->b</p>");
    let p = find_by_tag(&doc, doc.root(), "p").unwrap();
    let text_children: Vec<_> = doc
        .children(p)
        .filter(|&c| matches!(doc.data(c), NodeData::Text { .. }))
        .collect();
    assert_eq!(
        text_children.len(),
        2,
        "a real comment node would block the merge, even though blueice_dom doesn't keep it around"
    );
    assert_eq!(text_content(&doc, p), "ab");
}

#[test]
fn foster_parented_character_tokens_separated_by_a_comment_do_not_merge() {
    // Same correction, exercised on the foster-parenting insertion
    // path (text placed directly inside <table> is foster-parented
    // to just before the table, not inside it).
    let doc = parse("<div><table>a<!--x-->b</table></div>");
    let div = find_by_tag(&doc, doc.root(), "div").unwrap();
    let text_children: Vec<_> = doc
        .children(div)
        .filter(|&c| matches!(doc.data(c), NodeData::Text { .. }))
        .collect();
    assert_eq!(
        text_children.len(),
        2,
        "a real comment node would block the merge on the foster-parenting path too"
    );
}

#[test]
fn character_tokens_separated_by_a_real_element_do_not_merge() {
    // The merge rule must not over-fire: "a" and "c" here are
    // genuinely not adjacent siblings (a real <b> element sits
    // between them in the final tree), so they must stay as
    // three distinct children, not get merged across the element.
    let doc = parse("<p>a<b></b>c</p>");
    let p = find_by_tag(&doc, doc.root(), "p").unwrap();
    assert_eq!(doc.children(p).count(), 3);
}

#[test]
fn nested_table_directly_in_table_mode_closes_the_outer_one() {
    let doc = parse("<table><table><tr><td>inner</td></tr></table></table>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    // the malformed nested <table> closes the (empty) outer one and
    // starts a second, sibling table containing the real content
    let tables: Vec<_> = doc
        .children(body)
        .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name, ..} if tag_name=="table"))
        .collect();
    assert_eq!(tables.len(), 2);
    let td = find_by_tag(&doc, tables[1], "td").unwrap();
    assert_eq!(text_content(&doc, td), "inner");
}

#[test]
fn stray_table_structure_end_tag_in_table_mode_is_ignored() {
    let doc = parse("<table></tbody><tr><td>x</td></tr></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let td = find_by_tag(&doc, table, "td").unwrap();
    assert_eq!(text_content(&doc, td), "x");
}

#[test]
fn caption_closes_implicitly_before_a_row() {
    let doc = parse("<table><caption>Cap<tr><td>x</td></tr></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    assert_eq!(
        children_tags(&doc, table),
        vec!["caption".to_string(), "tbody".to_string()]
    );
}

#[test]
fn caption_closes_implicitly_on_end_table() {
    let doc = parse("<table><caption>Cap</table>after");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let table = find_by_tag(&doc, body, "table").unwrap();
    assert_eq!(children_tags(&doc, table), vec!["caption".to_string()]);
    assert_eq!(text_content(&doc, body), "Capafter");
}

#[test]
fn table_body_section_switches_directly_to_a_sibling_section() {
    let doc = parse("<table><tbody><tr><td>a</td></tr><thead><tr><th>b</th></tr></thead></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    assert_eq!(
        children_tags(&doc, table),
        vec!["tbody".to_string(), "thead".to_string()]
    );
}

#[test]
fn table_body_end_tag_closes_section_back_to_table_mode() {
    let doc = parse("<table><tbody><tr><td>a</td></tr></tbody><tr><td>b</td></tr></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    // a second, implicit tbody is opened for the trailing <tr>
    assert_eq!(
        children_tags(&doc, table),
        vec!["tbody".to_string(), "tbody".to_string()]
    );
}

// second_row_implicitly_closes_the_first is now
// tables.dat#2 (see tests/fixtures.rs).

#[test]
fn table_body_end_tag_implicitly_closes_an_open_row() {
    let doc = parse("<table><tbody><tr><td>a</tbody><tr><td>b</table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let bodies: Vec<_> = doc
        .children(table)
        .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name, ..} if tag_name=="tbody"))
        .collect();
    assert_eq!(bodies.len(), 2);
}

// second_cell_implicitly_closes_the_first is now
// tables.dat#3 (see tests/fixtures.rs).

#[test]
fn row_end_tag_implicitly_closes_an_open_cell() {
    let doc = parse("<table><tr><td>a</tr><tr><td>b</tr></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let tbody = doc.children(table).next().unwrap();
    assert_eq!(
        children_tags(&doc, tbody),
        vec!["tr".to_string(), "tr".to_string()]
    );
}

#[test]
fn hr_closes_an_open_p_and_becomes_its_sibling() {
    let doc = parse("<p><hr></p>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    // <hr> closes the first (now-empty) <p>; the stray </p> that
    // follows has no matching open <p> in scope, so per spec it
    // inserts (then immediately closes) a second, empty <p>.
    assert_eq!(
        children_tags(&doc, body),
        vec!["p".to_string(), "hr".to_string(), "p".to_string()]
    );
}

#[test]
fn a_stray_end_p_tag_with_nothing_open_inserts_an_empty_p() {
    // A bare `</p>` before any real content is ignored outright by
    // the "before html"/"before head" modes (per spec, matching
    // real browsers) -- the empty-<p>-insertion rule only fires once
    // a stray `</p>` is actually processed under "in body" rules, so
    // this needs other content first to get there.
    let doc = parse("<div></p>");
    let div = find_by_tag(&doc, doc.root(), "div").unwrap();
    assert_eq!(children_tags(&doc, div), vec!["p".to_string()]);
}

#[test]
fn an_immediately_closed_empty_comment_does_not_swallow_following_markup() {
    let doc = parse("<!--><div>--<!-->");
    let div = find_by_tag(&doc, doc.root(), "div");
    assert!(
        div.is_some(),
        "the <div> after an abruptly-closed `<!-->` comment must still be parsed as an element"
    );
    assert_eq!(text_content(&doc, div.unwrap()), "--");
}

#[test]
fn a_comment_with_one_extra_dash_before_close_also_closes_abruptly() {
    // `<!--->` is "comment start dash" seeing `>` immediately --
    // an empty-ish ("-") comment, not a signal to scan further.
    let doc = parse("<!---><div>x</div>");
    let div = find_by_tag(&doc, doc.root(), "div");
    assert!(div.is_some());
    assert_eq!(text_content(&doc, div.unwrap()), "x");
}

#[test]
fn a_comment_closed_via_the_bang_variant_does_not_swallow_following_text() {
    // `--!>` ("comment end bang" state, an "incorrectly closed
    // comment" parse error) is *also* a valid comment terminator,
    // alongside plain `-->`.
    let doc = parse("FOO<!-- BAR --!>BAZ");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let text_children: Vec<_> = doc
        .children(body)
        .filter(|&c| matches!(doc.data(c), NodeData::Text { .. }))
        .collect();
    assert_eq!(
        text_children.len(),
        2,
        "the dropped comment must still block FOO/BAZ from merging into one text node"
    );
    assert_eq!(text_content(&doc, body), "FOOBAZ");
}

#[test]
fn col_start_tag_attributes_survive_the_implicit_colgroup_reprocess() {
    let doc = parse("<table><col foo='bar'>");
    let col = find_by_tag(&doc, doc.root(), "col").unwrap();
    assert_eq!(
        doc.data(col),
        &NodeData::Element {
            tag_name: "col".to_string(),
            attributes: vec![("foo".to_string(), "bar".to_string())]
        }
    );
}

#[test]
fn a_second_html_start_tag_merges_new_attributes_without_overwriting_existing_ones() {
    let doc = parse("<html c=d><body></body><html a=b>");
    let html = find_by_tag(&doc, doc.root(), "html").unwrap();
    let NodeData::Element { attributes, .. } = doc.data(html) else {
        panic!("expected an element")
    };
    assert!(
        attributes.contains(&("c".to_string(), "d".to_string())),
        "the original attribute must survive"
    );
    assert!(
        attributes.contains(&("a".to_string(), "b".to_string())),
        "the new attribute from the second <html> tag must be merged in"
    );
}

#[test]
fn a_style_tag_after_an_explicit_head_close_still_lands_in_head() {
    let doc = parse("<head></head><style>x</style>");
    let head = find_by_tag(&doc, doc.root(), "head").unwrap();
    let style = find_by_tag(&doc, doc.root(), "style");
    assert!(
        style.is_some(),
        "style after </head> must still parse as an element"
    );
    assert!(
        doc.children(head).any(|c| Some(c) == style),
        "style must be a child of <head>, not implicitly moved into <body>"
    );
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert!(find_by_tag(&doc, body, "style").is_none());
}

#[test]
fn a_style_tag_directly_inside_a_table_is_a_child_of_the_table_not_foster_parented() {
    let doc = parse("<table><style>x</style></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let style = find_by_tag(&doc, doc.root(), "style");
    assert!(style.is_some());
    assert!(doc.children(table).any(|c| Some(c) == style), "<style> directly inside <table> must be inserted as the table's own child, not foster-parented in front of it");
}

#[test]
fn a_second_select_start_tag_closes_the_first_instead_of_nesting() {
    let doc = parse("<select><select>X");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let selects: Vec<_> = doc
        .children(body)
        .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name, ..} if tag_name=="select"))
        .collect();
    assert_eq!(
        selects.len(),
        1,
        "the second <select> must close the first, not nest inside it"
    );
    assert!(
        doc.children(selects[0]).next().is_none(),
        "the (closed) <select> must have no children of its own"
    );
    assert_eq!(text_content(&doc, body), "X");
}

#[test]
fn an_input_start_tag_inside_a_select_closes_it_and_becomes_a_sibling() {
    let doc = parse("<select><input>X");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["select".to_string(), "input".to_string()]
    );
    let select = find_by_tag(&doc, body, "select").unwrap();
    assert!(
        doc.children(select).next().is_none(),
        "the <select> must be empty -- <input> must not nest inside it"
    );
}

#[test]
fn a_stray_end_thead_tag_inside_an_implicit_tbody_cell_is_ignored() {
    let doc = parse("<table><td></thead>A");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let td = find_by_tag(&doc, table, "td").unwrap();
    assert_eq!(
        text_content(&doc, td),
        "A",
        "a </thead> with no matching open <thead> must be ignored, leaving \"A\" inside the cell"
    );
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["table".to_string()],
        "\"A\" must not be foster-parented in front of the table"
    );
}

#[test]
fn a_leading_newline_right_after_pre_open_is_stripped() {
    let doc = parse("<pre>\nfoo</pre>");
    let pre = find_by_tag(&doc, doc.root(), "pre").unwrap();
    assert_eq!(text_content(&doc, pre), "foo");
}

#[test]
fn only_the_first_of_two_leading_newlines_in_pre_is_stripped() {
    let doc = parse("<pre>\n\nfoo</pre>");
    let pre = find_by_tag(&doc, doc.root(), "pre").unwrap();
    assert_eq!(text_content(&doc, pre), "\nfoo");
}

#[test]
fn a_leading_newline_right_after_textarea_open_is_stripped() {
    let doc = parse("<textarea>\nfoo</textarea>");
    let ta = find_by_tag(&doc, doc.root(), "textarea").unwrap();
    assert_eq!(text_content(&doc, ta), "foo");
}

#[test]
fn whitespace_leading_a_mixed_character_run_in_after_head_mode_inserts_under_html() {
    // Once `</head>` has already been explicitly closed and popped,
    // "after head" mode's insertion point is the <html> element
    // itself (not head, which is no longer on the stack) -- a real
    // per-character tokenizer inserts leading whitespace there and
    // only the first non-whitespace character triggers the implicit
    // <body>; blueice's tokenizer batches the whole run into one
    // token, so this exercises the split that recovers the same
    // split point.
    let doc = parse("<head></head> x");
    let html = find_by_tag(&doc, doc.root(), "html").unwrap();
    let text_children: Vec<_> = doc
        .children(html)
        .filter(|&c| matches!(doc.data(c), NodeData::Text { .. }))
        .collect();
    assert_eq!(text_children.len(), 1);
    assert_eq!(text_content(&doc, text_children[0]), " ");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "x");
}

#[test]
fn whitespace_leading_a_mixed_character_run_still_in_head_mode_stays_in_head() {
    // Contrast with the previous test: here <head> is never
    // explicitly closed, so the implicit "in head" -> "after head"
    // transition only happens once a non-whitespace character
    // arrives -- the leading whitespace is processed while head is
    // still open and current, landing inside it.
    let doc = parse("<!doctype html><script> <!-- </script> --> </script> EOF");
    let head = find_by_tag(&doc, doc.root(), "head").unwrap();
    let text_children: Vec<_> = doc
        .children(head)
        .filter(|&c| matches!(doc.data(c), NodeData::Text { .. }))
        .collect();
    assert_eq!(text_children.len(), 1);
    assert_eq!(text_content(&doc, text_children[0]), " ");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "-->  EOF");
}

#[test]
fn whitespace_leading_a_mixed_character_run_after_body_close_stays_in_body() {
    let doc = parse("<html><body>a</body> x");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "a x");
}

#[test]
fn whitespace_leading_a_mixed_character_run_before_html_or_head_exist_is_dropped_not_inserted() {
    // WPT `doctype01.dat#30`: a bogus DOCTYPE (tokenized per the
    // "bogus DOCTYPE" state -- everything up to the *first* raw `>`
    // is discarded, including a nested `<!-- ... -->`-shaped run,
    // since that state doesn't know about comments at all) is
    // immediately followed by a lone newline, then stray text. Since
    // `<html>`/`<head>` don't exist yet, the leading whitespace in
    // that mixed run must be dropped outright -- not inserted as
    // text once an implicit `<head>` gets created for the
    // non-whitespace remainder.
    let doc = parse("<!DOCTYPE root-element [SYSTEM OR PUBLIC FPI] \"uri\" [ \n<!-- internal declarations -->\n]>");
    let head = find_by_tag(&doc, doc.root(), "head").unwrap();
    assert_eq!(doc.children(head).count(), 0);
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "]>");
}

#[test]
fn an_unterminated_quoted_attribute_value_at_eof_discards_the_whole_start_tag() {
    // WPT `webkit02.dat#4`: the tokenizer never emits `<img ...>` at
    // all (see `tokenizer.rs`'s EOF-in-tag fix), so it never reaches
    // the tree builder in the first place -- body stays empty.
    let doc = parse("<html><body><img src=\"\" border=\"0\" alt=\"><div>A</div></body></html>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(doc.children(body).count(), 0);
}

#[test]
fn a_raw_null_character_in_body_content_is_dropped_not_shown() {
    let doc = parse("<body>\u{0}");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert!(
        doc.children(body).next().is_none(),
        "a lone NUL character token in body must be ignored outright, not inserted as text"
    );
}

#[test]
fn a_raw_null_character_inside_a_select_is_dropped_not_shown() {
    let doc = parse("<html><select>\u{0}");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    assert!(doc.children(select).next().is_none());
}

#[test]
fn a_literal_dashdash_gt_inside_script_escaped_mode_still_closes_the_element_normally() {
    // `<!--` inside <script> enters "escaped" mode, but a genuine
    // `</script>` end tag still closes the element from there --
    // the escaped-mode machinery only matters for what counts as
    // literal text vs. a real closing tag, not for hiding the real
    // end tag itself.
    let doc = parse("<!doctype html><script> <!-- </script> --> </script> EOF");
    let script = find_by_tag(&doc, doc.root(), "script").unwrap();
    assert_eq!(text_content(&doc, script), " <!-- ");
}

#[test]
fn a_nested_script_open_tag_inside_escaped_mode_enters_double_escaped_mode() {
    // Once double-escaped, even a literal `</script>` is just text
    // -- only the closing `</script>` *outside* any nested
    // `<script>...</script>` pair actually ends the element.
    let doc = parse("<script>FOO<!--<script></script>-->BAR</script>QUX");
    let script = find_by_tag(&doc, doc.root(), "script").unwrap();
    assert_eq!(text_content(&doc, script), "FOO<!--<script></script>-->BAR");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "QUX");
}

#[test]
fn double_escaped_mode_only_toggles_back_on_a_real_closing_script_marker() {
    let doc = parse("<script>a<!--<script>b</script>c</script>d");
    let script = find_by_tag(&doc, doc.root(), "script").unwrap();
    // After `</script>` (the nested one) toggles back to escaped
    // mode, `c` is escaped-mode text and the *next* `</script>`
    // genuinely closes the element.
    assert_eq!(text_content(&doc, script), "a<!--<script>b</script>c");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "d");
}

#[test]
fn hr_inside_a_select_closes_an_open_option_and_optgroup_but_not_the_select() {
    let doc = parse("<select><optgroup><option>x<hr>");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    assert_eq!(
        children_tags(&doc, select),
        vec!["optgroup".to_string(), "hr".to_string()]
    );
    let optgroup = find_by_tag(&doc, select, "optgroup").unwrap();
    assert_eq!(children_tags(&doc, optgroup), vec!["option".to_string()]);
}

#[test]
fn an_unrecognized_start_tag_inside_select_is_inserted_normally_per_customizable_select() {
    // Per the *current* WHATWG spec (the "Customizable Select"
    // feature, already shipped in Chromium and Gecko -- see
    // `phase-2-mvp-scope/PLAN.md`'s cross-reference for the full
    // history of getting this right): the dedicated "in select"/
    // "in select in table" insertion modes were removed entirely.
    // `<select>`'s content is now ordinary "in body" content -- a
    // `<div>`/`<button>`/`<img>` reached while a `<select>` is open
    // is inserted completely normally, exactly like anywhere else.
    // Confirmed directly against the merged spec PR
    // (whatwg/html#10548) and its own test-suite update
    // (html5lib/html5lib-tests#178), not just re-derived from
    // memory. `<option>`/`<optgroup>`/`<hr>`/`<input>`/`<select>`
    // remain the only elements with any select-awareness at all.
    //
    // webkit02.dat#35: <div>/<i> are real nested content now.
    let doc = parse("<select><div><i></div><option>option");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    assert_eq!(
        children_tags(&doc, select),
        vec!["div".to_string(), "i".to_string()]
    );
    let div = doc.children(select).next().unwrap();
    assert_eq!(children_tags(&doc, div), vec!["i".to_string()]);
    let outer_i = doc.children(select).nth(1).unwrap();
    assert_eq!(children_tags(&doc, outer_i), vec!["option".to_string()]);

    // webkit02.dat#38: <button> is real content, containing "button" as its own text.
    let doc = parse("<select><button>button</select>");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    let button = find_by_tag(&doc, select, "button").unwrap();
    assert_eq!(text_content(&doc, button), "button");

    // webkit02.dat#42: <div>/<img> are real nested content too.
    let doc = parse("<select><div><option><img>option</option></div></select>");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    let div = find_by_tag(&doc, select, "div").unwrap();
    let option = find_by_tag(&doc, div, "option").unwrap();
    assert_eq!(children_tags(&doc, option), vec!["img".to_string()]);
    assert_eq!(text_content(&doc, option), "option");
}

#[test]
fn a_nested_select_start_tag_closes_the_outer_one_leaving_intervening_content_in_place() {
    // webkit02.dat#40/#41: since `<button>`/`<div>` are now real
    // content (previous test), the nested `<select>` start tag finds
    // the *outer* `<select>` in scope regardless of how deep the
    // current node is nested inside it, and closes it (per the
    // current spec's `<select>` start-tag rule: "if a select is in
    // scope, pop until select popped" -- it never opens a second
    // select for the token itself). Everything already inserted
    // before that point stays exactly where it was in the DOM.
    let doc = parse("<select><button><select></select></button></select>");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    assert_eq!(children_tags(&doc, select), vec!["button".to_string()]);
    assert_eq!(
        doc.children(find_by_tag(&doc, select, "button").unwrap())
            .count(),
        0
    );

    let doc = parse("<select><button><div><select></select>");
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    let button = find_by_tag(&doc, select, "button").unwrap();
    assert_eq!(children_tags(&doc, button), vec!["div".to_string()]);
}

#[test]
fn a_formatting_element_inside_select_gets_real_adoption_agency_treatment() {
    // tests1.dat#29/#99, confirmed against the merged spec PR's own
    // updated test expectations (html5lib/html5lib-tests#178): `<b>`
    // is a real formatting element even inside a `<select>` now.
    // The nested `<select>` start tag pops the outer select *and*
    // `<b>` off the stack of open elements (but not out of the
    // active-formatting-elements list, which only tracks stack
    // membership separately) -- so the next `<option>` triggers
    // `reconstruct_active_formatting_elements` to rebuild a *second*,
    // sibling `<b>` clone at the body level (select is no longer
    // open to receive it), and the final `</b>` end tag's adoption
    // agency run (finding no "special" element below `<b>` in the
    // stack, since `<option>` no longer wrongly counts as one --
    // see `SPECIAL_ELEMENTS`'s own docs) just pops both `<b>` and
    // `<option>` off the stack without touching the DOM they already
    // built, leaving "X" to land as body's own trailing text.
    let doc = parse("<select><b><option><select><option></b></select>X");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(
        children_tags(&doc, body),
        vec!["select".to_string(), "b".to_string()]
    );
    assert_eq!(text_content(&doc, body), "X");

    let select = doc.children(body).next().unwrap();
    assert_eq!(children_tags(&doc, select), vec!["b".to_string()]);
    let first_b = doc.children(select).next().unwrap();
    assert_eq!(children_tags(&doc, first_b), vec!["option".to_string()]);

    let second_b = doc.children(body).nth(1).unwrap();
    assert_eq!(children_tags(&doc, second_b), vec!["option".to_string()]);
}

#[test]
fn a_table_structure_tag_closes_a_select_opened_inside_a_table() {
    // Unlike plain "in select" (where such tags are simply
    // ignored), a <select> opened while already inside table
    // structure enters "in select in table" mode, where these tags
    // close the select instead -- reprocessing <tr> back under
    // "in table body" rules once <select> is closed, landing it
    // inside <tbody> (the select itself was foster-parented out in
    // front of the table when it was opened, same as any other
    // non-table content directly inside <tbody>).
    let doc = parse("<table><tbody><select><tr>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let select = find_by_tag(&doc, body, "select").unwrap();
    assert!(
        doc.children(body).any(|c| Some(c) == Some(select)),
        "the (empty, closed) <select> must be foster-parented in front of the table"
    );
    assert!(doc.children(select).next().is_none());
    let tbody = find_by_tag(&doc, doc.root(), "tbody").unwrap();
    assert_eq!(
        children_tags(&doc, tbody),
        vec!["tr".to_string()],
        "<tr> must land inside <tbody>, not be dropped"
    );
}

#[test]
fn a_table_structure_tag_is_ignored_by_a_select_opened_outside_a_table() {
    let doc = parse("<select><tr>x");
    assert!(
        find_by_tag(&doc, doc.root(), "tr").is_none(),
        "a plain (non-table) <select> must ignore a stray <tr> outright, not close on it"
    );
    let select = find_by_tag(&doc, doc.root(), "select").unwrap();
    assert_eq!(
        text_content(&doc, select),
        "x",
        "content after the ignored <tr> still lands inside the (still-open) <select>"
    );
}

#[test]
fn a_hidden_input_directly_inside_a_table_is_inserted_normally_not_foster_parented() {
    let doc = parse("<table><input type=hidDEN></table>");
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let input = find_by_tag(&doc, doc.root(), "input");
    assert!(input.is_some());
    assert!(doc.children(table).any(|c| Some(c) == input));
}

#[test]
fn a_non_hidden_input_directly_inside_a_table_is_still_foster_parented() {
    let doc = parse("<table><input type=text></table>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    let table = find_by_tag(&doc, doc.root(), "table").unwrap();
    let input = find_by_tag(&doc, doc.root(), "input");
    assert!(input.is_some());
    assert!(
        doc.children(body).any(|c| Some(c) == input),
        "a non-hidden <input> keeps the normal foster-parenting behavior"
    );
    assert!(!doc.children(table).any(|c| Some(c) == input));
}

#[test]
fn a_second_button_start_tag_closes_the_first_instead_of_nesting() {
    let doc = parse("<p><button><button>");
    let p = find_by_tag(&doc, doc.root(), "p").unwrap();
    let buttons: Vec<_> = doc
        .children(p)
        .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name, ..} if tag_name=="button"))
        .collect();
    assert_eq!(
        buttons.len(),
        2,
        "the second <button> must close the first, becoming its sibling, not nesting inside it"
    );
}

#[test]
fn an_end_button_tag_closes_a_still_open_p_inside_it() {
    let doc = parse("<button><p></button>x");
    let button = find_by_tag(&doc, doc.root(), "button").unwrap();
    assert_eq!(children_tags(&doc, button), vec!["p".to_string()]);
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    // "x" lands after the (now-closed) <button>, not inside it.
    let text_after = doc
        .children(body)
        .find(|&c| matches!(doc.data(c), NodeData::Text { .. }));
    assert!(text_after.is_some());
    assert_eq!(text_content(&doc, body), "x");
}

#[test]
fn content_after_body_close_reopens_body_processing() {
    let doc = parse("<html><body>x</body> extra</html>");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "x extra");
}

#[test]
fn content_after_html_close_still_lands_in_body() {
    let doc = parse("<html><body>x</body></html> tail");
    let body = find_by_tag(&doc, doc.root(), "body").unwrap();
    assert_eq!(text_content(&doc, body), "x tail");
}

// ---- interaction tests: coverage-number gaps often hide at feature
// boundaries, not inside a single feature. formatting_element_-
// reconstructs_across_a_new_block_boundary and adoption_agency_-
// foster_parents_the_relocated_node_when_common_ancestor_is_a_table
// were added here by a dedicated post-implementation test-review
// pass (per TEST_PLAN.md's Definition of Done), then migrated to
// reconstruction.dat and foster-parenting.dat#2 respectively once
// the shared fixture interface existed (see tests/fixtures.rs) --
// the second one is also how the block-furthest-block adoption-
// agency bug referenced above was caught in the first place.
