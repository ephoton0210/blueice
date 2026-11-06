// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Execute emitted decorators after parameter-property erasure and lowering.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

#[test]
#[ignore = "requires Node"]
fn decorated_base_and_derived_constructors_initialize_parameter_properties() {
    for target in [EcmaTarget::Es5, EcmaTarget::Es2020, EcmaTarget::Es2022] {
        for define in [false, true] {
            for (declaration, expected) in [
                ("@keep abstract class Base { constructor(public value: number) {} abstract read(): number; }
                  class Child extends Base { read(): number { return this.value; } }
                  export const result = new Child(42).read();", 42),
                ("class Base { constructor(public base: number) {} }
                  @keep class Child extends Base { constructor(public value: number) { super(value + 1); } }
                  const child = new Child(42); export const result = child.value + child.base;", 85),
            ] {
                let source = format!("function keep(value: any, context: any): any {{ return value; }} {declaration}");
                let result = compile("main.ts", &MapLoader::from([ModuleSource::new("main.ts", &source)]), CompilerOptions {
                    target, libraries: Some(vec![EcmaTarget::Es2022]),
                    module_kind: ModuleKind::CommonJs, use_define_for_class_fields: Some(define),
                    ..CompilerOptions::default()
                });
                assert!(!result.has_errors(), "{target:?}/{define}: {:?}", result.diagnostics);
                let javascript = &result.output.unwrap().artifacts["main.ts"].javascript;
                let output = Command::new("node").args(["-e", "const result={}; new Function('exports',process.argv[1])(result); require('node:assert/strict').equal(result.result,Number(process.argv[2]));", javascript, &expected.to_string()]).output().unwrap();
                assert!(output.status.success(), "{target:?}/{define}: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
    }
}
