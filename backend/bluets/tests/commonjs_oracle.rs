// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CommonJS emit against pinned TypeScript (J.4.1): each program is compiled by
//! BlueTSC and by `tsc --module commonjs` and both must print the same thing
//! under Node, which pins live bindings, initialization order, cycles and
//! interop rather than the emitted text. The offline tests beside it pin the
//! module-system diagnostics.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleKind, ModuleSource};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// `(name, es_module_interop, files)`; the first file is the entry.
type Program = (&'static str, bool, &'static [(&'static str, &'static str)]);

const PROGRAMS: &[Program] = &[
    (
        "named-default-namespace-and-side-effect-imports",
        false,
        &[
            (
                "main.ts",
                r#"import { f2, v2 as w } from "./b.ts";
import * as ns from "./b.ts";
import "./b.ts";
export const a = 1;
export let b = 2;
export function f() { return a + b + f2() + w; }
export class C { x = 1; }
export enum E { X, Y }
export namespace N { export const z = 10; }
const local = 5;
export { local as loc };
b++;
console.log(f(), new C().x, E.Y, N.z, ns.v2, ns.f2());
import * as self from "./main.ts";
console.log(Object.keys(self).sort().join(), self.b, self.loc);
"#,
            ),
            (
                "b.ts",
                r#"export const v2 = 3;
export function f2() { return 4; }
console.log("b loaded");
"#,
            ),
        ],
    ),
    (
        "cycle-sees-partial-exports-and-hoisted-functions",
        false,
        &[
            (
                "main.ts",
                r#"import { fromB, readA } from "./b.ts";
export const a = "a-value";
export function hoisted() { return "hoisted-a"; }
console.log(fromB, readA());
"#,
            ),
            (
                "b.ts",
                r#"import { a, hoisted } from "./main.ts";
export const fromB = typeof a + "/" + hoisted();
export function readA() { return a; }
"#,
            ),
        ],
    ),
    (
        "export-equals-and-import-equals-require",
        false,
        &[
            (
                "main.ts",
                r#"import make = require("./lib.ts");
console.log(make(2), typeof make);
"#,
            ),
            (
                "lib.ts",
                r#"function make(n: number): number { return n * 21; }
export = make;
"#,
            ),
        ],
    ),
    (
        "default-import-of-an-export-equals-module-with-interop",
        true,
        &[
            (
                "main.ts",
                r#"import make from "./lib.ts";
import * as all from "./lib.ts";
console.log(make(2), typeof all);
"#,
            ),
            (
                "lib.ts",
                r#"function make(n: number): number { return n * 21; }
export = make;
"#,
            ),
        ],
    ),
    (
        "default-export-and-default-import",
        false,
        &[
            (
                "main.ts",
                r#"import greet from "./g.ts";
console.log(greet("x"));
"#,
            ),
            (
                "g.ts",
                r#"export default function greet(name: string): string { return "hi " + name; }
"#,
            ),
        ],
    ),
    (
        "imported-method-call-does-not-bind-this-to-the-module",
        false,
        &[
            (
                "main.ts",
                r#"import { who } from "./w.ts";
console.log(who());
"#,
            ),
            (
                "w.ts",
                r#"export function who() { return typeof (this as unknown); }
"#,
            ),
        ],
    ),
];

fn options(program: &Program) -> CompilerOptions {
    CompilerOptions {
        module_kind: ModuleKind::CommonJs,
        es_module_interop: program.1,
        ..CompilerOptions::default()
    }
}

fn compile_program(program: &Program) -> Vec<(String, String)> {
    let sources: Vec<ModuleSource> = program
        .2
        .iter()
        .map(|(file, text)| ModuleSource::new(format!("memory:///{file}"), *text))
        .collect();
    let compiled = compile(
        &format!("memory:///{}", program.2[0].0),
        &MapLoader::from(sources),
        options(program),
    );
    assert!(
        compiled.output.is_some(),
        "BlueTSC rejected {}: {:#?}",
        program.0,
        compiled.diagnostics
    );
    compiled
        .output
        .unwrap()
        .artifacts
        .iter()
        .map(|(id, artifact)| {
            (
                id.strip_prefix("memory:///").unwrap().replace(".ts", ".js"),
                artifact.javascript.clone(),
            )
        })
        .collect()
}

