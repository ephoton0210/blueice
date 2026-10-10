// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{BuildArtifact, MapLoader, ModuleKind, SourceMap};
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
fn parses_the_class_field_semantics_flag_and_rejects_other_values() {
    for (value, expected) in [("true", Some(true)), ("false", Some(false))] {
        let parsed = args(&["check", "main.ts", "--use-define-for-class-fields", value]).unwrap();
        let Input::Entry { options, .. } = parsed.input else {
            panic!("expected explicit entry input");
        };
        assert_eq!(options.use_define_for_class_fields, expected);
    }
    let Input::Entry { options, .. } = args(&["check", "main.ts"]).unwrap().input else {
        panic!("expected explicit entry input");
    };
    assert_eq!(
        options.use_define_for_class_fields, None,
        "the target decides by default"
    );
    assert_eq!(
        args(&["check", "main.ts", "--use-define-for-class-fields", "maybe"]),
        Err(
            "unsupported --use-define-for-class-fields value `maybe`; expected true or false"
                .to_string()
        )
    );
    assert_eq!(
        args(&["check", "main.ts", "--use-define-for-class-fields"]),
        Err("--use-define-for-class-fields requires a value".to_string())
    );
}

#[test]
fn parses_the_const_enum_flags() {
    let Input::Entry { options, .. } = args(&["check", "main.ts"]).unwrap().input else {
        panic!("expected explicit entry input");
    };
    assert!(!options.preserve_const_enums && !options.isolated_modules);
    let Input::Entry { options, .. } = args(&[
        "check",
        "main.ts",
        "--preserve-const-enums",
        "--isolated-modules",
    ])
    .unwrap()
    .input
    else {
        panic!("expected explicit entry input");
    };
    assert!(options.preserve_const_enums && options.isolated_modules);
    assert!(!options.inlines_const_enums());
}

#[test]
fn parses_the_module_system_flags() {
    let Input::Entry { options, .. } = args(&["check", "main.ts"]).unwrap().input else {
        panic!("expected explicit entry input");
    };
    assert_eq!(options.module_kind, ModuleKind::Esm);
    assert!(!options.es_module_interop);
    let Input::Entry { options, .. } = args(&[
        "check",
        "main.ts",
        "--module",
        "commonjs",
        "--es-module-interop",
    ])
    .unwrap()
    .input
    else {
        panic!("expected explicit entry input");
    };
    assert_eq!(options.module_kind, ModuleKind::CommonJs);
    assert!(options.es_module_interop);
    assert!(args(&["check", "main.ts", "--module", "amd"]).is_err());
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
    let metadata = build_metadata(
        &invocation,
        &CompileSummary {
            artifacts: BTreeMap::new(),
            declaration_modules: BTreeMap::new(),
            assets: BTreeMap::new(),
            fingerprint: "test".to_string(),
            module_count: 1,
            has_errors: false,
        },
        None,
    );
    let serialized = serde_json::to_value(metadata).unwrap();
    assert_eq!(
        serialized["strictBoundaries"][0]["contractId"],
        "echo-string-v1"
    );
    assert_eq!(serialized["strictBoundaries"][0]["module"], "main.ts");
    assert_eq!(serialized["strictBoundaries"][0]["sourceStart"], 0);
    assert_eq!(serialized["strictBoundaries"][0]["sourceEnd"], 61);
    assert_eq!(serialized["strictBoundaries"][0]["maxStringBytes"], 64);
    assert_eq!(
        serialized["strictBoundaries"][0]["helperVersion"],
        RUNTIME_HELPER_V1_VERSION
    );

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
        downlevel_iteration: false,
        root_dir: None,
        standard_library: blueice_bluets::standard_library::identity(EcmaTarget::Es2022),
        language_version: "blue-ts-test",
        fingerprint: "bts-project-test".to_string(),
        target: "es2022",
        use_define_for_class_fields: true,
        preserve_const_enums: false,
        inline_const_enums: true,
        module: "esm",
        es_module_interop: false,
        jsx: None,
        jsx_factory: None,
        jsx_fragment_factory: None,
        jsx_import_source: None,
        package_resolution: None,
        remote_declarations: Vec::new(),
        class_helper_version: "bluets-class-helper-v1",
        target_helper_version: blueice_bluets::TARGET_HELPER_V1_VERSION,
        decorator_helper_version: "bluets-decorator-helper-v1",
        legacy_decorator_helper_version: "bluets-legacy-decorator-helper-v1",
        experimental_decorators: false,
        emit_decorator_metadata: false,
        runtime_policy: "checked",
        runtime_helper: None,
        strict_boundaries: Vec::new(),
        strict_artifacts: Vec::new(),
        source_map: true,
        declaration: true,
        entries: vec!["src/main.js".to_string()],
        declaration_modules: Vec::new(),
        imports: BTreeMap::new(),
        has_configured_imports: false,
    }
}

