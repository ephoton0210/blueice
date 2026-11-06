// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete Native module contracts for every frozen literal replacement-order decorator product.
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

use serde_json::json;

#[test]
fn preserved_class_decorator_transport_keeps_the_keyword_and_name_roles() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    use std::io::Write;
    use std::process::{Command, Stdio};
    let forms = [
        (
            "class-export",
            "@replace export class C{@dec x:number=1;} export const result:any=C;",
        ),
        (
            "class-local",
            "@replace class C{@dec x:number=1;} export {C};",
        ),
        (
            "member-export",
            "export class C{@dec x:number=1;} export const result:any=C;",
        ),
        (
            "plain-export",
            "export class C{x:number=1;} export const result:any=C;",
        ),
    ];
    let mut rows = Vec::new();
    for (form, body) in forms {
        for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
            for crlf in [false, true] {
                let plain=format!("function replace(value:any,context:any):any{{return value;}} function dec(value:any,context:any):any{{return value;}}\n{body}\n");
                let source = if crlf {
                    format!("/* G-E5 🧭 */ {}", plain.replace('\n', "\r\n"))
                } else {
                    plain
                };
                let id = "memory:///main.ts";
                let loader = MapLoader::from([ModuleSource::new(id, &source)]);
                let mut previous = None;
                for maps in [false, true] {
                    let result = compile(
                        id,
                        &loader,
                        CompilerOptions {
                            target: EcmaTarget::EsNext,
                            module_kind,
                            use_define_for_class_fields: Some(true),
                            source_map: maps,
                            inline_sources: maps,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let output = result.output.unwrap();
                    let artifact = &output.artifacts[id];
                    if let Some(previous) = &previous {
                        assert_eq!(
                            previous, &artifact.javascript,
                            "keyword source roles cannot change JavaScript bytes"
                        );
                    } else {
                        previous = Some(artifact.javascript.clone());
                    }
                    if maps {
                        rows.push(json!({"form":form,"source":source,"javascript":artifact.javascript,"map":artifact.source_map.as_ref().unwrap().to_json(),"module":if module_kind==ModuleKind::CommonJs{"CommonJS"}else{"ESNext"}}));
                    }
                }
            }
        }
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_replacement_preserved_header.cjs");
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
fn class_decorated_static_fields_preserve_the_target_specific_receiver() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut rows = Vec::new();
    for legacy in [false, true] {
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for define in [false, true] {
                for crlf in [false, true] {
                    let plain="function dec(value:any,context:any):any{return function(this:any,initial:any){return initial+1;};}\nfunction replace(value:any):any{return class extends value{static replacement=true;};}\n@replace export class C{@dec static x:number=1;@dec y:number=2;}\nexport const result:any=[Object.hasOwn(C,'x'),C.x,new C().y];\n";
                    let plain = if legacy {
                        plain.replace("function dec(value:any,context:any):any{return function(this:any,initial:any){return initial+1;};}", "function dec(...args:any[]):any{}")
                    } else {
                        plain.to_string()
                    };
                    let source = if crlf {
                        format!("/* G-E5 🧭 */ {}", plain.replace('\n', "\r\n"))
                    } else {
                        plain.to_string()
                    };
                    let id = "memory:///main.ts";
                    let loader = MapLoader::from([ModuleSource::new(id, &source)]);
                    let mut previous = None;
                    for maps in [false, true] {
                        let result = compile(
                            id,
                            &loader,
                            CompilerOptions {
                                target,
                                experimental_decorators: legacy,
                                module_kind: ModuleKind::CommonJs,
                                use_define_for_class_fields: Some(define),
                                source_map: maps,
                                inline_sources: maps,
                                libraries: Some(vec![EcmaTarget::Es2022]),
                                ..CompilerOptions::default()
                            },
                        );
                        assert!(!result.has_errors(), "{:?}", result.diagnostics);
                        let output = result.output.unwrap();
                        let artifact = &output.artifacts[id];
                        let javascript = artifact.javascript.clone();
                        if let Some(previous) = &previous {
                            assert_eq!(
                                previous, &javascript,
                                "replacement receiver identity must not depend on map emission"
                            );
                        } else {
                            previous = Some(javascript.clone());
                        }
                        rows.push(json!({"source":source,"javascript":javascript,"map":artifact.source_map.as_ref().map(|map|map.to_json()),"target":match target{EcmaTarget::Es5=>"ES5",EcmaTarget::Es2015=>"ES2015",_=>"ES2022"},"define":define,"maps":maps,"legacy":legacy}));
                    }
                }
            }
        }
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_replacement_receiver.cjs",
    );
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
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15 and every frozen literal replacement product"]
fn literal_replacement_order_preserves_every_complete_module_role() {
    inputs::verify_authorities();
    let group = "literal-replacement-order";
    let temporary = cli::Temporary::new("decorator-call-roles");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let specifications = inputs::input()["cases"].as_array().unwrap();
    let selection = specifications
        .iter()
        .enumerate()
        .filter(|(_, specification)| {
            specification["form"] == "ge3_decorator_compositions/literal-replacement-order"
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(selection.len(), 67);
    let mut observations = std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|worker| {
                let temporary = &temporary;
                let compiler = &compiler;
                let selection = &selection;
                scope.spawn(move || {
                    let mut result = Vec::new();
                    for (position, &index) in selection.iter().enumerate() {
                        if position % 2 != worker {
                            continue;
                        }
                        let specification = &specifications[index];
                        let original = inputs::resolve(specification);
                        let actual = cli::observe_builds(temporary, index, &original, 2);
                        let differences = declarations::compare(
                            specification,
                            &original,
                            &inputs::reference()["cases"][index],
                            &actual,
                        );
                        assert!(differences.is_empty(), "{}", differences.join("\n"));
                        result.push(json!({"index":index,"id":specification["id"],
                            "compilerSha256":compiler,"repeatBuilds":2,
                            "options":original["options"],"outputs":actual.outputs}));
                    }
                    result
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observations.sort_by_key(|row| row["index"].as_u64().unwrap());
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("decorator-{group}.json")),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    use std::io::Write;
    use std::process::{Command, Stdio};
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_decorator_replacement_roles.cjs",
    );
    let mut child = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let written = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&observations).unwrap());
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    written.unwrap();
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}
