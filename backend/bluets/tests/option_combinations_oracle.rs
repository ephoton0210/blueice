// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Option combinations against pinned TypeScript (J.6.2, K.2.4): one program that uses
//! enums, a const enum, namespaces, classes with fields, private names, static
//! members and cross-module imports is built by `bluetsc` and by `tsc` under every
//! combination of the options that change emit, and both outputs must print the
//! same thing under Node. The options are `target`, the module system,
//! `useDefineForClassFields`, `preserveConstEnums`, `isolatedModules`, source maps,
//! declarations, noEmit and strict checking.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

const MATRIX: &str = include_str!("fixtures/option_combinations/options-checker-matrix.tsv");
#[derive(Clone)]
struct Combination {
    target: &'static str,
    module: &'static str,
    define: Option<bool>,
    preserve: bool,
    isolated: bool,
    source_map: bool,
    declaration: bool,
    no_emit: bool,
    strict: bool,
}
impl Combination {
    fn name(&self) -> String {
        format!(
            "options-{}-{}-define{}-preserve{}-isolated{}-map{}-declaration{}-noemit{}-strict{}",
            self.target,
            self.module,
            self.define.map_or("default", |v| if v { "1" } else { "0" }),
            u8::from(self.preserve),
            u8::from(self.isolated),
            u8::from(self.source_map),
            u8::from(self.declaration),
            u8::from(self.no_emit),
            u8::from(self.strict)
        )
    }
    fn options(&self, directory: &str) -> Value {
        let mut options = json!({"target":self.target,"module":self.module,
            "isolatedModules":self.isolated,"sourceMap":self.source_map,
            "declaration":self.declaration,"noEmit":self.no_emit,"strict":self.strict,
            "skipLibCheck":true,"rewriteRelativeImportExtensions":true,"outDir":directory});
        if let Some(value) = self.define {
            options["useDefineForClassFields"] = json!(value);
        }
        // Absence preserves the isolatedModules implication; explicit false is invalid there.
        if self.preserve {
            options["preserveConstEnums"] = json!(true);
        }
        options
    }
}
fn combinations() -> Vec<Combination> {
    let mut all = Vec::new();
    for target in ["es2020", "es2022"] {
        for module in ["esnext", "commonjs"] {
            for define in [None, Some(true), Some(false)] {
                for preserve in [false, true] {
                    for isolated in [false, true] {
                        for source_map in [false, true] {
                            for declaration in [false, true] {
                                for no_emit in [false, true] {
                                    for strict in [false, true] {
                                        all.push(Combination {
                                            target,
                                            module,
                                            define,
                                            preserve,
                                            isolated,
                                            source_map,
                                            declaration,
                                            no_emit,
                                            strict,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    all
}
fn matrix() -> BTreeMap<String, bool> {
    MATRIX
        .lines()
        .map(|line| {
            let (name, verdict) = line.split_once('\t').unwrap();
            assert!(matches!(verdict, "accept" | "reject"));
            (name.to_string(), verdict == "accept")
        })
        .collect()
}
fn root(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("bluets-options-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(path.join("src/main.ts"), MAIN).unwrap();
    fs::write(path.join("src/lib.ts"), LIB).unwrap();
    fs::canonicalize(path).unwrap()
}
fn build(root: &Path, index: usize, combination: &Combination) -> PathBuf {
    let output = format!("blue-{index}");
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec(
            &json!({"compilerOptions":combination.options(&output),"files":["src/main.ts"]}),
        )
        .unwrap(),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(root)
        .args(["--project", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert_eq!(
        result.status.success(),
        matrix()[&combination.name()],
        "{}: {}",
        combination.name(),
        String::from_utf8_lossy(&result.stderr)
    );
    let directory = root.join(output);
    if combination.no_emit {
        assert!(
            !directory.exists(),
            "{}: noEmit wrote artifacts",
            combination.name()
        );
    } else {
        let expected = ["main", "lib"]
            .into_iter()
            .flat_map(|stem| {
                let mut paths = vec![format!("{stem}.js")];
                if combination.source_map {
                    paths.push(format!("{stem}.js.map"));
                }
                if combination.declaration {
                    paths.push(format!("{stem}.d.ts"));
                }
                paths
            })
            .collect::<BTreeSet<_>>();
        let actual = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            expected,
            actual,
            "{}: artifact inventory",
            combination.name()
        );
        for stem in ["main", "lib"] {
            let javascript = fs::read_to_string(directory.join(format!("{stem}.js"))).unwrap();
            assert_eq!(
                javascript.contains("//# sourceMappingURL="),
                combination.source_map,
                "{}: source map link",
                combination.name()
            );
            if combination.source_map {
                let map: Value = serde_json::from_slice(
                    &fs::read(directory.join(format!("{stem}.js.map"))).unwrap(),
                )
                .unwrap();
                assert_eq!(map["version"], 3);
                assert_eq!(map["sources"].as_array().unwrap().len(), 1);
                assert_eq!(
                    map["sourcesContent"][0],
                    if stem == "main" { MAIN } else { LIB }
                );
                assert!(!map["mappings"].as_str().unwrap().is_empty());
            }
        }
    }
    assert_eq!(fs::read_to_string(root.join("src/main.ts")).unwrap(), MAIN);
    assert_eq!(fs::read_to_string(root.join("src/lib.ts")).unwrap(), LIB);
    directory
}
#[test]
fn recorded_matrix_covers_the_complete_cartesian_product() {
    let all = combinations();
    assert_eq!(all.len(), 768);
    let names = all.iter().map(Combination::name).collect::<BTreeSet<_>>();
    assert_eq!(names.len(), 768);
    assert_eq!(names, matrix().into_keys().collect());
    let recorded: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/option_combinations/cases.json")).unwrap();
    let generated = all
        .iter()
        .enumerate()
        .map(|(index, c)| json!({"name":c.name(),"options":c.options(&format!("out-{index}"))}))
        .collect::<Vec<_>>();
    assert_eq!(
        generated, recorded,
        "recorded configurations must cover all generated options"
    );
}
#[test]
fn every_project_option_combination_replays_the_recorded_verdict() {
    let root = root("offline");
    for (index, combination) in combinations().iter().enumerate() {
        build(&root, index, combination);
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn every_emit_option_combination_prints_what_typescript_prints() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE");
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = root("oracle");
    let all = combinations();
    let rows = all
        .iter()
        .enumerate()
        .map(|(index, c)| json!({"name":c.name(),"options":c.options(&format!("tsc-{index}"))}))
        .collect::<Vec<_>>();
    fs::write(root.join("cases.json"), serde_json::to_vec(&rows).unwrap()).unwrap();
    let script = r#"
const path = require('path'), fs = require('fs');
const ts = require(process.argv[2])(process.argv[1]);
const results = [];
let previous;
for (const row of JSON.parse(fs.readFileSync('cases.json', 'utf8'))) {
 const converted = ts.convertCompilerOptionsFromJson(row.options, process.cwd());
 const program = ts.createProgram([path.resolve('src/main.ts')], converted.options, undefined, previous);
 previous = program;
 const diagnostics = [...converted.errors,...ts.getPreEmitDiagnostics(program)];
 const emitted = program.emit();
 diagnostics.push(...emitted.diagnostics);
 results.push([row.name,diagnostics.length === 0]);
 if (diagnostics.length) console.error(row.name,diagnostics.map(d=>ts.flattenDiagnosticMessageText(d.messageText,' ')));
}
fs.writeFileSync('verdicts.json',JSON.stringify(results));
"#;
    let reference = Command::new(&node)
        .current_dir(&root)
        .env("FORCE_COLOR", "0")
        .args(["-e", script])
        .arg(&tsc)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/oracle_support/load_typescript.cjs"
        ))
        .output()
        .unwrap();
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let verdicts: Vec<(String, bool)> =
        serde_json::from_slice(&fs::read(root.join("verdicts.json")).unwrap()).unwrap();
    let recorded = verdicts.into_iter().collect::<BTreeMap<_, _>>();
    if env::var_os("BLUEICE_WRITE_OPTIONS_MATRIX").is_some() {
        let text = recorded
            .iter()
            .map(|(name, accepted)| {
                format!("{name}\t{}\n", if *accepted { "accept" } else { "reject" })
            })
            .collect::<String>();
        fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/option_combinations/options-checker-matrix.tsv"),
            text,
        )
        .unwrap();
    } else {
        assert_eq!(
            recorded,
            matrix(),
            "pinned verdicts changed: {}",
            String::from_utf8_lossy(&reference.stderr)
        );
    }
    for (index, combination) in all.iter().enumerate() {
        let blue = build(&root, index, combination);
        let reference = root.join(format!("tsc-{index}"));
        if combination.no_emit {
            assert!(
                !reference.exists(),
                "{}: TypeScript noEmit artifacts",
                combination.name()
            );
            continue;
        }
        for directory in [&blue, &reference] {
            fs::write(
                directory.join("package.json"),
                if combination.module == "commonjs" {
                    r#"{"type":"commonjs"}"#
                } else {
                    r#"{"type":"module"}"#
                },
            )
            .unwrap();
        }
        let run = |directory: &Path| {
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
            "{}: {}",
            combination.name(),
            String::from_utf8_lossy(&blue_run.stderr)
        );
        assert!(
            tsc_run.status.success(),
            "{}: {}",
            combination.name(),
            String::from_utf8_lossy(&tsc_run.stderr)
        );
        assert_eq!(
            blue_run.stdout,
            tsc_run.stdout,
            "{}: Node output",
            combination.name()
        );
        if combination.declaration {
            for stem in ["main", "lib"] {
                assert_eq!(
                    fs::read_to_string(blue.join(format!("{stem}.d.ts"))).unwrap(),
                    fs::read_to_string(reference.join(format!("{stem}.d.ts"))).unwrap(),
                    "{}: {stem}.d.ts",
                    combination.name()
                );
            }
        }
        if combination.source_map {
            for stem in ["main", "lib"] {
                let map: Value = serde_json::from_slice(
                    &fs::read(reference.join(format!("{stem}.js.map"))).unwrap(),
                )
                .unwrap();
                assert_eq!(map["version"], 3);
                assert_eq!(map["sources"].as_array().unwrap().len(), 1);
                assert!(!map["mappings"].as_str().unwrap().is_empty());
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
