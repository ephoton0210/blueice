// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Option combinations against pinned TypeScript (J.6.2): one program that uses
//! enums, a const enum, namespaces, classes with fields, private names, static
//! members and cross-module imports is built by `bluetsc` and by `tsc` under every
//! combination of the options that change emit, and both outputs must print the
//! same thing under Node. The options are `target`, the module system,
//! `useDefineForClassFields`, `preserveConstEnums` and `isolatedModules`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const LIB: &str = r#"export const enum Color { Red, Green = 5, Blue }
export enum Level { Low = 1, High = 10 }
export namespace Util {
  export const base: number = 100;
  export function twice(n: number): number { return n * 2; }
  export enum Kind { A, B }
}
export class Base {
  static count: number = 0;
  id: number = ++Base.count;
  #secret: number = 7;
  tag: string = "base";
  reveal(): number { return this.#secret; }
}
"#;

const MAIN: &str = r#"import { Color, Level, Util, Base } from "./lib.ts";
class Local { name: string = "local"; kind: string; constructor() { this.kind = "k"; } }
class LocalDerived extends Local { name: string = "derived-local"; }
class Derived extends Base {
  note: string = "derived";
  extra: number = Util.twice(this.id);
  static label: string = "L" + Base.count;
}
const d = new Derived();
const e = new Derived();
console.log(Color.Red, Color.Green, Color.Blue, Level.Low, Level[10], Util.base, Util.Kind[1]);
console.log(new LocalDerived().name, new LocalDerived().kind, d.id, d.tag, d.note, d.extra, d.reveal(), e.id, Derived.label, Base.count);
console.log(Object.keys(d).join(","), Object.getOwnPropertyNames(Derived).sort().join(","));
"#;

const OPTIONS_MATRIX: &str =
    include_str!("fixtures/option_combinations/options-checker-matrix.tsv");

#[test]
fn recorded_matrix_covers_the_required_cartesian_product() {
    assert_eq!(OPTIONS_MATRIX.lines().count(), 768);
    assert_eq!(
        combinations().len(),
        768,
        "K.2.4 requires the new project options in the Cartesian oracle"
    );
}

struct Combination {
    target: &'static str,
    module: &'static str,
    define: Option<bool>,
    preserve: bool,
    isolated: bool,
}

fn combinations() -> Vec<Combination> {
    let mut all = Vec::new();
    for target in ["es2020", "es2022"] {
        for module in ["esnext", "commonjs"] {
            for define in [None, Some(true), Some(false)] {
                for preserve in [false, true] {
                    for isolated in [false, true] {
                        all.push(Combination {
                            target,
                            module,
                            define,
                            preserve,
                            isolated,
                        });
                    }
                }
            }
        }
    }
    all
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn every_emit_option_combination_prints_what_typescript_prints() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!("bluets-options-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let input = root.join("src");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("lib.ts"), LIB).unwrap();
    fs::write(input.join("main.ts"), MAIN).unwrap();
    let combinations = combinations();
    assert_eq!(combinations.len(), 48);
    for (index, combination) in combinations.iter().enumerate() {
        let label = format!(
            "target={} module={} define={:?} preserve={} isolated={}",
            combination.target,
            combination.module,
            combination.define,
            combination.preserve,
            combination.isolated
        );
        let commonjs = combination.module == "commonjs";
        let blue = root.join(format!("blue-{index}"));
        let reference = root.join(format!("tsc-{index}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
        command
            .current_dir(&input)
            .args([
                "build",
                "main.ts",
                "--target",
                combination.target,
                "--module",
                combination.module,
                "--out-dir",
            ])
            .arg(&blue);
        let mut tsc_command = Command::new(&tsc);
        tsc_command
            .current_dir(&input)
            .args([
                "--target",
                &combination.target.to_uppercase(),
                "--module",
                if commonjs { "commonjs" } else { "es2022" },
                "--pretty",
                "false",
                "--skipLibCheck",
                "--allowImportingTsExtensions",
                "--rewriteRelativeImportExtensions",
                "--outDir",
            ])
            .arg(&reference)
            .arg("main.ts");
        if let Some(define) = combination.define {
            let value = if define { "true" } else { "false" };
            command.args(["--use-define-for-class-fields", value]);
            tsc_command.args(["--useDefineForClassFields", value]);
        }
        if combination.preserve {
            command.arg("--preserve-const-enums");
            tsc_command.arg("--preserveConstEnums");
        }
        if combination.isolated {
            command.arg("--isolated-modules");
            tsc_command.arg("--isolatedModules");
        }
        let built = command.output().unwrap();
        assert!(
            built.status.success(),
            "{label}: bluetsc failed: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let emitted = tsc_command.output().unwrap();
        assert!(
            reference.join("main.js").exists(),
            "{label}: tsc emitted nothing: {}",
            String::from_utf8_lossy(&emitted.stdout)
        );
        for directory in [&blue, &reference] {
            fs::write(
                directory.join("package.json"),
                if commonjs {
                    r#"{"type":"commonjs"}"#
                } else {
                    r#"{"type":"module"}"#
                },
            )
            .unwrap();
        }
        let run = |directory: &PathBuf| {
            Command::new(&node)
                .env("FORCE_COLOR", "0")
                .arg(directory.join("main.js"))
                .output()
                .unwrap()
        };
        let blue_run = run(&blue);
        let tsc_run = run(&reference);
        assert!(
            blue_run.status.success(),
            "{label}: BlueTSC output failed: {}",
            String::from_utf8_lossy(&blue_run.stderr)
        );
        assert!(
            tsc_run.status.success(),
            "{label}: tsc output failed: {}",
            String::from_utf8_lossy(&tsc_run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&blue_run.stdout),
            String::from_utf8_lossy(&tsc_run.stdout),
            "{label}: BlueTSC and TypeScript print different output"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn new_option_matrix_matches_pinned_typescript() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE").expect("set BLUEICE_BLUETSC_ORACLE");
    let root = env::temp_dir().join(format!("bluets-options-record-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.ts"), MAIN).unwrap();
    fs::write(root.join("src/lib.ts"), LIB).unwrap();
    fs::write(
        root.join("cases.json"),
        include_str!("fixtures/option_combinations/cases.json"),
    )
    .unwrap();
    let script = r#"
const path = require('path'), fs = require('fs');
const ts = require(path.resolve(process.argv[1], '../../lib/typescript.js'));
if (ts.version !== '5.9.3') throw Error(ts.version);
let lines = [];
let previous;
for (const row of JSON.parse(fs.readFileSync('cases.json', 'utf8'))) {
 const converted = ts.convertCompilerOptionsFromJson(row.options, process.cwd());
 const program = ts.createProgram([path.resolve('src/main.ts')], converted.options, undefined, previous);
 previous = program;
 const diagnostics = [...converted.errors,...ts.getPreEmitDiagnostics(program)];
 lines.push(row.name + '\t' + (diagnostics.length ? 'reject' : 'accept'));
}
fs.writeFileSync('matrix.tsv', lines.sort().join('\n')+'\n');
"#;
    let output = Command::new("node")
        .current_dir(&root)
        .args(["-e", script])
        .arg(tsc)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(root.join("matrix.tsv")).unwrap();
    if env::var_os("BLUEICE_WRITE_OPTIONS_MATRIX").is_some() {
        fs::write(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/option_combinations/options-checker-matrix.tsv"),
            text,
        )
        .unwrap();
    } else {
        assert_eq!(
            text.lines().collect::<std::collections::BTreeSet<_>>(),
            OPTIONS_MATRIX.lines().collect()
        );
    }
    fs::remove_dir_all(root).unwrap();
}
