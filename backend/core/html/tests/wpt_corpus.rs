// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CI gate (see `wpt_tree_construction_corpus`'s trailing `assert!`s)
//! against the full WPT/html5lib-tests tree-construction corpus
//! (`development/browser_core/reference/wpt/`, fetched by `.github/
//! workflows/ci.yml` before this test runs -- see that directory's own
//! `README.md` for the fetch commands, needed for a local run too) --
//! covers real HTML5 compatibility, not just the handful of hand-picked
//! cases already in `../../../development/browser_core/testing/fixtures/`.
//! Skips itself (doesn't fail) when the corpus isn't checked out
//! locally, so a plain `cargo test --workspace` without that optional
//! fetch still passes for local development. Reuses `blueice_testing`'s
//! fixture parser unmodified -- the corpus's own `#data`/`#errors`/
//! `#document`/`#document-fragment` format is exactly what that parser
//! already reads.
//!
//! A byte-exact dump comparison against the raw corpus starts at ~19%
//! pass, almost entirely because of already-known, deliberate MVP scope
//! cuts (`blueice_dom` never materializes Comment/Doctype nodes at all,
//! SVG/MathML foreign content, `<template>`, the full named-character-
//! reference table, ...) rather than bugs -- [`strip_unsupported_lines`]
//! and [`likely_out_of_scope_reason`] exist to separate that expected
//! noise from genuine failures worth a human reading, not to make the
//! number look better. Over thirty real bugs were found and fixed this
//! way across five triage passes (see `testing/TEST_PLAN.md`'s "WPT
//! tree-construction corpus" section for the full list); the pass rate
//! after normalizing away known scope cuts moved 39.0% -> 58.3% as a
//! direct result, with unclassified failures (the ones actually worth
//! reading) dropping from 669 to **0** -- every remaining case from the
//! third pass turned out to be a real, fixable bug once traced against
//! an authoritative-enough source (the actual current WHATWG spec text
//! and/or its own merged test-suite updates, not just a reference
//! implementation's source, which can itself be stale -- see
//! `likely_out_of_scope_reason`'s own doc comment for the one case that
//! taught this the hard way). With genuine failures at zero, this
//! became worth gating on for real: `wpt_tree_construction_corpus` now
//! asserts `unclassified` is empty *and* that the total pass count
//! hasn't dropped below [`BASELINE_PASSED`] (a second, coarser check
//! against a real regression getting silently absorbed into an
//! already-known classification bucket).

use blueice_testing::load_fixtures;
use std::path::PathBuf;

