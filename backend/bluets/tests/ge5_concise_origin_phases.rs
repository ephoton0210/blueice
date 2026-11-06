// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source and generated concise arrows retain distinct Native source roles.
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
fn generated_access_concise_arrows_keep_their_native_grouping_roles() {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("concise-origin-phases");
    let mut rows = Vec::new();
    for index in [6390usize, 6391, 6422, 6423] {
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
        rows.push(serde_json::json!({"index":index,"id":specification["id"],"options":original["options"],"outputs":actual.outputs}));
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_concise_origin_phases.cjs",
    );
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

#[test]
fn original_concise_operand_coexists_with_generated_access_and_keeps_javascript_bytes() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    use std::io::Write;
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source = format!("{prefix}function dec(value:any,context:any):any {{}} export const source=(value:number):number=>value+1; export class C {{ @dec field:number=1; }} export const result:number=source(41);\n");
        let id = "memory:///main.ts";
        let options = CompilerOptions {
            target: EcmaTarget::Es5,
            module_kind: ModuleKind::CommonJs,
            source_map: true,
            inline_sources: true,
            use_define_for_class_fields: Some(false),
            libraries: Some(vec![EcmaTarget::Es2022]),
            ..CompilerOptions::default()
        };
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            options.clone(),
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let without = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            CompilerOptions {
                source_map: false,
                inline_sources: false,
                ..options
            },
        );
        assert!(!without.has_errors(), "{:?}", without.diagnostics);
        assert_eq!(
            artifact.javascript,
            without.output.unwrap().artifacts[id].javascript,
            "origin selection must not change JavaScript bytes when maps are disabled"
        );
        let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_concise_origin_phases.cjs");
        let mut child = std::process::Command::new("node")
            .arg(script)
            .arg("source-operand")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&serde_json::to_vec(&serde_json::json!({"source":source,"code":artifact.javascript,"raw":artifact.source_map.as_ref().unwrap().to_json()})).unwrap()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
