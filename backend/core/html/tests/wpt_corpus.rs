// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scratch runner (reports, doesn't gate the build) against the full
//! WPT/html5lib-tests tree-construction corpus
//! (`development/browser_core/reference/wpt/`, see that directory's
//! `README.md` to fetch it) -- covers real HTML5 compatibility, not
//! just the handful of hand-picked cases already in
//! `../../../development/browser_core/testing/fixtures/`. Reuses
//! `blueice_testing`'s fixture parser unmodified -- the corpus's own
//! `#data`/`#errors`/`#document`/`#document-fragment` format is
//! exactly what that parser already reads.
//!
//! **Deliberately not a hard pass/fail gate.** A byte-exact dump
//! comparison against the raw corpus starts at ~19% pass, almost
//! entirely because of already-known, deliberate MVP scope cuts
//! (`blueice_dom` never materializes Comment/Doctype nodes at all,
//! SVG/MathML foreign content, `<template>`, the full named-character-
//! reference table, ...) rather than bugs -- [`strip_unsupported_lines`]
//! and [`likely_out_of_scope_reason`] exist to separate that expected
//! noise from genuine failures worth a human reading, not to make the
//! number look better. Over thirty real bugs were found and fixed this
//! way across three triage passes (see `testing/TEST_PLAN.md`'s "WPT
//! tree-construction corpus" section for the full list); the pass rate
//! after normalizing away known scope cuts moved 39.0% -> 56.4% as a
//! direct result, with unclassified failures (the ones actually worth
//! reading) dropping from 669 to 21. Converting this into an actual CI
//! gate (with a maintained skip-list) is future work, not done here.

use blueice_testing::load_fixtures;
use std::path::PathBuf;

/// Drops any dump line representing a `<!DOCTYPE>` or `<!--comment-->`
/// node, at any depth -- `blueice_dom::dump` never emits these
/// (comments/doctypes aren't materialized as nodes at all, a
/// deliberate scope decision predating this corpus run, not something
/// this comparison should count as a failure). Applied to both sides
/// so the comparison is fair to what BlueIce actually claims to
/// support, rather than penalizing it for a documented non-goal.
fn strip_unsupported_lines(dump: &str) -> String {
    dump.lines()
        .filter(|line| {
            let content = line.strip_prefix("| ").unwrap_or(line).trim_start();
            !(content.starts_with("<!--") || content.starts_with("<!DOCTYPE"))
        })
        .collect::<Vec<_>>()
        .join("\n")
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
fn likely_out_of_scope_reason(file: &str, data: &str, expected_raw: &str) -> Option<&'static str> {
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
            match likely_out_of_scope_reason(&file, fixture.data(), expected_raw) {
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
}
