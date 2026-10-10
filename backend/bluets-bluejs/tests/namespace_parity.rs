// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct namespace execution against pinned TypeScript (J.3.5.5): each program
//! is lowered by the direct bridge and run by BlueJS, and the JavaScript pinned
//! `tsc` emits for the same program is run by Node; the value the program
//! completes with must be the same.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use blueice_bluejs as bluejs;
use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;

/// Each program completes with a value (its last expression statement).
const PROGRAMS: &[(&str, &str)] = &[
    (
        "members, merging, nesting and reads of exported variables",
        "namespace N { export const a: number = 1; export let b: number = a + 1; const hidden: number = 5; \
         export function f(): number { return a + b + hidden; } \
         export class C { v: number = b; } export enum E { X, Y } \
         export namespace Inner { export const z: number = a * 10; } b++; } \
         const f: number = N.f(); const v: number = new N.C().v; \
         namespace N { export const extra: number = N.a + 100; } \
         f + v + N.E.Y + N.Inner.z + N.extra;",
    ),
    (
        "a closure and a shorthand property read an exported variable",
        "namespace R { export let c: number = 0; \
         export function inc(): number { c += 1; return c; } \
         export function pair(): { c: number } { return { c }; } } \
         R.inc(); R.inc(); R.pair().c * 10 + R.c;",
    ),
    (
        "namespaces merge into a function and a class",
        "function g(): number { return 1; } namespace g { export const meta: number = 5; } \
         class K { static s: number = 2; } namespace K { export const t: number = 3; } \
         g() + g.meta + K.s + K.t;",
    ),
    (
        "dotted and reopened namespaces reach earlier blocks",
        "namespace A.B { export const x: number = 1; } \
         namespace A.B { export const y: number = x + 1; } \
         namespace A { export const z: number = B.x + B.y; } \
         A.B.x + A.B.y + A.z;",
    ),
    (
        "enums merge across blocks of a namespace",
        "namespace M { export enum E { A, B } } \
         namespace M { export enum E { C = 5 } export const v: E = E.C; } \
         M.E.A + M.E.B + M.E.C + M.v;",
    ),
    (
        "an exported class extends a class another namespace exported",
        "namespace N { export class Base { x: number = 1; } } \
         namespace M { export class D extends N.Base { y: number = 2; } } \
         new M.D().x + new M.D().y;",
    ),
    (
        "a function declared in one block is read through the object in another",
        "namespace S { export function one(): number { return 1; } } \
         namespace S { export function two(): number { return one() + 1; } } \
         S.two() + S.one();",
    ),
];

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn the_direct_bridge_runs_namespaces_to_the_value_typescript_and_node_compute() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!("bluets-bluejs-namespaces-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (index, (name, source)) in PROGRAMS.iter().enumerate() {
        let artifact = compile_direct_script(
            "memory:///direct.ts",
            &MapLoader::from([ModuleSource::new("memory:///direct.ts", *source)]),
            CompilerOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{name}: the bridge refused it: {error:?}"));
        let blue = match bluejs::Vm::default().execute(&artifact.bytecode).unwrap() {
            bluejs::Value::Number(number) => format!("{number}"),
            other => panic!("{name}: unexpected completion value {other:?}"),
        };

        let directory = root.join(index.to_string());
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("program.ts"), source).unwrap();
        let emitted = Command::new(&tsc)
            .args([
                "--target", "ES2022", "--strict", "--pretty", "false", "--outDir",
            ])
            .arg(directory.join("out"))
            .arg(directory.join("program.ts"))
            .output()
            .unwrap();
        let javascript = fs::read_to_string(directory.join("out").join("program.js"))
            .unwrap_or_else(|_| panic!("{name}: tsc emitted nothing: {emitted:?}"));
        let printed = Command::new(&node)
            .arg("-p")
            .arg(&javascript)
            .output()
            .unwrap();
        assert!(printed.status.success(), "{name}: node failed: {printed:?}");
        let reference = String::from_utf8_lossy(&printed.stdout).trim().to_string();
        assert_eq!(blue, reference, "{name}: BlueJS and Node disagree");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn direct_execution_refuses_commonjs_naming_the_supported_route() {
    use blueice_bluets::{CompilerOptions, MapLoader, ModuleKind, ModuleSource};
    let loader = MapLoader::from(vec![ModuleSource::new(
        "memory:///main.ts",
        "export const a = 1;\n",
    )]);
    let options = || CompilerOptions {
        module_kind: ModuleKind::CommonJs,
        ..CompilerOptions::default()
    };
    for result in [
        blueice_bluets_bluejs::compile_direct_script("memory:///main.ts", &loader, options()).err(),
        blueice_bluets_bluejs::compile_direct_module("memory:///main.ts", &loader, options()).err(),
        blueice_bluets_bluejs::compile_direct_module_graph("memory:///main.ts", &loader, options())
            .err(),
    ] {
        let message = format!("{:?}", result.expect("CommonJS is refused"));
        assert!(
            message.contains("host-provided CommonJS loader"),
            "{message}"
        );
    }
}

#[test]
fn direct_execution_refuses_owner_loaded_module_wrappers() {
    use blueice_bluets::{CompilerOptions, MapLoader, ModuleKind, ModuleSource};
    let id = "memory:///main.ts";
    let loader = MapLoader::from([ModuleSource::new(id, "export const answer = 42;")]);
    for kind in [ModuleKind::Amd, ModuleKind::Umd] {
        let options = || CompilerOptions {
            module_kind: kind,
            ..CompilerOptions::default()
        };
        for result in [
            blueice_bluets_bluejs::compile_direct_script(id, &loader, options()).err(),
            blueice_bluets_bluejs::compile_direct_module(id, &loader, options()).err(),
            blueice_bluets_bluejs::compile_direct_module_graph(id, &loader, options()).err(),
        ] {
            let message = format!(
                "{:?}",
                result.expect("the wrapper requires an owner loader")
            );
            assert!(message.contains("ECMAScript modules only"), "{message}");
            assert!(
                message.contains(&format!("host-provided {} loader", kind.as_str())),
                "{message}"
            );
        }
    }
}

#[test]
fn direct_execution_links_no_remote_or_installed_package_declaration() {
    use blueice_bluets::{CompilerOptions, ModuleLoader, ModuleSource};
    struct Loader;
    impl ModuleLoader for Loader {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            match id {
                "main.ts" => Ok(ModuleSource::new(
                    id,
                    "import { remote } from \"remote-lib\";\nexport const a = remote();\n",
                )),
                other => Ok(ModuleSource::new(
                    other,
                    "export declare function remote(): number;\n",
                )),
            }
        }
        fn resolve(&self, _from: &str, specifier: &str) -> Result<String, String> {
            Ok(match specifier {
                "remote-lib" => format!("@remote/{}.d.ts", "0".repeat(64)),
                other => other.to_string(),
            })
        }
    }
    let error = blueice_bluets_bluejs::compile_direct_module_graph(
        "main.ts",
        &Loader,
        CompilerOptions::default(),
    )
    .err()
    .expect("a runtime import of a remote declaration is refused");
    assert!(
        format!("{error:?}").contains("links no installed packages"),
        "{error:?}"
    );
}
