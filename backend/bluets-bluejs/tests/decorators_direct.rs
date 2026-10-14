// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct execution of standard decorators (J.5.3): the program is lowered by the
//! direct bridge, where BlueJS evaluates and applies the decorators with its own
//! standard-decorator semantics, and the JavaScript pinned `tsc` emits for the
//! same program is run by Node; the completion value must agree.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use blueice_bluejs as bluejs;
use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;

/// Each program completes with a number. The direct bridge's expression subset
/// has no arrow functions, so decorators are function declarations.
const PROGRAMS: &[(&str, &str)] = &[
    (
        "method, getter and setter decorators count their applications",
        "let total: number = 0;\n\
         function addOne(value: any, ctx: any): any { total = total + 1; return undefined; }\n\
         function addTwo(value: any, ctx: any): any { total = total + 2; return undefined; }\n\
         class A { @addOne m(): number { return 10; } @addTwo get g(): number { return 20; } @addOne static s(): number { return 30; } }\n\
         total * 1000 + new A().m() + new A().g + A.s();",
    ),
    (
        "field decorators return initializers, last decorator first",
        "function plusFive(initial: number): number { return initial + 5; }\n\
         function timesTen(initial: number): number { return initial * 10; }\n\
         function five(value: any, ctx: any): any { return plusFive; }\n\
         function ten(value: any, ctx: any): any { return timesTen; }\n\
         class F { @five a: number = 1; @ten static b: number = 2; @five @ten c: number = 3; }\n\
         new F().a * 10000 + F.b * 100 + new F().c;",
    ),
    (
        "class decorators replace the class and add initializers",
        "let log: number = 0;\n\
         class Other { v: number = 42; }\n\
         function bump(): void { log = log + 1; }\n\
         function swap(value: any, ctx: any): any { ctx.addInitializer(bump); return Other; }\n\
         @swap class Orig { v: number = 1; }\n\
         new Orig().v * 10 + log;",
    ),
    (
        "auto-accessors with decorators",
        "function twice(v: number): number { return v * 2; }\n\
         function doubled(value: any, ctx: any): any { return { init: twice }; }\n\
         class Acc { @doubled accessor n: number = 21; accessor plain: number = 4; }\n\
         const acc = new Acc();\n\
         acc.n = acc.n + 1;\n\
         acc.n * 100 + acc.plain;",
    ),
    (
        "extra initializers run per instance",
        "let instances: number = 0;\n\
         function bump(): void { instances = instances + 1; }\n\
         function counted(value: any, ctx: any): void { ctx.addInitializer(bump); }\n\
         class Counted { @counted m(): void {} constructor() {} }\n\
         new Counted(); new Counted(); new Counted();\n\
         instances;",
    ),
    (
        "a member-expression decorator is called without a receiver bound by the bridge",
        "let seen: number = 0;\n\
         function mark(value: any, ctx: any): void { seen = seen + 7; }\n\
         class P { @mark m(): void {} }\n\
         new P();\n\
         seen;",
    ),
];

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn the_direct_bridge_runs_standard_decorators_to_the_value_typescript_and_node_compute() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!("bluets-bluejs-decorators-{}", std::process::id()));
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
            .args(["--target", "ES2022", "--pretty", "false", "--outDir"])
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
fn a_decorated_class_runs_directly_without_a_helper() {
    let program = "function plusTwo(initial: number): number { return initial + 2; }\nfunction add(value: any, ctx: any): any { return plusTwo; }\nclass F { @add a: number = 1; }\nnew F().a;";
    let artifact = compile_direct_script(
        "memory:///direct.ts",
        &MapLoader::from([ModuleSource::new("memory:///direct.ts", program)]),
        CompilerOptions::default(),
    )
    .unwrap();
    match bluejs::Vm::default().execute(&artifact.bytecode).unwrap() {
        bluejs::Value::Number(number) => assert_eq!(number, 3.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn experimental_decorators_are_refused_naming_the_build_route() {
    let program = "function mark(target: any, key: string): void {}\nclass F { @mark a: number = 1; }\nnew F().a;";
    let error = compile_direct_script(
        "memory:///direct.ts",
        &MapLoader::from([ModuleSource::new("memory:///direct.ts", program)]),
        CompilerOptions {
            experimental_decorators: true,
            ..CompilerOptions::default()
        },
    )
    .err()
    .expect("legacy decorators are refused");
    assert!(
        format!("{error:?}").contains("bluetsc build --experimental-decorators"),
        "{error:?}"
    );
}
