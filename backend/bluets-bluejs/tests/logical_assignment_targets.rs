// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Logical assignments preserve their binding reads, effects and suspension.

use blueice_bluejs::{parse_module_with_edition, SyntaxEdition};
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = include_str!("fixtures/logical_assignment_targets/main.ts");

fn emitted(target: EcmaTarget) -> String {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", SOURCE)]),
        CompilerOptions {
            target,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    result.output.unwrap().artifacts["memory:///main.ts"]
        .javascript
        .clone()
}

#[test]
fn emitted_logical_assignments_parse_at_the_selected_bluejs_edition() {
    for (target, edition) in [
        (EcmaTarget::Es2020, SyntaxEdition::Es2020),
        (EcmaTarget::Es2021, SyntaxEdition::Es2021),
    ] {
        let source = emitted(target);
        parse_module_with_edition(&source, edition)
            .unwrap_or_else(|error| panic!("{target:?}: {error:?}\n{source}"));
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn binding_effects_match_pinned_typescript_and_native_assignments() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "bluets-logical-target-oracle-{}",
            std::process::id()
        ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), SOURCE).unwrap();
    fs::write(root.join("blue20.mjs"), emitted(EcmaTarget::Es2020)).unwrap();
    fs::write(root.join("native21.mjs"), emitted(EcmaTarget::Es2021)).unwrap();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"
const fs = require('fs'), path = require('path'), {pathToFileURL} = require('url');
const ts = require(process.argv[2] + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const root = process.argv[1];
async function observations(file) {
    const m = await import(pathToFileURL(file).href);
    let calls = 0, reads = 0, writes = 0, stored;
    globalThis.effect = () => 20 + ++calls;
    const local = m.operate();
    calls = 0;
    const short = m.conditional(false);
    const shortCalls = calls;
    const nested = m.conditional(true);
    const nestedCalls = calls;
    const values = [];
    for (const value of [0, 7, null, undefined]) {
        calls = reads = writes = 0;
        stored = value;
        Object.defineProperty(globalThis, 'provided', {configurable: true,
            get() { reads++; return stored; }, set(value) { writes++; stored = value; }});
        values.push([m.nullish(), reads, writes, calls]);
    }
    calls = reads = writes = 0;
    stored = undefined;
    globalThis.effect = () => { calls++; throw 'effect'; };
    let abrupt;
    try { m.nullish(); } catch (error) { abrupt = error; }
    return [local, short, shortCalls, nested, nestedCalls, values,
        [abrupt, reads, writes, calls], m.collision(), await m.suspended()];
}
(async () => {
    const input = path.join(root, 'main.ts');
    const options = {target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ES2022,
        lib: ['lib.es2020.d.ts'], strict: true, skipLibCheck: true, outDir: root + '/ts'};
    const program = ts.createProgram([input], options);
    const diagnostics = ts.getPreEmitDiagnostics(program);
    if (diagnostics.length) throw new Error(ts.formatDiagnostics(diagnostics,
        {getCanonicalFileName: x => x, getCurrentDirectory: () => root, getNewLine: () => '\n'}));
    if (program.emit().emitSkipped) throw new Error('Skipped emit');
    fs.renameSync(root + '/ts/main.js', root + '/reference.mjs');
    const expected = await observations(root + '/reference.mjs');
    const native = await observations(root + '/native21.mjs');
    const actual = await observations(root + '/blue20.mjs');
    if (JSON.stringify(expected) !== JSON.stringify(native)
        || JSON.stringify(expected) !== JSON.stringify(actual)) throw new Error(JSON.stringify({expected,native,actual}));
    const normative = [[21,22],[7,0],0,[21,21],1,
        [[0,1,0,0],[7,1,0,0],[21,1,1,1],[21,1,1,1]],
        ['effect',1,0,1],[17,42],42];
    if (JSON.stringify(actual) !== JSON.stringify(normative)) throw new Error(JSON.stringify(actual));
    process.stdout.write(JSON.stringify({version: ts.version, observations: actual}));
})().catch(error => { console.error(error); process.exitCode = 1; });
"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(repository)
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