#[test]
fn commonjs_artifacts_use_require_and_exports_and_no_esm_syntax() {
    for program in PROGRAMS {
        for (file, javascript) in compile_program(program) {
            assert!(
                !javascript.contains("import ") && !javascript.contains("export "),
                "{} {file} kept ES module syntax:\n{javascript}",
                program.0
            );
            assert!(
                !javascript.contains(".ts\"") && !javascript.contains(".ts'"),
                "{} {file} left a .ts specifier:\n{javascript}",
                program.0
            );
        }
    }
}

#[test]
fn commonjs_header_and_references_follow_typescripts_shape() {
    let output = compile_program(&PROGRAMS[0]);
    let main = &output.iter().find(|(file, _)| file == "main.js").unwrap().1;
    assert!(main.starts_with(
        "\"use strict\"; Object.defineProperty(exports, \"__esModule\", { value: true });"
    ));
    assert!(main.contains("exports.a = 1;"), "{main}");
    assert!(main.contains("exports.b++"), "{main}");
    assert!(main.contains("(0, b_1.f2)()"), "{main}");
    assert!(main.contains("exports.f = f;"), "{main}");
    assert!(main.contains("exports.C = C;"), "{main}");
    assert!(main.contains("exports.E = E = {}"), "{main}");
    assert!(main.contains("exports.N = N = {}"), "{main}");
    assert!(main.contains("exports.loc = local;"), "{main}");
}

#[test]
fn commonjs_emit_keeps_every_source_line() {
    for program in PROGRAMS {
        let output = compile_program(program);
        for (file, text) in program.2 {
            let javascript = &output
                .iter()
                .find(|(name, _)| name == &file.replace(".ts", ".js"))
                .unwrap()
                .1;
            assert_eq!(
                javascript.matches('\n').count(),
                text.matches('\n').count(),
                "{} {file}",
                program.0
            );
        }
    }
}

