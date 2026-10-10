// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// K.7.2 live native verdict, source-origin and declaration replay.
const fs = require('fs');
const path = require('path');
const assert = require('assert');
const os = require('os');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const corpus = path.join(fixtures, 'module_systems');
const ts = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'))(
    process.env.BLUEICE_BLUETSC_ORACLE || 'tsc',
);
const reference = JSON.parse(fs.readFileSync(path.join(corpus, 'reference.json'), 'utf8'));
const observe = require(path.join(corpus, 'observe.cjs'));
assert.strictEqual(ts.version, '5.9.3');
assert.strictEqual(reference.typescript, ts.version);
const libraries = path.dirname(ts.getDefaultLibFilePath({}));
const parsedLibraries = new Map();
const verdicts = [];
let accepts = 0;
async function replay(temporary) {
for (const item of reference.cases) {
    const directory = path.join(corpus, item.id);
    const file = path.join(directory, 'tsconfig.json');
    const read = ts.readConfigFile(file, ts.sys.readFile);
    assert.strictEqual(read.error, undefined, item.id);
    const parsed = ts.parseJsonConfigFileContent(read.config, ts.sys, directory);
    assert.deepStrictEqual(parsed.errors, [], item.id);
    const options = parsed.options;
    const host = ts.createCompilerHost(options);
    const original = host.getSourceFile;
    host.getSourceFile = (file, version, onError, createNew) => {
        if (path.dirname(file) !== libraries) return original(file, version, onError, createNew);
        const key = `${file}:${JSON.stringify(version)}`;
        if (!parsedLibraries.has(key)) {
            parsedLibraries.set(key, original(file, version, onError, createNew));
        }
        return parsedLibraries.get(key);
    };
    const outputs = new Map();
    host.writeFile = (file, text) => outputs.set(path.basename(file), text);
    const program = ts.createProgram(parsed.fileNames, options, host);
    const diagnostics = ts.getPreEmitDiagnostics(program).map(diagnostic => {
        const result = {
            code: diagnostic.code, start: diagnostic.start, length: diagnostic.length,
            message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
        };
        // The helper controls were independently recorded as single-file
        // observations; the other families retain their recorded file origin.
        if (item.family !== 'helper-options') result.file = path.basename(diagnostic.file.fileName);
        return result;
    });
    assert.deepStrictEqual(diagnostics, item.reference.diagnostics, `${item.id}: diagnostics`);
    verdicts.push(`${item.id}/${item.entry}\t${diagnostics.length ? 'reject' : 'accept'}\n`);
    const emitted = program.emit();
    if (diagnostics.length) {
        assert.strictEqual(emitted.emitSkipped, true, item.id);
        assert.strictEqual(outputs.size, 0, item.id);
        continue;
    }
    accepts += 1;
    assert.strictEqual(emitted.emitSkipped, false, item.id);
    const declarations = Object.fromEntries([...outputs].filter(([name]) => /\.d\.(?:ts|mts|cts)$/.test(name)));
    const expected = item.family === 'helper-options'
        ? {'main.d.ts': item.reference.declaration} : item.reference.declarations;
    assert.deepStrictEqual(declarations, expected, `${item.id}: declarations`);
    if (item.family === 'per-file') {
        assert.deepStrictEqual([...outputs.keys()].sort(), [...item.reference.files].sort(), `${item.id}: suffixes`);
    }
    if (item.family === 'helper-options') {
        assert.strictEqual(outputs.get('main.js').includes('"tslib"'), item.reference.importsRuntime, item.id);
    }
    const copied = path.join(temporary, item.id);
    fs.cpSync(directory, copied, {recursive:true});
    const out = path.join(copied, 'out');
    fs.mkdirSync(out);
    for (const [name, text] of outputs) fs.writeFileSync(path.join(out, name), text);
    const actual = JSON.parse(JSON.stringify(await observe(out, item)));
    const expectedObservation = item.family === 'module' ? {observations:item.reference.observations}
        : item.family === 'per-file' ? {stdout:item.reference.stdout}
        : item.family === 'helper-options' ? {observation:item.reference.observation, missingGlobal:item.reference.missingGlobal}
        : {observation:item.reference.observation};
    assert.deepStrictEqual(actual, expectedObservation, `${item.id}: execution`);
}
assert.strictEqual(reference.cases.length, 100);
assert.strictEqual(accepts, 82);
const matrix = verdicts.join('');
const matrixFile = path.join(corpus, 'module-systems-checker-matrix.tsv');
if (process.env.BLUEICE_WRITE_MODULE_SYSTEMS_MATRIX === '1') {
    fs.writeFileSync(matrixFile, matrix);
} else {
    assert.strictEqual(fs.readFileSync(matrixFile, 'utf8').replaceAll('\r\n', '\n'), matrix);
}
process.stdout.write(JSON.stringify({typescript: ts.version, cases:100, accepts, rejects:18}) + '\n');
}
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-native-modules-'));
replay(temporary).catch(error => {console.error(error); process.exitCode = 1;})
    .finally(() => fs.rmSync(temporary, {recursive:true, force:true}));