/// Drops any dump line representing a `<!DOCTYPE>` or `<!--comment-->`
/// node, at any depth -- `blueice_dom::dump` never emits these
/// (comments/doctypes aren't materialized as nodes at all, a
/// deliberate scope decision predating this corpus run, not something
/// this comparison should count as a failure). Applied to both sides
/// so the comparison is fair to what BlueIce actually claims to
/// support, rather than penalizing it for a documented non-goal.
///
/// A comment's own content can itself contain a literal newline (e.g.
/// `comments01.dat`'s `<!-- BAR --!\n>BAZ -->` case), which the
/// html5lib-tests dump format renders as a *second* raw line with no
/// `<!--`/`| ` marker of its own -- just the comment's leftover
/// content, ending in `-->`. A naive per-line filter drops the first
/// line (it starts with `<!--`) but leaves that continuation line
/// behind, producing a spurious mismatch against BlueIce's side (which
/// has no such line at all, comments never being materialized). Fixed
/// by tracking "still inside an unterminated comment's continuation"
/// across lines and dropping those too, until the line that actually
/// closes the comment (`-->`) is reached.
fn strip_unsupported_lines(dump: &str) -> String {
    let mut out = Vec::new();
    let mut in_comment_continuation = false;
    for line in dump.lines() {
        let content = line.strip_prefix("| ").unwrap_or(line).trim_start();
        if in_comment_continuation {
            if content.ends_with("-->") {
                in_comment_continuation = false;
            }
            continue;
        }
        if content.starts_with("<!--") {
            if !content.ends_with("-->") {
                in_comment_continuation = true;
            }
            continue;
        }
        if content.starts_with("<!DOCTYPE") {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// The Phase 2 MVP HTML element list (`phase-2-mvp-scope/PLAN.md`'s
/// "MVP HTML scope (decided)"), used only to guess whether a failing
/// WPT case exercises an element BlueIce was never scoped to support
/// at all (`<ruby>`/`<rp>`/`<rt>`, `<listing>`/`<plaintext>`,
/// `<marquee>`, ...) -- not a claim that every element on this list is
/// bug-free, just that an element *off* it failing is expected, not a
/// regression.
const MVP_ELEMENTS: &[&str] = &[
    "html", "head", "title", "meta", "link", "style", "script", "body", "div", "span", "p", "br", "hr", "section", "article", "header", "footer", "nav", "main", "aside", "ul", "ol", "li", "pre",
    "blockquote", "figure", "figcaption", "h1", "h2", "h3", "h4", "h5", "h6", "a", "b", "i", "em", "strong", "u", "small", "code", "sub", "sup", "form", "input", "button", "label", "select",
    "option", "optgroup", "textarea", "fieldset", "legend", "table", "caption", "colgroup", "col", "thead", "tbody", "tfoot", "tr", "td", "th", "img",
];

/// A rough, classification-only tag-name scanner over raw `#data` --
/// not a real tokenizer (doesn't understand RAWTEXT/RCDATA/comments/
/// attribute values that might coincidentally contain `<letter`), but
/// good enough to guess "does this input mention any element outside
/// MVP scope" without needing to actually parse it.
fn mentions_element_outside_mvp_scope(data: &str) -> bool {
    let bytes = data.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-') {
                end += 1;
            }
            let tag = data[start..end].to_ascii_lowercase();
            if !MVP_ELEMENTS.contains(&tag.as_str()) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// A quick, deliberately coarse guess at *why* a failing case doesn't
/// match -- not a claim that the guessed category is definitely
/// right, just enough to separate "almost certainly an already-known,
/// documented MVP scope cut" from "worth a human actually reading the
/// diff" so a 1000+-case corpus is triageable at all.
///
/// `full_name` (the fixture's own `file#index`) is accepted but
/// currently unused by any rule below -- kept as a parameter (not
/// removed) because exact per-fixture-name allowlisting is the right
/// tool the moment a *specific*, individually-confirmed case needs
/// classifying (as opposed to a broad file- or content-based rule),
/// which has already happened once here and is likely to again: an
/// earlier version of this function allowlisted several `webkit02.dat`/
/// `tests1.dat` cases as "confirmed stale against the current spec"
/// (their `<select>` content model looked like a pre-2015 relic). That
/// conclusion was wrong -- checked against html5lib's Python reference
/// implementation, which is itself now stale: the WHATWG spec's
/// "Customizable Select" feature (whatwg/html#10548, already shipped in
/// Chromium and Gecko) removed the dedicated "in select"/"in select in
/// table" insertion modes entirely in mid-2025, folding `<select>`'s
/// content model into ordinary "in body" processing. Those fixtures'
/// original expectations were right all along; BlueIce's tree builder
/// (`step_in_body`'s `"select"`/`"option"`/`"optgroup"`/`"hr"`/`"input"`
/// arms) now implements the current algorithm directly instead of a
/// separate mode, confirmed against the merged spec PR's actual text
/// and its own test-suite update (html5lib/html5lib-tests#178) rather
/// than the outdated reference implementation.
fn likely_out_of_scope_reason(full_name: &str, file: &str, data: &str, expected_raw: &str) -> Option<&'static str> {
    let _ = full_name;
    if file.starts_with("scripted_") || file == "noscript01.dat" {
        return Some("scripting (needs real JS execution or a scripting-disabled parsing mode)");
    }
    if file == "template.dat" || data.contains("<template") {
        return Some("<template> element (not in MVP HTML scope)");
    }
    if file == "namespace-sensitivity.dat" || expected_raw.contains("<svg ") || expected_raw.contains("<math ") || data.contains("<svg") || data.contains("<math") {
        return Some("SVG/MathML foreign content (explicit MVP non-goal)");
    }
    if file == "quirks01.dat" {
        return Some("quirks-mode doctype handling (no quirks-mode concept in MVP)");
    }
    if file == "processing-instructions.dat" || data.contains("<?") {
        return Some("<?...?> as a distinct ProcessingInstruction node (no such node kind in blueice_dom at all -- a newer/optional spec proposal, not classic bogus-comment handling)");
    }
    if mentions_element_outside_mvp_scope(data) {
        return Some("mentions an element outside the Phase 2 MVP HTML element list");
    }
    // Anything beyond the minimal named-character-reference set
    // (`&amp; &lt; &gt; &quot; &apos; &nbsp;` plus numeric/hex refs) is
    // an explicit Phase 2 HTML-scope cut, not a bug.
    let known = ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;", "&nbsp;"];
    let stripped = known.iter().fold(data.to_string(), |acc, k| acc.replace(k, ""));
    let has_other_named_ref = stripped.match_indices('&').any(|(i, _)| {
        let after = &stripped[i + 1..];
        !after.starts_with('#') && after.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    });
    if has_other_named_ref {
        return Some("named character reference outside the minimal supported set");
    }
    None
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../development/browser_core/reference/wpt/html/syntax/parsing/resources")
}

#[test]
fn wpt_tree_construction_corpus() {
    let dir = corpus_dir();
    if !dir.exists() {
        eprintln!("skipping: WPT corpus not checked out at {dir:?} -- see reference/README.md");
        return;
    }

    let fixtures = load_fixtures(&dir);
    assert!(!fixtures.is_empty(), "corpus directory exists but no .dat files were found in it");

    use std::collections::BTreeMap;
    let mut per_file: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // file -> (pass, fail)
    let mut skipped_fragment = 0usize;
    let mut skipped_no_document = 0usize;
    let mut unclassified: Vec<(String, String, String)> = Vec::new(); // (name, expected, actual) -- worth a human reading
    let mut classified_counts: BTreeMap<&'static str, usize> = BTreeMap::new();

    for fixture in &fixtures {
        if fixture.section("document-fragment").is_some() {
            skipped_fragment += 1;
            continue;
        }
        let Some(expected_raw) = fixture.document() else {
            skipped_no_document += 1;
            continue;
        };
        let file = fixture.name.split('#').next().unwrap_or(&fixture.name).to_string();

        let doc = blueice_html::parse(fixture.data());
        let actual_raw = blueice_dom::dump(&doc);
        let actual_raw = actual_raw.strip_suffix('\n').unwrap_or(&actual_raw);
        let expected = strip_unsupported_lines(expected_raw);
        let actual = strip_unsupported_lines(actual_raw);

        let entry = per_file.entry(file.clone()).or_insert((0, 0));
        if actual == expected {
            entry.0 += 1;
        } else {
            entry.1 += 1;
            match likely_out_of_scope_reason(&fixture.name, &file, fixture.data(), expected_raw) {
                Some(reason) => *classified_counts.entry(reason).or_insert(0) += 1,
                None => unclassified.push((fixture.name.clone(), expected, actual)),
            }
        }
    }

    let total_passed: usize = per_file.values().map(|(p, _)| p).sum();
    let total: usize = per_file.values().map(|(p, f)| p + f).sum();
    println!();
    println!("=== WPT tree-construction corpus ===");
    println!("{total_passed}/{total} passed ({:.1}%)", 100.0 * total_passed as f64 / total.max(1) as f64);
    println!("skipped: {skipped_fragment} fragment-context cases (innerHTML-style parsing, out of MVP scope), {skipped_no_document} with no #document section");
    println!();
    println!("per file (pass/total):");
    for (file, (pass, fail)) in &per_file {
        let file_total = pass + fail;
        println!("  {file:<45} {pass:>4}/{file_total:<4} ({:.0}%)", 100.0 * *pass as f64 / (file_total.max(1)) as f64);
    }

    println!();
    println!("failures classified as likely already-known MVP scope cuts:");
    for (reason, count) in &classified_counts {
        println!("  {count:>4}  {reason}");
    }
    println!();
    println!("=== {} UNCLASSIFIED failures (not an obvious scope cut -- worth reading) ===", unclassified.len());
    for (name, expected, actual) in &unclassified {
        println!("--- {name} ---\nexpected:\n{expected}\nactual:\n{actual}\n");
    }

    // The actual gate. Two separate checks, since `likely_out_of_scope_reason`'s
    // classification is a heuristic (see its own doc comment) that could in
    // principle bucket a genuine new regression into an existing "known
    // scope cut" reason without anyone noticing -- `total_passed` catches
    // that class of miss even when `unclassified` stays empty.
    assert!(unclassified.is_empty(), "{} unclassified WPT tree-construction failure(s) -- see the printed diffs above; either fix the bug or extend `likely_out_of_scope_reason` with a specific, justified reason", unclassified.len());
    assert!(
        total_passed >= BASELINE_PASSED,
        "WPT tree-construction pass count regressed: {total_passed} passed vs. a baseline of {BASELINE_PASSED} -- a real regression that `likely_out_of_scope_reason` happened to bucket away rather than leave unclassified (check the per-file breakdown above for which file's pass count dropped). If this is instead a deliberate, reviewed change (e.g. the corpus itself was re-fetched and legitimately shifted), update `BASELINE_PASSED` to the new value in the same change."
    );
}

/// The known-good WPT tree-construction pass count as of the last
/// deliberate review of this gate (`development/browser_core/testing/TEST_PLAN.md`'s
/// "WPT tree-construction corpus" section has the full history) --
/// `wpt_tree_construction_corpus`'s own second assertion treats a drop
/// below this as a build-breaking regression. Bump this only alongside
/// a change that's actually supposed to move the number (a real bug fix
/// that raises it, or a deliberate corpus re-fetch) -- never just to
/// silence a failing assertion.
const BASELINE_PASSED: usize = 1022;

#[cfg(test)]
mod tests {
    use super::strip_unsupported_lines;

    #[test]
    fn drops_a_single_line_comment() {
        let dump = "| <html>\n|   <body>\n|     \"FOO\"\n|     <!--  BAR  -->\n|     \"BAZ\"";
        assert_eq!(strip_unsupported_lines(dump), "| <html>\n|   <body>\n|     \"FOO\"\n|     \"BAZ\"");
    }

    #[test]
    fn drops_every_line_of_a_comment_whose_own_content_spans_a_literal_newline() {
        // `comments01.dat`'s `<!-- BAR --!\n>BAZ -->` case: the
        // comment's content contains a real newline, so its dump spans
        // two raw lines with no per-line marker on the second one --
        // both must be dropped, not just the first.
        let dump = "| <html>\n|   <body>\n|     \"FOO\"\n|     <!--  BAR --!\n>BAZ -->";
        assert_eq!(strip_unsupported_lines(dump), "| <html>\n|   <body>\n|     \"FOO\"");
    }

    #[test]
    fn drops_a_doctype_line() {
        let dump = "| <!DOCTYPE html>\n| <html>\n|   <body>";
        assert_eq!(strip_unsupported_lines(dump), "| <html>\n|   <body>");
    }
}
