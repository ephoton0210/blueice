// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Older targets preserve native async-generator requests and closing.

use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use std::{fs, path::PathBuf, process::Command};

const SOURCE: &str = include_str!("fixtures/async_generator_targets/main.ts");

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn async_generator_targets_match_native_effects_declarations_and_syntax() {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", SOURCE)]),
        CompilerOptions {
            declaration: true,
            target: blueice_bluets::EcmaTarget::Es2017,
            libraries: Some(vec![blueice_bluets::EcmaTarget::Es2020]),
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
            "bluets-async-generator-target-oracle-{}",
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
    const options = {target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022,
        lib: ['lib.es2020.d.ts'], strict: true, skipLibCheck: true,
        declaration: true, outDir: root + '/ts'};
    const program = ts.createProgram([root + '/main.ts'], options);
    const diagnostics = ts.getPreEmitDiagnostics(program);
    if (diagnostics.length || program.emit().emitSkipped) throw new Error(ts.formatDiagnostics(diagnostics,
        {getCanonicalFileName: x => x, getCurrentDirectory: () => root, getNewLine: () => '\n'}));
    fs.renameSync(root + '/ts/main.js', root + '/reference.mjs');
    const expected = await import(pathToFileURL(root + '/reference.mjs').href);
    const actual = await import(pathToFileURL(root + '/actual.mjs').href);
    const pair = result => [result.value,result.done];
    async function observe(module) {
        const observations = [], iterator = module.sequence();
        observations.push((await Promise.all([iterator.next(),iterator.next()])).map(pair));
        observations.push(pair(await iterator.return('stop')),module.count());
        const before = module.sequence();
        observations.push(pair(await before.return('before')),module.count());
        const thrown = module.sequence();
        try { await thrown.throw('before'); } catch(error) { observations.push(error); }
        observations.push(pair(await thrown.next()),module.count());
        const completed = module.sequence();
        observations.push([pair(await completed.next()),pair(await completed.next()),pair(await completed.next())]);
        observations.push(pair(await completed.return('after')));
        try { await completed.throw('after'); } catch(error) { observations.push(error); }
        observations.push(pair(await completed.next()),module.count());
        const recovery = module.recovery();
        observations.push(pair(await recovery.next()));
        observations.push(pair(await recovery.return(Promise.reject('rejected-return'))));
        observations.push(pair(await recovery.next()));
        return observations;
    }
    const normative = [[[20,false],[22,false]],['stop',true],1,['before',true],1,
        'before',[null,true],1,[[20,false],[22,false],['done',true]],['after',true],
        'after',[null,true],2,[1,false],[42,false],['done',true]];
    for (const module of [expected,actual]) {
        const observed = await observe(module);
        if (JSON.stringify(observed)!==JSON.stringify(normative)) throw new Error(JSON.stringify(observed));
    }
    const declaration = fs.readFileSync(root + '/ts/main.d.ts','utf8').replaceAll('\r\n','\n');
    const actualDeclaration = fs.readFileSync(root + '/actual.d.ts','utf8').replaceAll('\r\n','\n');
    if (actualDeclaration !== declaration)
        throw new Error(JSON.stringify({actualDeclaration, expectedDeclaration: declaration}));
    const syntax = require('child_process').spawnSync(process.execPath,
        [process.argv[2] + '/backend/bluets/tests/fixtures/oracle_support/assert_target_syntax.cjs',
         root + '/actual.mjs', 'ES2017', 'es2022'], {encoding:'utf8'});
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
