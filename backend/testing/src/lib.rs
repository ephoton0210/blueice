// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared cross-stage test interface for BlueIce's rendering pipeline.
//!
//! Settled design (see `development/browser_core/testing/TEST_PLAN.md`,
//! "Rendering-correctness fixtures"): a single corpus of small HTML
//! fixtures under `development/browser_core/testing/fixtures/`, each one
//! a plain-text file with a `#data` section (the HTML input) plus one
//! named section per pipeline stage that can check something about it.
//! Only `#document` (a canonical DOM-tree dump) exists today, matching
//! `blueice-html` being the only real stage past `blueice-dom` -- `css`
//! adds `#styles`, `layout` adds `#layout`, `paint` adds `#paint`, and
//! Phase 5 adds an `#ai-representation` section for the accessibility
//! -tree-shaped output plan §3 settled on, all *without* changing this
//! parser or any earlier stage's own section. This is the concrete
//! answer to needing one stable interface instead of one refactored
//! per phase: sections are looked up by name, so a section a given
//! consumer doesn't know about is silently invisible to it, never a
//! breaking change.
//!
//! The dump format (`#document`) is a deliberate adaptation of the
//! html5lib-tests tree-construction format -- a `| `-prefixed,
//! 2-space-per-depth indented listing, elements as `<tag>`, attributes
//! as their own sorted, one-level-deeper lines, text as `"content"` --
//! reused rather than invented because it's already the de facto
//! standard this exact kind of test takes in every browser engine this
//! project reads as reference, and it's trivially diffable by a human.
//! Comments/doctypes never appear in the dump, matching `blueice-dom`
//! not materializing them as nodes (see `blueice_html`'s tree builder).

use blueice_dom::{Document, NodeData, NodeId};
use std::path::{Path, PathBuf};

/// Renders `doc` as a canonical, whitespace-exact tree dump in the
/// `#document` format described in the module docs. Two documents with
/// the same effective shape always produce byte-identical dumps
/// (attributes are sorted, so insertion order never leaks into the
/// comparison), which is what makes this usable as a fixture's expected
/// output rather than an ad hoc `Debug` dump.
pub fn dump_dom(doc: &Document) -> String {
    let mut out = String::new();
    for child in doc.children(doc.root()) {
        dump_node(doc, child, 0, &mut out);
    }
    out
}

fn dump_node(doc: &Document, id: NodeId, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    match doc.data(id) {
        NodeData::Document => unreachable!("the Document node is never its own child"),
        NodeData::Element { tag_name, attributes } => {
            out.push_str(&format!("| {indent}<{tag_name}>\n"));
            let mut attrs: Vec<_> = attributes.iter().collect();
            attrs.sort_by(|a, b| a.0.cmp(&b.0));
            let attr_indent = "  ".repeat(depth + 1);
            for (name, value) in attrs {
                out.push_str(&format!("| {attr_indent}{name}=\"{value}\"\n"));
            }
            for child in doc.children(id) {
                dump_node(doc, child, depth + 1, out);
            }
        }
        NodeData::Text { data } => {
            out.push_str(&format!("| {indent}\"{data}\"\n"));
        }
    }
}

/// One fixture parsed out of a `.dat` file: a name (for failure
/// messages) plus every named section found in source order. Sections
/// are looked up by name (see [`Fixture::section`]) rather than fixed
/// fields, so a future section type needs no change here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    pub name: String,
    sections: Vec<(String, String)>,
}

