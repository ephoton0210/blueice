// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public compiler boundaries for original auto-accessor storage and callable phases.
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
fn auto_accessor_storage_and_both_callable_ends_match_native() {
    observe(false, false);
}

#[test]
fn static_auto_accessors_keep_their_original_lexical_receiver() {
    observe(true, false);
}

#[test]
fn auto_accessor_storage_has_identical_javascript_with_and_without_maps() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let specifications = inputs::input()["cases"].as_array().unwrap();
    let mut count = 0;
    for specification in specifications
        .iter()
        .filter(|row| row["form"] == "ge3_decorator_compositions/literal-auto-accessors")
    {
        let row = inputs::resolve(specification);
        let source = row["files"]["main.ts"].as_str().unwrap();
        let id = "memory:///main.ts";
        let options = CompilerOptions {
            target: match row["options"]["target"].as_str().unwrap() {
                "ES2015" => EcmaTarget::Es2015,
                "ES2022" => EcmaTarget::Es2022,
                "ESNext" => EcmaTarget::EsNext,
                target => panic!("unfrozen auto-accessor target: {target}"),
            },
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
            "auto-accessor phases must not depend on map emission: {}",
            specification["id"]
        );
        count += 1;
    }
    assert_eq!(count, 51);
}

#[test]
fn lowered_auto_accessor_private_storage_keeps_every_original_role() {
    observe(false, true);
}

#[test]
fn lowered_static_auto_accessors_keep_their_original_lexical_receiver() {
    observe(true, true);
}

