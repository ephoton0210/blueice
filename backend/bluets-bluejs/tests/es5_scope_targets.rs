// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ES5 scope lowering preserves shadowing, loop captures and lexical receivers.

use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use std::{fs, path::PathBuf, process::Command};

const SOURCE: &str = include_str!("fixtures/es5_scope_targets/main.ts");

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn es5_scope_targets_match_native_execution_declarations_and_syntax() {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", SOURCE)]),
        CompilerOptions {
            declaration: true,
            target: blueice_bluets::EcmaTarget::Es5,
            checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts["memory:///main.ts"];
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "bluets-es5-scope-target-oracle-{}",
            std::process::id()
        ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.ts"), SOURCE).unwrap();
    fs::write(root.join("actual.mjs"), &artifact.javascript).unwrap();
    fs::write(
        root.join("actual.d.ts"),
        artifact.declaration.as_ref().unwrap(),
    )
    .unwrap();
    let script = r#"
const fs = require('fs'), {pathToFileURL} = require('url');
const root = process.argv[1];
const ts = require(process.argv[2] + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE);
(async () => {
    const options = {target: ts.ScriptTarget.ES5, module: ts.ModuleKind.ES2022,
        strict: true, skipLibCheck: true, declaration: true, outDir: root + '/ts'};
    const program = ts.createProgram([root + '/main.ts'], options);
    const diagnostics = ts.getPreEmitDiagnostics(program);
    if (diagnostics.length || program.emit().emitSkipped) throw new Error(ts.formatDiagnostics(diagnostics,
        {getCanonicalFileName: x => x, getCurrentDirectory: () => root, getNewLine: () => '\n'}));
    fs.renameSync(root + '/ts/main.js', root + '/reference.mjs');
    const expected = await import(pathToFileURL(root + '/reference.mjs').href);
    const actual = await import(pathToFileURL(root + '/actual.mjs').href);
    const normative = [42,1,[20,22,0,1,2],42];
    for (const module of [expected,actual]) {
        const observed = [module.result,module.outside,module.observed(),module.receiver.call({value:42})];
        if (JSON.stringify(observed)!==JSON.stringify(normative)) throw new Error(JSON.stringify(observed));
    }
    const declaration = fs.readFileSync(root + '/ts/main.d.ts','utf8').replaceAll('\r\n','\n');
    const actualDeclaration = fs.readFileSync(root + '/actual.d.ts','utf8').replaceAll('\r\n','\n');
    if (actualDeclaration !== declaration)
        throw new Error(JSON.stringify({actualDeclaration, expectedDeclaration: declaration}));
    const syntax = require('child_process').spawnSync(process.execPath,
        [process.argv[2] + '/backend/bluets/tests/fixtures/oracle_support/assert_target_syntax.cjs',
         root + '/actual.mjs', 'ES5', 'es2022'], {encoding:'utf8'});
    if (syntax.status !== 0) throw new Error(syntax.stdout + syntax.stderr);
    process.stdout.write(JSON.stringify({version: ts.version, values: normative}));
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
