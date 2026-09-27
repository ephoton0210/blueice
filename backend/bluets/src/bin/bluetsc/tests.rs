// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{BuildArtifact, SourceMap};
use std::collections::BTreeMap;

fn args(values: &[&str]) -> Result<Args, String> {
    parse_args(values.iter().map(|value| value.to_string()))
}

#[test]
fn build_requires_an_output_directory() {
    assert_eq!(
        args(&["build", "main.ts"]),
        Err("build requires --out-dir <directory>".to_string())
    );
}

#[test]
fn parses_check_with_a_strict_policy() {
    let parsed = args(&[
        "check",
        "main.ts",
        "--runtime-policy",
        "strict-runtime",
        "--target",
        "es2020",
    ])
    .unwrap();
    assert_eq!(parsed.command, Command::Check);
    let Input::Entry { options, .. } = parsed.input else {
        panic!("expected explicit entry input");
    };
    assert_eq!(options.runtime_policy, RuntimePolicy::StrictRuntime);
    assert_eq!(options.target, EcmaTarget::Es2020);
}

#[test]
fn config_mode_has_no_flag_escape_hatch() {
    assert_eq!(
        args(&["check", "--config", "bluetsc.json", "--source-map"]),
        Err(
            "`--config` owns project settings; unsupported extra argument `--source-map`"
                .to_string()
        )
    );
}

#[test]
fn configured_output_cannot_escape_its_project_root() {
    assert!(configured_output_path(Path::new("/project"), "../outside").is_err());
    assert!(configured_output_path(Path::new("/project"), ".").is_err());
    assert!(configured_output_path(Path::new("/project"), "").is_err());
    assert_eq!(
        configured_output_path(Path::new("/project"), "dist").unwrap(),
        PathBuf::from("/project/dist")
    );
}

#[test]
fn declaration_modules_cannot_be_executable_entries() {
    assert!(ensure_not_declaration_entry(Path::new("types/api.d.ts"), "entry").is_err());
    assert!(ensure_not_declaration_entry(Path::new("src/main.ts"), "entry").is_ok());
}

#[test]
fn resolver_fingerprint_is_project_root_relative() {
    let first_root = Path::new("/first/project");
    let second_root = Path::new("/second/project");
    let first = BTreeMap::from([("@shared/".to_string(), first_root.join("src/shared"))]);
    let second = BTreeMap::from([("@shared/".to_string(), second_root.join("src/shared"))]);
    assert_eq!(
        import_map_fingerprint(first_root, &first),
        import_map_fingerprint(second_root, &second)
    );
}

#[test]
fn artifact_paths_accept_root_relative_ids_but_reject_parent_traversal() {
    assert_eq!(
        artifact_relative_path(
            Path::new("/project"),
            Path::new("src/main.ts"),
            "src/main.ts"
        )
        .unwrap(),
        PathBuf::from("src/main.ts")
    );
    assert!(artifact_relative_path(
        Path::new("/project"),
        Path::new("../outside.ts"),
        "../outside.ts"
    )
    .is_err());
}

#[test]
fn project_module_ids_are_root_relative() {
    assert_eq!(
        project_module_id(Path::new("/project"), Path::new("/project/src/main.ts")),
        "src/main.ts"
    );
}

#[test]
fn configured_strict_boundary_is_confined_and_keeps_exact_source_span() {
    let temporary = unique_test_directory("strict-boundary-config");
    let module = temporary.join("main.ts");
    fs::write(
        &module,
        "export function echo(value: string): string { return value; }",
    )
    .unwrap();
    let config = temporary.join("bluetsc.json");
    fs::write(
        &config,
        r#"{"entries":["main.ts"],"runtimePolicy":"strict-runtime","strictBoundaries":[{"contractId":"echo-string-v1","module":"main.ts","function":"echo","sourceStart":0,"sourceEnd":61,"maxStringBytes":64,"helperVersion":"bluets-runtime-helper-v1"}]}"#,
    )
    .unwrap();
    let invocation = resolve_config_invocation(config).unwrap();
    let boundary = &invocation.options.strict_runtime_boundaries[0];
    assert_eq!(boundary.span.module, "main.ts");
    assert_eq!(boundary.span.start, 0);
    assert_eq!(boundary.span.end, 61);
    assert_eq!(boundary.max_string_bytes, 64);

    let invalid = StrictBoundaryConfig {
        contract_id: "x".to_string(),
        module: "../outside.ts".to_string(),
        function: "echo".to_string(),
        source_start: 0,
        source_end: 1,
        max_string_bytes: 1,
        helper_version: RUNTIME_HELPER_V1_VERSION.to_string(),
    };
    assert!(configured_strict_boundary(&temporary, invalid).is_err());
    fs::remove_dir_all(temporary).unwrap();
}