#[test]
fn legacy_auto_accessor_helpers_keep_name_and_decorator_phases() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("legacy-auto-accessor-helpers");
    let mut rows = Vec::new();
    for index in 6302..6326 {
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
        rows.push(serde_json::json!({"index":index,"outputs":actual.outputs}));
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_literal_auto_accessor_legacy.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn lowered_auto_accessor_class_core_preserves_every_boundary_and_gap() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let temporary = cli::Temporary::new("auto-accessor-class-core");
    let mut rows = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != "ge3_decorator_compositions/literal-auto-accessors" {
            continue;
        }
        let original = inputs::resolve(specification);
        if original["options"]["target"] == "ESNext" {
            continue;
        }
        let actual = cli::observe_builds(&temporary, index, &original, 2);
        let differences = declarations::compare(
            specification,
            &original,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(differences.is_empty(), "{}", differences.join("\n"));
        rows.push(serde_json::json!({"index":index,"outputs":actual.outputs}));
    }
    assert_eq!(rows.len(), 35);
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_literal_auto_accessor_core.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn anonymous_class_names_reserve_file_level_static_private_captures() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    use std::io::Write;
    use std::process::{Command, Stdio};
    let forms = [
        ("none", "class C{}"),
        ("private-static", "class C{static #x=1;}"),
        ("two-static", "class C{static #x=1;}class D{static #x=2;}"),
        ("instance-private", "class C{#x=1;}"),
        ("plain-auto", "class C{static accessor x=1;}"),
        ("decorated-auto", "class C{@dec static accessor x=1;}"),
        (
            "nested-static",
            "function local(){class C{static #x=1;}return C;}",
        ),
        ("reserved-user", "const _a=0;class C{static #x=1;}"),
    ];
    let mut rows = Vec::new();
    for legacy in [false, true] {
        for (form, classes) in forms {
            for variant in ["original", "crlfAstral"] {
                let plain=format!("function dec(...args:any[]):any{{}}\nfunction make(value:any):any{{return class extends value{{static replacement=true;}};}}\n{classes}\nexport const result:any=make(Object).name;\n");
                let source = if variant == "original" {
                    plain
                } else {
                    format!("/* G-E5 🧭 */ {}", plain.replace('\n', "\r\n"))
                };
                let id = "memory:///main.ts";
                let loader = MapLoader::from([ModuleSource::new(id, &source)]);
                let output = compile(
                    id,
                    &loader,
                    CompilerOptions {
                        target: EcmaTarget::Es2015,
                        module_kind: ModuleKind::CommonJs,
                        experimental_decorators: legacy,
                        use_define_for_class_fields: Some(false),
                        source_map: true,
                        inline_sources: true,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(
                    !output.has_errors(),
                    "{form}/{legacy}/{variant}: {:?}",
                    output.diagnostics
                );
                rows.push(serde_json::json!({"form":form,"legacy":legacy,"variant":variant,"source":source,"javascript":output.output.unwrap().artifacts[id].javascript}));
            }
        }
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_auto_accessor_capture_names.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn auto_accessor_factory_phases_keep_distinct_statement_endpoints() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let temporary = cli::Temporary::new("auto-accessor-factory-phases");
    let mut rows = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != "ge3_decorator_compositions/literal-auto-accessors" {
            continue;
        }
        let original = inputs::resolve(specification);
        if original["options"]["target"] != "ES2015"
            || original["options"]["experimentalDecorators"] == true
        {
            continue;
        }
        let actual = cli::observe_builds(&temporary, index, &original, 2);
        let differences = declarations::compare(
            specification,
            &original,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(differences.is_empty(), "{}", differences.join("\n"));
        rows.push(serde_json::json!({"index":index,"outputs":actual.outputs}));
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_auto_accessor_factory_phases.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn legacy_auto_accessor_static_storage_keeps_its_statement_endpoint() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let temporary = cli::Temporary::new("legacy-auto-accessor-static-completion");
    let mut rows = Vec::new();
    for index in 6302..6310 {
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
        rows.push(serde_json::json!({"index":index,"outputs":actual.outputs}));
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_auto_accessor_legacy_completion.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn legacy_auto_accessor_static_initializers_observe_the_initial_module_binding() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut rows = Vec::new();
    for target in [EcmaTarget::Es2015, EcmaTarget::Es2022] {
        for published in [false, true] {
            for crlf in [false, true] {
                let export = if published { "export " } else { "" };
                let user_publication = if published { "" } else { "exports.C=C;" };
                let plain=format!("declare const exports:any;\nlet observed:any;function dec(...args:any[]):any{{}}\n{export}class C{{@dec accessor x:number=1;@dec static accessor y:number=(observed=exports.C,2);}}{user_publication}\nexport const result=[observed===C,typeof observed,C.y];\n");
                let source = if crlf {
                    format!("/* G-E5 🧭 */ {}", plain.replace('\n', "\r\n"))
                } else {
                    plain
                };
                let id = "memory:///main.ts";
                let loader = MapLoader::from([ModuleSource::new(id, &source)]);
                let mut javascript = None;
                for maps in [false, true] {
                    let result = compile(
                        id,
                        &loader,
                        CompilerOptions {
                            target,
                            module_kind: ModuleKind::CommonJs,
                            experimental_decorators: true,
                            use_define_for_class_fields: Some(false),
                            source_map: maps,
                            inline_sources: maps,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let code = result.output.unwrap().artifacts[id].javascript.clone();
                    if let Some(previous) = &javascript {
                        assert_eq!(
                            previous, &code,
                            "publication ordering must not depend on map emission"
                        );
                    } else {
                        javascript = Some(code.clone());
                    }
                    rows.push(serde_json::json!({"source":source,"javascript":code,"maps":maps,"target":if target==EcmaTarget::Es2015{"ES2015"}else{"ES2022"},"published":published}));
                }
            }
        }
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_auto_accessor_publication_runtime.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

fn observe(runtime: bool, lowered: bool) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("literal-auto-accessor-origins");
    let mut rows = Vec::new();
    let selection = if lowered {
        (6278..6286).chain(6302..6310)
    } else {
        (6286..6294).chain(6310..6318)
    };
    for index in selection {
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
        rows.push(serde_json::json!({"index":index,"outputs":actual.outputs}));
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join("verify_ge5_literal_auto_accessor_storage.cjs");
    let mut command = Command::new("node");
    command.arg("--experimental-vm-modules").arg(script);
    if runtime {
        command.arg("--runtime");
    }
    if lowered {
        command.arg("--lowered");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}
