// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array rest arguments preserve source effects, method calls and reduce results.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;
use std::{fs, path::PathBuf, process::Command};

const SOURCE: &str = include_str!("fixtures/rest_array_parameters/main.ts");

#[test]
fn direct_functions_accept_array_rest_arguments() {
    for source in [
        "function first(...values: number[]): number { return values[0]; } const values = [42]; first(...values);",
        "function first(prefix: string, ...values: number[]): number { return values[0]; } const values = [42]; first('ok', ...values);",
    ] {
        let artifact = compile_direct_script(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(Vm::default().execute(&artifact.bytecode).unwrap(), Value::Number(42.0));
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn rest_arrays_match_node_and_declarations() {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", SOURCE)]),
        CompilerOptions {
            declaration: true,
            checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts["memory:///main.ts"];
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-rest-array-oracle-{}", std::process::id()));
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
    const options = {target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ES2022,
        strict: true, skipLibCheck: true, declaration: true, outDir: root + '/ts'};
    const program = ts.createProgram([root + '/main.ts'], options);
    const diagnostics = ts.getPreEmitDiagnostics(program);
    if (diagnostics.length || program.emit().emitSkipped) throw new Error(ts.formatDiagnostics(diagnostics,
        {getCanonicalFileName: x => x, getCurrentDirectory: () => root, getNewLine: () => '\n'}));
    fs.renameSync(root + '/ts/main.js', root + '/reference.mjs');
    const names = ['answer','sourceReads','member','arity'];
    const expected = await import(pathToFileURL(root + '/reference.mjs').href);
    const actual = await import(pathToFileURL(root + '/actual.mjs').href);
    const normative = [42,1,42,0];
    for (const module of [expected,actual]) if (JSON.stringify(names.map(name => module[name])) !== JSON.stringify(normative))
        throw new Error(JSON.stringify(names.map(name => module[name])));
    const declaration = fs.readFileSync(root + '/ts/main.d.ts','utf8').replaceAll('\r\n','\n');
    const actualDeclaration = fs.readFileSync(root + '/actual.d.ts','utf8').replaceAll('\r\n','\n');
    if (actualDeclaration !== declaration)
        throw new Error(JSON.stringify({actualDeclaration, expectedDeclaration: declaration}));
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