fn test_metadata() -> BuildMetadata {
    BuildMetadata {
        language_version: "blue-ts-test",
        fingerprint: "bts-project-test".to_string(),
        target: "es2022",
        runtime_policy: "checked",
        runtime_helper: None,
        source_map: true,
        declaration: true,
        entries: vec!["src/main.js".to_string()],
        declaration_modules: Vec::new(),
        imports: BTreeMap::new(),
        has_configured_imports: false,
    }
}

#[test]
fn publishing_replaces_a_complete_output_directory_only_after_staging() {
    let temporary = unique_test_directory("publish");
    let root = temporary.join("source");
    let output = temporary.join("output");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("obsolete.js"), "old output").unwrap();
    let module = root.join("src/main.ts");
    let artifacts = BTreeMap::from([(
        path_id(&module),
        BuildArtifact {
            module_id: path_id(&module),
            javascript: "export const answer = 42;".to_string(),
            source_map: Some(SourceMap {
                file: "main.js".to_string(),
                sources: vec!["src/main.ts".to_string()],
                sources_content: vec!["export const answer: number = 42;".to_string()],
                mappings: "AAAA".to_string(),
            }),
            declaration: Some("export declare const answer: number;\n".to_string()),
            fingerprint: "test".to_string(),
            strict_runtime: None,
        },
    )]);

    publish_build(
        &root,
        &output,
        &artifacts,
        &BTreeMap::new(),
        &test_metadata(),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(output.join("src/main.js")).unwrap(),
        "export const answer = 42;\n//# sourceMappingURL=main.js.map\n"
    );
    assert!(output.join("src/main.js.map").is_file());
    assert!(output.join("src/main.d.ts").is_file());
    let manifest = fs::read_to_string(output.join("bluetsc.manifest.json")).unwrap();
    assert!(manifest.contains("\"languageVersion\": \"blue-ts-test\""));
    assert!(!manifest.contains(&root.to_string_lossy().into_owned()));
    assert!(!output.join("obsolete.js").exists());
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn strict_publisher_stages_the_exact_versioned_helper_and_releases_it_on_replacement() {
    let temporary = unique_test_directory("runtime-helper-v1");
    let root = temporary.join("source");
    let output = temporary.join("output");
    fs::create_dir_all(&root).unwrap();
    let mut metadata = test_metadata();
    metadata.runtime_policy = RuntimePolicy::StrictRuntime.as_str();
    metadata.runtime_helper = Some(runtime_helper_v1_identity());
    publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(output.join(RUNTIME_HELPER_V1_FILE)).unwrap(),
        RUNTIME_HELPER_V1_SOURCE
    );
    assert!(RUNTIME_HELPER_V1_SOURCE.starts_with("// This Source Code Form"));
    assert!(RUNTIME_HELPER_V1_SOURCE.contains("bluets-runtime-helper-v1"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("bluetsc.manifest.json")).unwrap()).unwrap();
    let identity = runtime_helper_v1_identity();
    assert_eq!(manifest["runtimeHelper"]["version"], identity.version);
    assert_eq!(manifest["runtimeHelper"]["file"], identity.file);
    assert_eq!(manifest["runtimeHelper"]["sha256"], identity.sha256);

    metadata.runtime_helper.as_mut().unwrap().sha256 = "sha256:wrong".into();
    assert!(publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata
    )
    .is_err());
    assert_eq!(
        fs::read_to_string(output.join(RUNTIME_HELPER_V1_FILE)).unwrap(),
        RUNTIME_HELPER_V1_SOURCE
    );
    metadata.runtime_helper = None;
    assert!(publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata
    )
    .is_err());

    metadata.runtime_policy = RuntimePolicy::Checked.as_str();
    metadata.runtime_helper = Some(runtime_helper_v1_identity());
    assert!(publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata
    )
    .is_err());
    metadata.runtime_helper = None;
    publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata,
    )
    .unwrap();
    assert!(!output.join(RUNTIME_HELPER_V1_FILE).exists());
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn staging_failure_does_not_replace_an_existing_output_directory() {
    let temporary = unique_test_directory("publish-failure");
    let root = temporary.join("source");
    let output = temporary.join("output");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("preserved.js"), "previous artifact").unwrap();
    let artifacts = BTreeMap::from([(
        path_id(&temporary.join("outside.ts")),
        BuildArtifact {
            module_id: path_id(&temporary.join("outside.ts")),
            javascript: String::new(),
            source_map: None,
            declaration: None,
            fingerprint: "test".to_string(),
            strict_runtime: None,
        },
    )]);

    assert!(publish_build(
        &root,
        &output,
        &artifacts,
        &BTreeMap::new(),
        &test_metadata(),
    )
    .is_err());
    assert_eq!(
        fs::read_to_string(output.join("preserved.js")).unwrap(),
        "previous artifact"
    );
    fs::remove_dir_all(temporary).unwrap();
}

fn unique_test_directory(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "blueice-bluets-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    path
}
