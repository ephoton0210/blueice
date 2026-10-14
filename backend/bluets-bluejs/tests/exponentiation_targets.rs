// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exponentiation keeps associativity, update effects and operand ordering.

use blueice_bluejs::{parse_module_with_edition, SyntaxEdition};
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
use std::{fs, path::PathBuf, process::Command};

const SOURCE: &str = include_str!("fixtures/exponentiation_targets/main.ts");

fn emitted(target: EcmaTarget) -> (String, String) {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", SOURCE)]),
        CompilerOptions {
            target,
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts["memory:///main.ts"];
    (
        artifact.javascript.clone(),
        artifact.declaration.clone().unwrap(),
    )
}

#[test]
fn power_effects_parse_at_the_selected_bluejs_edition() {
    for (target, edition) in [
        (EcmaTarget::Es2015, SyntaxEdition::Es2015),
        (EcmaTarget::Es2016, SyntaxEdition::Es2016),
    ] {
        let (source, _) = emitted(target);
        parse_module_with_edition(&source, edition)
            .unwrap_or_else(|error| panic!("{target:?}: {error:?}\n{source}"));
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn power_effects_match_pinned_typescript_and_native_exponentiation() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-power-target-oracle-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), SOURCE).unwrap();
    for (name, target) in [
        ("blue15", EcmaTarget::Es2015),
        ("native16", EcmaTarget::Es2016),
    ] {
        let (javascript, declaration) = emitted(target);
        fs::write(root.join(format!("{name}.mjs")), javascript).unwrap();
        fs::write(root.join(format!("{name}.d.ts")), declaration).unwrap();
    }
    let script = r#"
const fs = require('fs'), path = require('path'), {pathToFileURL} = require('url');
const root = process.argv[1];
const ts = require(process.argv[2] + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
async function observations(file) {
    const module = await import(pathToFileURL(file).href);
    let events = [];
    globalThis.operand = (name, value) => { events.push(name); return value; };
    globalThis.receiver = name => {
        events.push(name);
        return {read() { events.push(name + '.read'); return 2; },
            get exponent() { events.push(name + '.exponent'); return 3; }};
    };
    return ['associated','grouped','unary','negative','updated','members'].map(name => {
        events = [];
        return [module[name](), events.slice()];
    });
}
(async () => {
    const options = {target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.ES2022,
        strict: true, skipLibCheck: true, declaration: true, outDir: root + '/ts'};
    const program = ts.createProgram([root + '/main.ts'], options);
    const diagnostics = ts.getPreEmitDiagnostics(program);
    if (diagnostics.length || program.emit().emitSkipped) throw new Error(ts.formatDiagnostics(diagnostics,
        {getCanonicalFileName: x => x, getCurrentDirectory: () => root, getNewLine: () => '\n'}));
    fs.renameSync(root + '/ts/main.js', root + '/reference.mjs');
    const expected = await observations(root + '/reference.mjs');
    const native = await observations(root + '/native16.mjs');
    const actual = await observations(root + '/blue15.mjs');
    const normative = [[512,['left','middle','right']],[64,['left','middle','right']],
        [0.25,['left','right']],[-8,['left','right']],[16,[]],
        [8,['left','left.read','right','right.exponent']]];
    if ([expected,native,actual].some(value => JSON.stringify(value) !== JSON.stringify(normative)))
        throw new Error(JSON.stringify({expected,native,actual}));
    const declaration = fs.readFileSync(root + '/ts/main.d.ts','utf8').replaceAll('\r\n','\n');
    for (const file of ['blue15','native16']) {
        if (fs.readFileSync(root + '/' + file + '.d.ts','utf8').replaceAll('\r\n','\n') !== declaration)
            throw new Error(file + ': declarations differ');
    }
    process.stdout.write(JSON.stringify({version:ts.version, observations:actual}));
})().catch(error => { console.error(error); process.exitCode = 1; });
"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
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