fn strict_fixture(root: &Path) -> (BTreeMap<String, BuildArtifact>, BuildMetadata) {
    let source = "export function echo(value: string): string { return value; }";
    fs::write(root.join("main.ts"), source).unwrap();
    let boundary = StrictRuntimeBoundary {
        contract_id: "echo-v1".to_string(),
        function: "echo".to_string(),
        span: SourceSpan::new("main.ts", 0, source.len()),
        max_string_bytes: 64,
        helper_version: RUNTIME_HELPER_V1_VERSION.to_string(),
    };
    let compiled = compile(
        "main.ts",
        &MapLoader::from([ModuleSource::new("main.ts", source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            source_map: true,
            declaration: true,
            strict_runtime_boundaries: vec![boundary.clone()],
            ..CompilerOptions::default()
        },
    );
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let artifacts = compiled.output.unwrap().artifacts;
    let mut metadata = test_metadata();
    metadata.language_version = blueice_bluets::LANGUAGE_VERSION;
    metadata.runtime_policy = RuntimePolicy::StrictRuntime.as_str();
    metadata.runtime_helper = Some(runtime_helper_v1_identity());
    metadata.strict_boundaries = vec![StrictBoundaryManifest::from(&boundary)];
    metadata.strict_artifacts = strict_artifact_inventory(&artifacts);
    metadata.entries = vec!["main.js".to_string()];
    metadata.fingerprint = fingerprint_entries(&[artifacts["main.ts"].fingerprint.clone()]);
    (artifacts, metadata)
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
    assert!(!manifest.contains("\"strictBoundaries\""));
    assert!(!manifest.contains("\"strictArtifacts\""));
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
    let (artifacts, mut metadata) = strict_fixture(&root);
    publish_build(
        &root,
        &output,
        &artifacts,
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
    assert_eq!(manifest["strictBoundaries"][0]["contractId"], "echo-v1");
    assert_eq!(manifest["strictArtifacts"][0]["module"], "main.ts");
    assert_eq!(
        manifest["strictArtifacts"][0]["emittedJavascriptSha256"],
        sha256_label(artifacts["main.ts"].javascript.as_bytes())
    );

    metadata.runtime_helper.as_mut().unwrap().sha256 = "sha256:wrong".into();
    assert!(publish_build(
        &root,
        &output,
        &artifacts,
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
        &artifacts,
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
        &BTreeMap::new(),
        &metadata
    )
    .is_err());
    metadata.runtime_helper = None;
    assert!(publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata
    )
    .is_err());
    metadata.strict_boundaries.clear();
    metadata.strict_artifacts.clear();
    publish_build(
        &root,
        &output,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata,
    )
    .unwrap();
    assert!(!output.join(RUNTIME_HELPER_V1_FILE).exists());
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn strict_publisher_rejects_manifest_and_call_tampering_before_replacement() {
    let temporary = unique_test_directory("strict-publish-audit");
    let root = temporary.join("source");
    let output = temporary.join("output");
    fs::create_dir_all(&root).unwrap();
    let (artifacts, metadata) = strict_fixture(&root);
    publish_build(
        &root,
        &output,
        &artifacts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &metadata,
    )
    .unwrap();
    let original_js = fs::read(output.join("main.js")).unwrap();
    let original_manifest = fs::read(output.join("bluetsc.manifest.json")).unwrap();
    let assert_refused =
        |candidate_metadata: &BuildMetadata,
         candidate_artifacts: &BTreeMap<String, BuildArtifact>| {
            let error = publish_build(
                &root,
                &output,
                candidate_artifacts,
                &BTreeMap::new(),
                &BTreeMap::new(),
                candidate_metadata,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(fs::read(output.join("main.js")).unwrap(), original_js);
            assert_eq!(
                fs::read(output.join("bluetsc.manifest.json")).unwrap(),
                original_manifest
            );
        };

    let mut wrong_budget = metadata.clone();
    wrong_budget.strict_boundaries[0].max_string_bytes -= 1;
    assert_refused(&wrong_budget, &artifacts);
    let mut wrong_digest = metadata.clone();
    wrong_digest.strict_artifacts[0].emitted_javascript_sha256 = "sha256:wrong".to_string();
    assert_refused(&wrong_digest, &artifacts);
    let mut wrong_fingerprint = metadata.clone();
    wrong_fingerprint.fingerprint = "bts-tampered".to_string();
    assert_refused(&wrong_fingerprint, &artifacts);
    let mut wrong_target = metadata.clone();
    wrong_target.target = "es2018";
    assert_refused(&wrong_target, &artifacts);
    let mut wrong_language = metadata.clone();
    wrong_language.language_version = "blue-ts-old";
    assert_refused(&wrong_language, &artifacts);
    let mut wrong_declarations = metadata.clone();
    wrong_declarations.declaration_modules = vec!["missing.d.ts".to_string()];
    assert_refused(&wrong_declarations, &artifacts);

    let mut wrong_import = artifacts.clone();
    let changed_import =
        wrong_import["main.ts"]
            .javascript
            .replacen("import {", "import /* changed */ {", 1);
    wrong_import.get_mut("main.ts").unwrap().javascript = changed_import;
    assert_refused(&metadata, &wrong_import);
    let mut missing_call = artifacts.clone();
    let changed_call = missing_call["main.ts"].javascript.replacen(
        "__bluetsValidateStringV1(",
        "__bluetsOther(",
        1,
    );
    missing_call.get_mut("main.ts").unwrap().javascript = changed_call;
    assert_refused(&metadata, &missing_call);
    let mut extra_runtime = artifacts.clone();
    extra_runtime
        .get_mut("main.ts")
        .unwrap()
        .javascript
        .push_str("\nglobalText();\n");
    assert_refused(&metadata, &extra_runtime);
    let mut changed_map = artifacts.clone();
    changed_map
        .get_mut("main.ts")
        .unwrap()
        .source_map
        .as_mut()
        .unwrap()
        .mappings
        .push_str("AAAA");
    assert_refused(&metadata, &changed_map);
    let mut missing_map = artifacts.clone();
    missing_map.get_mut("main.ts").unwrap().source_map = None;
    assert_refused(&metadata, &missing_map);
    let mut missing_record = artifacts.clone();
    missing_record.get_mut("main.ts").unwrap().strict_runtime = None;
    assert_refused(&metadata, &missing_record);

    let mut checked_metadata = metadata.clone();
    checked_metadata.runtime_policy = RuntimePolicy::Checked.as_str();
    checked_metadata.runtime_helper = None;
    checked_metadata.strict_boundaries.clear();
    checked_metadata.strict_artifacts.clear();
    assert_refused(&checked_metadata, &artifacts);

    let source = fs::read_to_string(root.join("main.ts")).unwrap();
    for policy in [RuntimePolicy::Checked, RuntimePolicy::TranspileOnly] {
        let weak = compile(
            "main.ts",
            &MapLoader::from([ModuleSource::new("main.ts", &source)]),
            CompilerOptions {
                runtime_policy: policy,
                source_map: true,
                declaration: true,
                ..CompilerOptions::default()
            },
        );
        assert!(!weak.has_errors(), "{:?}", weak.diagnostics);
        let weak_artifacts = weak.output.unwrap().artifacts;
        assert!(weak_artifacts["main.ts"].strict_runtime.is_none());
        let mut forged = metadata.clone();
        forged.fingerprint = fingerprint_entries(&[weak_artifacts["main.ts"].fingerprint.clone()]);
        forged.strict_artifacts = strict_artifact_inventory(&weak_artifacts);
        assert_refused(&forged, &weak_artifacts);
    }
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
