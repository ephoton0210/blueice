// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every pinned accepted target program reaches the real BlueJS edition gate.

use blueice_bluejs::{parse_module_with_edition, SyntaxEdition};
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path, process::Command};

const OPAQUE_GENERATOR: &str = "export function* sequence(): Generator<number, void, unknown> { switch (1) { case 1: break; }; yield 42; }";

fn opaque_javascript() -> String {
    javascript_for(OPAQUE_GENERATOR)
}

fn javascript_for(source: &str) -> String {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            target: EcmaTarget::Es5,
            libraries: Some(vec![EcmaTarget::Es2020]),
            module_kind: ModuleKind::CommonJs,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.output.unwrap().artifacts["memory:///main.ts"]
        .javascript
        .clone()
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn switch_local_binding_survives_a_later_generator_resume() {
    let source = "export function* stored(): Generator<number, number | undefined, unknown> { switch (1) { case 1: var value: number | undefined = 7; break; } yield 1; return value; }";
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "bluets-switch-generator-local-{}",
            std::process::id()
        ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), source).unwrap();
    fs::write(root.join("blue.js"), javascript_for(source)).unwrap();
    let script = r#"
const path = require('node:path'), assert = require('node:assert/strict');
const root = process.argv[1], ts = require(process.argv[2] + '/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE);
const program = ts.createProgram([path.join(root,'main.ts')], {target:ts.ScriptTarget.ES5, module:ts.ModuleKind.CommonJS, lib:['lib.es2020.d.ts'], strict:true, skipLibCheck:true, noEmitOnError:true, outDir:path.join(root,'native')});
assert.equal(ts.getPreEmitDiagnostics(program).length, 0); assert.equal(program.emit().emitSkipped, false);
for (const filename of ['native/main.js','blue.js']) {
  const iterator = require(path.join(root,filename)).stored();
  assert.deepEqual([iterator.next(),iterator.next()], [{value:1,done:false},{value:7,done:true}], filename);
}

"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets"))
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn opaque_suspension_graph_emits_valid_es5_output() {
    parse_module_with_edition(&opaque_javascript(), SyntaxEdition::Es5).unwrap();
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn lowered_suspension_matches_pinned_native_execution() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-opaque-generator-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), OPAQUE_GENERATOR).unwrap();
    fs::create_dir_all(root.join("out")).unwrap();
    fs::write(root.join("out/blue.js"), opaque_javascript()).unwrap();
    let script = r#"
const path = require('path');
const ts = require(process.argv[2] + '/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
if (ts.version !== '5.9.3') throw new Error('oracle version differs');
const root = process.argv[1];
const program = ts.createProgram([path.join(root,'main.ts')], {target:ts.ScriptTarget.ES5, module:ts.ModuleKind.CommonJS, lib:['lib.es2020.d.ts'], strict:true, skipLibCheck:true, noEmitOnError:true, outDir:path.join(root,'out')});
const diagnostics = ts.getPreEmitDiagnostics(program);
if (diagnostics.length || program.emit().emitSkipped) throw new Error(ts.formatDiagnostics(diagnostics,{getCanonicalFileName:x=>x,getCurrentDirectory:()=>root,getNewLine:()=> '\n'}));
const native=require(path.join(root,'out/main.js')).sequence();
const blue=require(path.join(root,'out/blue.js')).sequence();
const expected=[native.next(),native.next()];
const actual=[blue.next(),blue.next()];
if(JSON.stringify(expected)!=='[{"value":42,"done":false},{"done":true}]')throw Error(JSON.stringify(expected));
if(JSON.stringify(actual)!==JSON.stringify(expected))throw Error(JSON.stringify(actual));
"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets"))
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn every_emitted_target_parses_at_its_selected_bluejs_edition() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../bluets/tests/fixtures/typescript_oracle/targets-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets/tests/fixtures/typescript_oracle");
    let mut compared = 0;
    let mut targets = BTreeSet::new();
    let mut failures = Vec::new();
    for case in reference["cases"].as_array().unwrap() {
        if case["accepts"] != true {
            continue;
        }
        let entry = case["entry"].as_str().unwrap();
        let target = EcmaTarget::parse(case["target"].as_str().unwrap()).unwrap();
        let edition = match target {
            EcmaTarget::Es5 => SyntaxEdition::Es5,
            EcmaTarget::Es2015 => SyntaxEdition::Es2015,
            EcmaTarget::Es2016 => SyntaxEdition::Es2016,
            EcmaTarget::Es2017 => SyntaxEdition::Es2017,
            EcmaTarget::Es2018 => SyntaxEdition::Es2018,
            EcmaTarget::Es2019 => SyntaxEdition::Es2019,
            EcmaTarget::Es2020 => SyntaxEdition::Es2020,
            EcmaTarget::Es2021 => SyntaxEdition::Es2021,
            EcmaTarget::Es2022 => SyntaxEdition::Es2022,
            EcmaTarget::Es2023 => SyntaxEdition::Es2023,
            EcmaTarget::EsNext => SyntaxEdition::EsNext,
        };
        let flags = case["flags"].as_array().unwrap();
        let flag = |name: &str| {
            flags
                .iter()
                .position(|value| value == name)
                .and_then(|index| flags.get(index + 1).and_then(Value::as_str))
        };
        let options = CompilerOptions {
            checking: Some(CheckingOptions::default()),
            target,
            libraries: Some(
                flag("--lib")
                    .unwrap()
                    .split(',')
                    .map(|name| EcmaTarget::parse(name).unwrap())
                    .collect(),
            ),
            module_kind: if flag("--module").unwrap() == "commonjs" {
                ModuleKind::CommonJs
            } else {
                ModuleKind::Esm
            },
            downlevel_iteration: flag("--downlevelIteration") == Some("true"),
            declaration: true,
            ..CompilerOptions::default()
        };
        let source = fs::read_to_string(root.join(entry)).unwrap();
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            options,
        );
        if result.has_errors() {
            failures.push(format!("{entry}: compile failed: {:?}", result.diagnostics));
            continue;
        }
        let output = result.output.unwrap();
        let artifact = &output.artifacts["memory:///main.ts"];
        if let Err(error) = parse_module_with_edition(&artifact.javascript, edition) {
            failures.push(format!("{entry}: {error:?}"));
        }
        compared += 1;
        targets.insert(target.as_str());
    }
    assert_eq!(compared, 600, "{}", failures.join("\n"));
    assert_eq!(targets.len(), 11);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn nested_binary_and_short_circuit_suspensions_preserve_native_resume_order() {
    let source = "export function* binary(): Generator<number, number, number> { return (yield 1) + (yield 2) * 2; } export function* logical(): Generator<number, number, number> { const first: number = 0 && (yield 9); const second: number = first || (yield 1); return second; }";
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "bluets-expression-generator-resume-{}",
            std::process::id()
        ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), source).unwrap();
    fs::write(root.join("blue.js"), javascript_for(source)).unwrap();
    let script = r#"