impl Fixture {
    pub fn section(&self, name: &str) -> Option<&str> {
        self.sections.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The `#data` section (the HTML input). Every fixture must have
    /// one; this is the one section every consumer, present or future,
    /// needs.
    pub fn data(&self) -> &str {
        self.section("data")
            .unwrap_or_else(|| panic!("fixture {:?} has no #data section", self.name))
    }

    /// The `#document` section (expected [`dump_dom`] output), if this
    /// fixture checks DOM shape.
    pub fn document(&self) -> Option<&str> {
        self.section("document")
    }
}

/// Parses every fixture out of one `.dat` file's contents. `source_name`
/// (typically the file's own name) is used only to build readable
/// fixture names for test-failure output.
pub fn parse_fixtures(source_name: &str, content: &str) -> Vec<Fixture> {
    let mut fixtures = Vec::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    let mut index = 0usize;

    fn flush_section(current: &mut Option<(String, Vec<&str>)>, sections: &mut Vec<(String, String)>) {
        if let Some((name, mut lines)) = current.take() {
            // A blank line separating two fixtures in the same file is a
            // separator, not trailing content of the section it happens
            // to land in while being collected -- strip it so a
            // fixture's #document doesn't gain a phantom trailing blank
            // depending on whether another fixture happens to follow it
            // in the same file.
            while lines.last() == Some(&"") {
                lines.pop();
            }
            sections.push((name, lines.join("\n")));
        }
    }

    fn flush_fixture(sections: &mut Vec<(String, String)>, fixtures: &mut Vec<Fixture>, source_name: &str, index: &mut usize) {
        if !sections.is_empty() {
            fixtures.push(Fixture {
                name: format!("{source_name}#{index}"),
                sections: std::mem::take(sections),
            });
            *index += 1;
        }
    }

    for line in content.lines() {
        if let Some(section_name) = line.strip_prefix('#') {
            if section_name == "data" {
                flush_section(&mut current, &mut sections);
                flush_fixture(&mut sections, &mut fixtures, source_name, &mut index);
            } else {
                flush_section(&mut current, &mut sections);
            }
            current = Some((section_name.to_string(), Vec::new()));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
        // lines before any "#" marker (e.g. stray blank separator lines
        // between fixtures) are silently skipped.
    }
    flush_section(&mut current, &mut sections);
    flush_fixture(&mut sections, &mut fixtures, source_name, &mut index);

    fixtures
}

/// Loads and parses every `.dat` file directly inside `dir` (not
/// recursive), sorted by filename for deterministic test ordering.
pub fn load_fixtures(dir: &Path) -> Vec<Fixture> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading fixture directory {dir:?}: {e}"))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "dat"))
        .collect();
    paths.sort();

    paths
        .into_iter()
        .flat_map(|path| {
            let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
            let source_name = path.file_name().unwrap().to_string_lossy().to_string();
            parse_fixtures(&source_name, &content)
        })
        .collect()
}

/// The shared fixture corpus's location, resolved relative to this
/// crate's own manifest directory so every caller gets the same answer
/// regardless of where *they* live in the workspace.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/testing/fixtures")
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_dom::NodeData;

    #[test]
    fn dump_dom_renders_nested_elements_and_text() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_node(NodeData::Element {
            tag_name: "p".to_string(),
            attributes: vec![],
        });
        doc.append_child(root, p);
        let text = doc.create_node(NodeData::Text { data: "hi".to_string() });
        doc.append_child(p, text);