fn diagnostics(module_kind: ModuleKind, interop: bool, files: &[(&str, &str)]) -> Vec<String> {
    let sources: Vec<ModuleSource> = files
        .iter()
        .map(|(file, text)| ModuleSource::new(format!("memory:///{file}"), *text))
        .collect();
    compile(
        &format!("memory:///{}", files[0].0),
        &MapLoader::from(sources),
        CompilerOptions {
            module_kind,
            es_module_interop: interop,
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .map(|diagnostic| diagnostic.message.clone())
    .collect()
}

const EQUALS_LIB: (&str, &str) = (
    "lib.ts",
    "function make(n: number): number { return n; }\nexport = make;\n",
);

#[test]
fn import_equals_and_export_equals_are_refused_in_an_ecmascript_module() {
    let messages = diagnostics(
        ModuleKind::Esm,
        false,
        &[
            (
                "main.ts",
                "import make = require(\"./lib.ts\");\nmake(1);\n",
            ),
            EQUALS_LIB,
        ],
    )
    .join("\n");
    assert!(messages.contains("`import x = require()`"), "{messages}");
    assert!(messages.contains("`export =` cannot be used"), "{messages}");
}

#[test]
fn a_default_import_of_an_export_equals_module_needs_interop() {
    let main = "import make from \"./lib.ts\";\nmake(1);\n";
    let without = diagnostics(
        ModuleKind::CommonJs,
        false,
        &[("main.ts", main), EQUALS_LIB],
    )
    .join("\n");
    assert!(without.contains("esModuleInterop"), "{without}");
    let with = diagnostics(ModuleKind::CommonJs, true, &[("main.ts", main), EQUALS_LIB]);
    assert!(with.is_empty(), "{with:?}");
}

#[test]
fn an_export_assignment_cannot_share_a_module_with_other_exports() {
    let messages = diagnostics(
        ModuleKind::CommonJs,
        false,
        &[(
            "main.ts",
            "export const a = 1;\nconst b = 2;\nexport = b;\n",
        )],
    )
    .join("\n");
    assert!(
        messages.contains("export assignment cannot be used"),
        "{messages}"
    );
}

#[test]
fn top_level_await_is_refused_in_a_commonjs_module() {
    let messages = diagnostics(
        ModuleKind::CommonJs,
        false,
        &[(
            "main.ts",
            "export const a = 1;\nawait Promise.resolve(1);\n",
        )],
    )
    .join("\n");
    assert!(messages.contains("top-level `await`"), "{messages}");
}

#[test]
fn a_local_that_shadows_an_imported_name_is_refused_rather_than_misrewritten() {
    let messages = diagnostics(
        ModuleKind::CommonJs,
        false,
        &[
            (
                "main.ts",
                "import { v2 } from \"./b.ts\";\nfunction g(v2: number) { return v2; }\nconsole.log(g(1), v2);\n",
            ),
            ("b.ts", "export const v2 = 3;\n"),
        ],
    )
    .join("\n");
    assert!(
        messages.contains("shadows an imported or exported binding"),
        "{messages}"
    );
}

#[test]
fn the_module_kind_is_part_of_the_project_fingerprint() {
    let sources = |kind| {
        compile(
            "memory:///main.ts",
            &MapLoader::from(vec![ModuleSource::new(
                "memory:///main.ts",
                "export const a = 1;\n",
            )]),
            CompilerOptions {
                module_kind: kind,
                ..CompilerOptions::default()
            },
        )
        .output
        .unwrap()
        .fingerprint
    };
    assert_ne!(sources(ModuleKind::Esm), sources(ModuleKind::CommonJs));
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_commonjs_programs_print_what_typescript_prints() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    for program in PROGRAMS {
        let directory = Directory::new();
        let blue = directory.path().join("blue");
        let reference = directory.path().join("tsc");
        let input = directory.path().join("src");
        for folder in [&blue, &reference, &input] {
            fs::create_dir_all(folder).unwrap();
        }
        for folder in [&blue, &reference] {
            fs::write(folder.join("package.json"), "{\"type\":\"commonjs\"}\n").unwrap();
        }
        for (file, text) in program.2 {
            fs::write(input.join(file), text).unwrap();
        }
        for (file, javascript) in compile_program(program) {
            fs::write(blue.join(file), javascript).unwrap();
        }
        let mut command = Command::new(&tsc);
        command.args([
            "--target",
            "ES2022",
            "--module",
            "commonjs",
            "--strict",
            "--pretty",
            "false",
            "--allowImportingTsExtensions",
            "--rewriteRelativeImportExtensions",
            "--outDir",
        ]);
        command.arg(&reference);
        if program.1 {
            command.arg("--esModuleInterop");
        }
        let emitted = command.arg(input.join(program.2[0].0)).output().unwrap();
        let entry = program.2[0].0.replace(".ts", ".js");
        assert!(
            reference.join(&entry).exists(),
            "tsc emitted nothing for {}: {emitted:?}",
            program.0
        );
        let blue_run = Command::new(&node).arg(blue.join(&entry)).output().unwrap();
        let tsc_run = Command::new(&node)
            .arg(reference.join(&entry))
            .output()
            .unwrap();
        assert!(
            blue_run.status.success(),
            "{}: BlueTSC output failed: {blue_run:?}",
            program.0
        );
        assert!(
            tsc_run.status.success(),
            "{}: tsc output failed: {tsc_run:?}",
            program.0
        );
        assert_eq!(
            String::from_utf8_lossy(&blue_run.stdout),
            String::from_utf8_lossy(&tsc_run.stdout),
            "{}: BlueTSC and TypeScript print different output",
            program.0
        );
    }
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "blueice-bluets-commonjs-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
