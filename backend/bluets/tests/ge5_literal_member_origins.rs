// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Capture the complete adjacent matrices before their independent Native role replay.
#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

#[test]
fn literal_member_helpers_keep_original_headers_and_logical_statement_ends() {
    observe(
        &[
            6070, 6071, 6102, 6103, 6134, 6135, 6166, 6167, 6286, 6287, 6310, 6311, 6342, 6343,
            6374, 6375,
        ],
        "verify_ge5_literal_member_origins.cjs",
    );
}

#[test]
fn lowered_accessor_pairs_keep_one_complete_native_descriptor() {
    observe(
        &[
            6118, 6119, 6120, 6121, 6122, 6123, 6124, 6125, 6150, 6151, 6152, 6153, 6154, 6155,
            6156, 6157, 8161, 8162, 8163,
        ],
        "verify_ge5_literal_accessor_pairs.cjs",
    );
}

#[test]
fn erased_legacy_accessor_decorators_keep_the_retained_modifier_source() {
    observe(
        &[6176, 6177, 6180, 6181],
        "verify_ge5_literal_accessor_modifiers.cjs",
    );
}

#[test]
fn retained_legacy_accessor_declarations_keep_their_complete_construction_chain() {
    observe(
        &[
            6162, 6163, 6164, 6165, 6170, 6171, 6172, 6173, 6178, 6179, 6180, 6181,
        ],
        "verify_ge5_literal_accessor_chain.cjs",
    );
}

#[test]
fn accessor_pair_grouping_keeps_identical_javascript_with_and_without_maps() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for index in [
        6118usize, 6119, 6120, 6121, 6122, 6123, 6124, 6125, 6150, 6151, 6152, 6153, 6154, 6155,
        6156, 6157,
    ] {
        let row = inputs::resolve(&inputs::input()["cases"][index]);
        let source = row["files"]["main.ts"].as_str().unwrap();
        let id = "memory:///main.ts";
        let options = CompilerOptions {
            target: EcmaTarget::Es5,
            module_kind: if row["options"]["module"] == "CommonJS" {
                ModuleKind::CommonJs
            } else {
                ModuleKind::Esm
            },
            source_map: true,
            inline_sources: true,
            experimental_decorators: row["options"]["experimentalDecorators"].as_bool().unwrap(),
            use_define_for_class_fields: row["options"]["useDefineForClassFields"].as_bool(),
            libraries: Some(vec![EcmaTarget::Es2022]),
            ..CompilerOptions::default()
        };
        let loader = MapLoader::from([ModuleSource::new(id, source)]);
        let mapped = compile(id, &loader, options.clone());
        let unmapped = compile(
            id,
            &loader,
            CompilerOptions {
                source_map: false,
                inline_sources: false,
                ..options
            },
        );
        assert!(!mapped.has_errors(), "{:?}", mapped.diagnostics);
        assert!(!unmapped.has_errors(), "{:?}", unmapped.diagnostics);
        assert_eq!(
            mapped.output.unwrap().artifacts[id].javascript,
            unmapped.output.unwrap().artifacts[id].javascript,
            "maps must not alter the source accessor-pair ABI: {index}"
        );
    }
}

fn observe(selection: &[usize], script_name: &str) {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("literal-member-origins");
    let mut rows = Vec::new();
    for &index in selection {
        let specification = &inputs::input()["cases"][index];
        let original = inputs::resolve(specification);
        let actual = cli::observe_builds(&temporary, index, &original, 2);
        let differences = declarations::compare(
            specification,
            &original,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(differences.is_empty(), "{}", differences.join("\n"));
        rows.push(
            serde_json::json!({"index":index,"id":specification["id"],"outputs":actual.outputs}),
        );
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join(script_name);
    let mut child = std::process::Command::new("node")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}
