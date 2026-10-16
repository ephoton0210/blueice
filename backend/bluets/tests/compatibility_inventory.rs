// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source-derived compatibility inventory tooling (K.14.1).

#[path = "fixtures/compatibility_inventory/inventory.rs"]
mod inventory;
#[path = "fixtures/compatibility_inventory/rust_facts.rs"]
mod rust_facts;
#[path = "fixtures/compatibility_inventory/snapshot.rs"]
mod snapshot;

#[test]
fn rust_case_facts_preserve_scope_and_ignore_literal_attribute_text() {
    let source = include_str!("fixtures/compatibility_inventory/samples/harness.rs.txt");
    let facts = rust_facts::parse(source).unwrap();
    assert_eq!(facts.ignored_tests, ["pinned", "nested::second"]);
    assert_eq!(
        facts.includes,
        [
            "fixtures/first.ts",
            "fixtures/in_macro.ts",
            "fixtures/second.ts"
        ]
    );
    assert_eq!(facts.modules.len(), 1);
    assert_eq!(facts.modules[0].name, "external");
    assert_eq!(facts.modules[0].path.as_deref(), Some("support/custom.rs"));
    assert_eq!(facts.arrays.len(), 3);
    assert_eq!(facts.arrays[0].scope, "");
    assert_eq!(facts.arrays[0].binding, "CASES");
    assert_eq!(facts.arrays[0].labels, ["first", "second"]);
    assert_eq!(facts.arrays[1].scope, "pinned");
    assert_eq!(facts.arrays[1].binding, "cases");
    assert_eq!(facts.arrays[1].labels, ["quoted", "raw"]);
    assert_eq!(facts.arrays[2].scope, "pinned");
    assert_eq!(facts.arrays[2].binding, "for@0");
    assert_eq!(facts.arrays[2].labels, ["es2022", "commonjs"]);
}

#[test]
fn malformed_rust_is_an_inventory_error() {
    assert!(rust_facts::parse("fn broken( {").is_err());
}

#[test]
fn repository_paths_cannot_escape_or_use_host_specific_roots() {
    assert_eq!(
        snapshot::relative("backend/bluets/tests", "../../bluets-bluejs/tests/a.rs").unwrap(),
        "backend/bluets-bluejs/tests/a.rs"
    );
    for path in [
        "../../../../private.ts",
        "/tmp/private.ts",
        "C:/private.ts",
        "..\\private.ts",
    ] {
        assert!(
            snapshot::relative("backend/bluets/tests", path).is_err(),
            "{path}"
        );
    }
}

#[test]
fn generated_section_is_idempotent_and_rejects_broken_markers() {
    let replacement = format!("{}\ngenerated\n{}", inventory::START, inventory::END);
    let original =
        "intro\n## 1. What is compared with `tsc`, and how\nold\n## 2. Subset\nunchanged";
    let once = inventory::replace_section(original, &replacement).unwrap();
    assert!(once.starts_with("intro\n"));
    assert!(once.ends_with("## 2. Subset\nunchanged"));
    assert!(!once.contains("old"));
    assert_eq!(
        inventory::replace_section(&once, &replacement).unwrap(),
        once
    );
    for invalid in [
        inventory::START.to_string(),
        inventory::END.to_string(),
        format!("{replacement}{}", inventory::START),
        format!("{}{}", inventory::END, inventory::START),
    ] {
        assert!(inventory::replace_section(&invalid, &replacement).is_err());
    }
}

#[test]
fn recorded_suite_graph_matches_the_current_committed_inputs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let mut actual = inventory::generate(&tree).unwrap();
    let recorded: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(inventory::DIRECTORY).join(inventory::ARTIFACT))
            .unwrap(),
    )
    .unwrap();
    actual.source_revision = recorded["source_revision"].as_str().unwrap().to_string();
    assert_eq!(
        actual.source_graph_sha256,
        recorded["source_graph_sha256"].as_str().unwrap(),
        "committed source/fixture tree differs from the measured revision"
    );
    assert_eq!(serde_json::to_value(&actual).unwrap(), recorded);
    let document = std::fs::read_to_string(
        root.join(inventory::DIRECTORY)
            .join("COMPATIBILITY_INVENTORY.md"),
    )
    .unwrap();
    assert_eq!(
        inventory::replace_section(&document, &inventory::markdown(&actual)).unwrap(),
        document
    );
    let maps = actual
        .suites
        .iter()
        .find(|suite| suite.harness.ends_with("/source_map_tokens_oracle.rs"))
        .unwrap();
    for name in [
        "legacy_metadata::legacy_parameter_and_metadata_frames_match_native",
        "non_decorator::object_spread_helper_frames_match_native",
        "system_wrapper::system_wrapper_frames_match_native",
    ] {
        assert!(
            maps.ignored_tests.iter().any(|test| test == name),
            "missing child module {name}"
        );
    }
    assert!(!actual
        .suites
        .iter()
        .any(|suite| suite.harness.ends_with("/compatibility_inventory.rs")));
}