        assert_eq!(dump_dom(&doc), "| <p>\n|   \"hi\"\n");
    }

    #[test]
    fn dump_dom_sorts_attributes_regardless_of_insertion_order() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = doc.create_node(NodeData::Element {
            tag_name: "div".to_string(),
            attributes: vec![("class".to_string(), "b".to_string()), ("id".to_string(), "a".to_string())],
        });
        doc.append_child(root, div);

        assert_eq!(dump_dom(&doc), "| <div>\n|   class=\"b\"\n|   id=\"a\"\n");
    }

    #[test]
    fn dump_dom_of_empty_document_is_empty_string() {
        let doc = Document::new();
        assert_eq!(dump_dom(&doc), "");
    }

    #[test]
    fn parse_single_fixture_with_data_and_document() {
        let fixtures = parse_fixtures(
            "t.dat",
            "#data\n<p>hi</p>\n#document\n| <p>\n|   \"hi\"\n",
        );
        assert_eq!(fixtures.len(), 1);
        assert_eq!(fixtures[0].name, "t.dat#0");
        assert_eq!(fixtures[0].data(), "<p>hi</p>");
        assert_eq!(fixtures[0].document(), Some("| <p>\n|   \"hi\""));
    }

    #[test]
    fn parse_multiple_fixtures_in_one_file() {
        let fixtures = parse_fixtures(
            "t.dat",
            "#data\n<p>a</p>\n#document\n| <p>\n|   \"a\"\n\n#data\n<p>b</p>\n#document\n| <p>\n|   \"b\"\n",
        );
        assert_eq!(fixtures.len(), 2);
        assert_eq!(fixtures[0].data(), "<p>a</p>");
        assert_eq!(fixtures[1].data(), "<p>b</p>");
        assert_eq!(fixtures[0].name, "t.dat#0");
        assert_eq!(fixtures[1].name, "t.dat#1");
    }

    #[test]
    fn trailing_blank_separator_line_is_not_part_of_the_section_content() {
        // regression test: a fixture followed by another one in the same
        // file (i.e. with a blank separator line after it) must produce
        // the exact same #document value as the same fixture written
        // last in a file with no trailing blank line.
        let alone = parse_fixtures("t.dat", "#data\n<p>hi</p>\n#document\n| <p>\n|   \"hi\"\n");
        let followed_by_another =
            parse_fixtures("t.dat", "#data\n<p>hi</p>\n#document\n| <p>\n|   \"hi\"\n\n#data\n<p>x</p>\n#document\n| <p>\n");
        assert_eq!(alone[0].document(), followed_by_another[0].document());
        assert_eq!(followed_by_another[0].document(), Some("| <p>\n|   \"hi\""));
    }

    #[test]
    fn parse_multiline_data_section() {
        let fixtures = parse_fixtures("t.dat", "#data\n<p>\nline two\n</p>\n#document\n| <p>\n");
        assert_eq!(fixtures[0].data(), "<p>\nline two\n</p>");
    }

    #[test]
    fn fixture_section_is_none_for_a_section_this_fixture_does_not_have() {
        let fixtures = parse_fixtures("t.dat", "#data\n<p>x</p>\n");
        assert_eq!(fixtures[0].document(), None);
    }

    #[test]
    fn unknown_future_section_is_preserved_and_ignored_by_existing_accessors() {
        // proves the "extend without refactoring" property: a section
        // future phases will add (#styles) round-trips through the
        // generic accessor without needing any change to this crate.
        let fixtures = parse_fixtures("t.dat", "#data\n<p>x</p>\n#styles\ncolor: red\n");
        assert_eq!(fixtures[0].section("styles"), Some("color: red"));
        assert_eq!(fixtures[0].document(), None);
    }

    #[test]
    #[should_panic(expected = "has no #data section")]
    fn data_accessor_panics_with_a_readable_message_if_missing() {
        let fixtures = parse_fixtures("t.dat", "#document\n| <p>\n");
        fixtures[0].data();
    }

    #[test]
    fn empty_file_yields_no_fixtures() {
        assert_eq!(parse_fixtures("empty.dat", ""), vec![]);
    }

    #[test]
    fn load_fixtures_reads_every_dat_file_in_a_directory_sorted() {
        let dir = std::env::temp_dir().join(format!("blueice-testing-fixtures-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.dat"), "#data\n<p>b</p>\n").unwrap();
        std::fs::write(dir.join("a.dat"), "#data\n<p>a</p>\n").unwrap();
        std::fs::write(dir.join("ignore.txt"), "not a fixture").unwrap();

        let fixtures = load_fixtures(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(fixtures.len(), 2, "the .txt file must be ignored");
        assert_eq!(fixtures[0].data(), "<p>a</p>", "a.dat sorts before b.dat");
        assert_eq!(fixtures[1].data(), "<p>b</p>");
    }

    #[test]
    fn fixtures_dir_points_at_the_shared_corpus() {
        let dir = fixtures_dir();
        assert!(dir.ends_with("development/browser_core/testing/fixtures"));
    }
}