const path = require('node:path'), assert = require('node:assert/strict');
const root = process.argv[1], ts = require(process.argv[2] + '/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE);
const program = ts.createProgram([path.join(root,'main.ts')], {target:ts.ScriptTarget.ES5, module:ts.ModuleKind.CommonJS, lib:['lib.es2020.d.ts'], strict:true, skipLibCheck:true, noEmitOnError:true, outDir:path.join(root,'native')});
assert.equal(ts.getPreEmitDiagnostics(program).length, 0); assert.equal(program.emit().emitSkipped, false);
for (const filename of ['native/main.js','blue.js']) {
  const module = require(path.join(root,filename)), binary = module.binary(), logical = module.logical();
  assert.deepEqual([binary.next(),binary.next(10),binary.next(3),binary.next()], [{value:1,done:false},{value:2,done:false},{value:16,done:true},{value:undefined,done:true}], filename + ': arithmetic');
  assert.deepEqual([logical.next(),logical.next(42),logical.next()], [{value:1,done:false},{value:42,done:true},{value:undefined,done:true}], filename + ': short circuit');
}

"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets"))
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn async_joins_capture_receivers_and_propagate_rejections_like_native() {
    let input: Value = serde_json::from_str(include_str!(
        "../../bluets/tests/fixtures/ge1_downlevel/cases.json"
    ))
    .unwrap();
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "bluets-async-receiver-joins-{}",
            std::process::id()
        ));
    fs::create_dir(&root).unwrap();
    for (form, alternate) in [
        ("helper-name-collisions", false),
        ("nested-await-call", false),
        ("conditional-expression-await", false),
        ("conditional-expression-await", true),
        ("conditional-await", false),
        ("conditional-await", true),
    ] {
        let row = input["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["form"] == form)
            .unwrap();
        let source = row["files"]["main.ts"].as_str().unwrap();
        let source = if alternate {
            source.replace("resolve(true)", "resolve(false)")
        } else {
            source.to_owned()
        };
        let case = root.join(format!("{form}-{alternate}"));
        fs::create_dir(&case).unwrap();
        fs::write(case.join("main.ts"), &source).unwrap();
        fs::write(case.join("blue.js"), javascript_for(&source)).unwrap();
    }
    let script = r#"
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict');
const root = process.argv[1], ts = require(process.argv[2] + '/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE);
assert.equal(ts.version, '5.9.3');
function execute(code, PromiseType = Promise) {
  const exports = {}; vm.runInNewContext(code, {exports, Promise: PromiseType}); return exports.result;
}
(async () => {
for (const name of fs.readdirSync(root)) {
  const directory = path.join(root, name), program = ts.createProgram([path.join(directory,'main.ts')], {target:ts.ScriptTarget.ES5, module:ts.ModuleKind.CommonJS, lib:['lib.es2020.d.ts'], strict:true, skipLibCheck:true, noEmitOnError:true, outDir:path.join(directory,'native')});
  assert.equal(ts.getPreEmitDiagnostics(program).length, 0); assert.equal(program.emit().emitSkipped, false);
  for (const filename of ['native/main.js','blue.js']) {
    const code = fs.readFileSync(path.join(directory, filename), 'utf8'), label = name + '/' + filename;
    const expected = name.endsWith('-true') ? 9 : name.startsWith('conditional-await-') ? 3 : 42;
    assert.equal(await execute(code), expected, label);
    if (!name.startsWith('nested-await-call')) continue;
    for (const reject of [false, true]) {
      let reads = 0; const calls = [], failure = new Error('inner rejection');
      class TrackedPromise extends Promise {}
      Object.defineProperty(TrackedPromise, 'resolve', {get() {
        const read = ++reads;
        return function(value) {
          assert.equal(this, TrackedPromise, label + ': method receiver');
          if (read <= 2) {
            calls.push([read, value]);
            if (reject && read === 2) return Promise.reject(failure);
          }
          return Promise.resolve(value);
        };
      }});
      const result = execute(code, TrackedPromise);
      if (reject) { await assert.rejects(result, error => error === failure); assert.deepEqual(calls, [[2,42]], label + ': rejected argument skips captured invocation'); }
      else { assert.equal(await result, 42); assert.deepEqual(calls, [[2,42],[1,42]], label + ': capture outer getter before inner await'); }
    }
  }
}
})().catch(error => { console.error(error); process.exitCode = 1; });
"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets"))
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
