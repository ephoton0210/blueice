// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Reproduce supplemental Native callback expectations without rewriting them.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {createHash} = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
const ts = require(implementation);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const directory = path.join(repo, 'backend/bluets/tests/fixtures/mapped_inference');
const filename = process.argv[2] || 'callback-context-controls.json';
assert.deepEqual(process.argv.slice(2), process.argv[2] ? [filename] : []);
assert(['callback-context-controls.json', 'generic-argument-controls.json'].includes(filename));
const input = fs.readFileSync(path.join(directory, filename));
const fixture = JSON.parse(input), frozen = JSON.parse(fs.readFileSync(path.join(directory, 'FROZEN.json')));
assert.equal(ts.version, '5.9.3');
assert.equal(sha(input), frozen.supplementarySha256[filename]);
assert.equal(sha(fs.readFileSync(implementation)), fixture.implementationSha256);
const expectedCases = filename === 'callback-context-controls.json' ? 4 : 12;
assert.equal(fixture.cases.length, expectedCases);
const parent = process.env.BLUEICE_MAPPED_INFERENCE_OBSERVATIONS || os.tmpdir();
fs.mkdirSync(parent, {recursive: true});
const root = fs.mkdtempSync(path.join(parent, filename === 'callback-context-controls.json' ? 'native-callback-context-' : 'native-generic-arguments-'));
const report = {sourceRevision: process.env.BLUEICE_MAPPED_INFERENCE_SOURCE_REVISION || null,
    implementationSha256: fixture.implementationSha256, inputSha256: sha(input),
    libraries: {}, cases: [], inventoryCredit: 0, conclusion: 'in_progress'};
const save = () => fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
function diagnostic(row, cwd) {
    const point = row.file && row.start !== undefined ? row.file.getLineAndCharacterOfPosition(row.start) : null;
    return {code: row.code, message: ts.flattenDiagnosticMessageText(row.messageText, '\n'),
        file: row.file ? path.relative(cwd, row.file.fileName).replaceAll('\\', '/') : null,
        position: point ? {line: point.line + 1, column: point.character + 1, length: row.length} : null,
        relatedInformation: (row.relatedInformation || []).map(value => diagnostic(value, cwd))};
}
try {
    save();
    for (const row of fixture.cases) {
        const cwd = path.join(root, row.id); fs.mkdirSync(cwd);
        const source = path.join(cwd, 'main.ts'); fs.writeFileSync(source, row.source);
        const options = ts.convertCompilerOptionsFromJson(fixture.options, cwd);
        assert.deepEqual(options.errors, []);
        const program = ts.createProgram([source], options.options);
        for (const file of program.getSourceFiles()) {
            if (path.dirname(file.fileName) !== path.dirname(implementation)) continue;
            const name = path.basename(file.fileName), digest = sha(fs.readFileSync(file.fileName));
            assert.equal(digest, fixture.libraries[name], 'unpinned library: ' + name);
            report.libraries[name] = digest;
        }
        const diagnostics = ts.getPreEmitDiagnostics(program).map(value => diagnostic(value, cwd));
        const declarations = {};
        program.emit(undefined, (file, text) => {
            fs.mkdirSync(path.dirname(file), {recursive: true}); fs.writeFileSync(file, text);
            if (file.endsWith('.d.ts')) declarations[path.relative(path.join(cwd, 'dist'), file).replaceAll('\\', '/')] = text;
        });
        const observed = {id: row.id, accepts: diagnostics.length === 0, diagnostics, declarations};
        fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(observed, null, 2) + '\n');
        assert.deepEqual(observed, {id: row.id, accepts: row.accepts, diagnostics: row.diagnostics, declarations: row.declarations});
        report.cases.push(observed); save();
    }
    assert.equal(sha(fs.readFileSync(path.join(directory, filename))), report.inputSha256);
    report.conclusion = 'success'; save(); console.log(JSON.stringify({root, comparisons: expectedCases, inventoryCredit: 0}));
} catch (error) {
    report.conclusion = 'failure'; report.error = String(error.stack || error); save(); throw error;
}
