// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A legacy provider observes the original numeric property key rather than a string coercion.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

#[test]
fn legacy_numeric_property_keys_keep_their_provider_argument_type() {
    let body="const kinds:any[]=[]; const reflection:any=Reflect; reflection.decorate=function(decorators:any[],target:any,key:any,descriptor:any):any { kinds.push(typeof key); }; function dec(...args:any[]):any {} export class Box { @dec 1.5:number=40; } export const result:any=kinds;\n";
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source = format!("{prefix}{body}");
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for source_map in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map,
                        inline_sources: source_map,
                        experimental_decorators: true,
                        use_define_for_class_fields: Some(false),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let code = &result.output.unwrap().artifacts[id].javascript;
                let execution = Command::new("node")
                    .args([
                        "-e",
                        &format!("{code}\nprocess.stdout.write(JSON.stringify(exports.result));"),
                    ])
                    .output()
                    .unwrap();
                assert!(
                    execution.status.success(),
                    "{}",
                    String::from_utf8_lossy(&execution.stderr)
                );
                assert_eq!(
                    String::from_utf8(execution.stdout).unwrap(),
                    "[\"number\"]",
                    "{target:?} maps={source_map} provider observes Native numeric-key ABI"
                );
            }
        }
    }
}