#[test]
fn git_batch_decoder_rejects_corruption_and_does_not_split_body_newlines() {
    let path = "source.rs".to_string();
    let blob = "0123456789abcdef".to_string();
    let selected = [(&path, &blob)];
    let good = b"0123456789abcdef blob 4\na\nb\n\n";
    assert_eq!(
        snapshot::decode_batch(good, &selected).unwrap()[&path],
        "a\nb\n"
    );
    for invalid in [
        b"0123456789abcdef blob 4".as_slice(),
        b"other blob 0\n\n",
        b"0123456789abcdef tree 0\n\n",
        b"0123456789abcdef blob no\n\n",
        b"0123456789abcdef blob 4\nabc",
        b"0123456789abcdef blob 4\nabcdX",
        b"0123456789abcdef blob 1\n\xff\n",
        b"0123456789abcdef blob 0\n\nextra",
    ] {
        assert!(
            snapshot::decode_batch(invalid, &selected).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn immutable_tree_resolves_nested_modules_and_refuses_missing_inputs() {
    let fixture = GitFixture::new();
    fixture.write(
        "backend/bluets/tests/oracle.rs",
        "mod nested { #[path=\"custom.rs\"] mod child; }",
    );
    fixture.write(
        "backend/bluets/tests/nested/custom.rs",
        "#[test] #[ignore] fn witness() { let _ = include_str!(\"input.ts\"); }",
    );
    fixture.write(
        "backend/bluets/tests/nested/input.ts",
        "export const value = 1;\n",
    );
    fixture.write(
        "backend/bluets/tests/fixtures/diagnostics/reference.json",
        r#"{"version":"5.9.3","templates":[],"cases":[]}"#,
    );
    let unicode = "backend/bluets/tests/fixtures/unicode-冰-😀/main.ts";
    fixture.write(unicode, "export const value = 1;\n");
    let production = "backend/bluets/src/compatibility_inventory.rs";
    fixture.write(production, "pub const INCLUDED: bool = true;\n");
    let revision = fixture.commit();
    let tree = snapshot::Snapshot::read(&fixture.0, &revision).unwrap();
    assert!(
        tree.blobs.contains_key(unicode),
        "Git paths must not retain display quoting"
    );
    assert!(
        tree.blobs.contains_key(production),
        "tooling exclusions must not hide production source"
    );
    let measured = inventory::generate(&tree).unwrap();
    assert_eq!(measured.suites.len(), 1);
    assert_eq!(measured.suites[0].ignored_tests, ["nested::child::witness"]);
    assert!(measured.suites[0]
        .inputs
        .contains_key("backend/bluets/tests/nested/input.ts"));
    // Later dirty edits cannot contaminate evidence for the frozen revision.
    fixture.write("backend/bluets/tests/nested/custom.rs", "not valid Rust");
    assert_eq!(
        inventory::generate(&snapshot::Snapshot::read(&fixture.0, &revision).unwrap())
            .unwrap()
            .source_graph_sha256,
        measured.source_graph_sha256
    );
    // Committed missing/escaping inputs fail closed rather than silently reducing counts.
    for input in ["missing.ts", "../../../../../../private.ts"] {
        fixture.write(
            "backend/bluets/tests/nested/custom.rs",
            &format!("#[test] #[ignore] fn witness() {{ let _ = include_str!({input:?}); }}"),
        );
        let revision = fixture.commit();
        assert!(
            inventory::generate(&snapshot::Snapshot::read(&fixture.0, &revision).unwrap()).is_err()
        );
    }
    fixture.write("backend/bluets/tests/oracle.rs", "mod absent;");
    let revision = fixture.commit();
    assert!(
        inventory::generate(&snapshot::Snapshot::read(&fixture.0, &revision).unwrap()).is_err()
    );
}

struct GitFixture(std::path::PathBuf);

impl GitFixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "bluets-k14-inventory-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self(directory);
        fixture.git(&["init", "--quiet"]);
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn git(&self, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(&self.0)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "Inventory fixture")
            .env("GIT_COMMITTER_NAME", "Inventory fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn commit(&self) -> String {
        self.git(&[
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.safecrlf=false",
            "add",
            ".",
        ]);
        let tree = self.git(&["write-tree"]);
        self.git(&[
            "-c",
            "commit.gpgsign=false",
            "commit-tree",
            &tree,
            "-m",
            "inventory fixture",
        ])
    }
}

impl Drop for GitFixture {
    fn drop(&mut self) {
        // Remove only the unique directory successfully created by this test.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
