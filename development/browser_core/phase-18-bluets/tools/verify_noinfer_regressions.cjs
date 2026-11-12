// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Supplemental Native regressions retain their own four-program denominator.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
const ts = require(implementation), sha = bytes => createHash('sha256').update(bytes).digest('hex');
const directory = path.join(repo, 'backend/bluets/tests/fixtures/noinfer');
const original = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json')));
const bytes = fs.readFileSync(path.join(directory, 'keyof-diagnostics.json')), input = JSON.parse(bytes);
assert.equal(ts.version, '5.9.3'); assert.equal(input.typescript, ts.version);
assert.equal(sha(fs.readFileSync(implementation)), original.implementationSha256);
for (const [name, hash] of Object.entries(original.libraries)) assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), hash);
const compiler = fs.realpathSync(process.env.BLUEICE_NOINFER_COMPILER), compilerHash = sha(fs.readFileSync(compiler));
if (process.env.BLUEICE_GE5_EXPECTED_COMPILER_SHA256) assert.equal(compilerHash, process.env.BLUEICE_GE5_EXPECTED_COMPILER_SHA256);
const capture = process.env.BLUEICE_NOINFER_OBSERVATIONS;
const root = capture ? path.join(path.resolve(capture), 'regressions-' + process.pid) : fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-noinfer-regressions-'));
fs.mkdirSync(root, {recursive: true});
const report = {schema: 1, sourceRevision: process.env.BLUEICE_NOINFER_SOURCE_REVISION || null, compilerSha256: compilerHash,
  inputSha256: sha(bytes), implementationSha256: original.implementationSha256, libraries: original.libraries, cases: [], conclusion: 'in_progress'};
const save = () => fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
function native(d) {
  return {code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'), start: d.start, length: d.length,
    file: d.file ? path.basename(d.file.fileName) : null, relatedInformation: (d.relatedInformation || []).map(native)};
}
function blue(d, source) {
  d = d.typescript || d; const span = d.span, bytes = Buffer.from(source, 'utf8');
  return {code: d.code, message: d.message, start: bytes.subarray(0, span.start).toString('utf8').length,
    length: bytes.subarray(span.start, span.end).toString('utf8').length, file: span.module, relatedInformation: (d.relatedInformation || []).map(row => blue(row, source))};
}
try {
  save(); assert.equal(input.observations.length, 4);
  for (const expected of input.observations) {
    const cwd = path.join(root, expected.id); fs.mkdirSync(cwd);
    fs.writeFileSync(path.join(cwd, 'entry.ts'), expected.source);
    fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify({compilerOptions: {...input.options, outDir: 'dist'}, files: ['entry.ts']}));
    const options = ts.convertCompilerOptionsFromJson(input.options, cwd); assert.deepEqual(options.errors, []);
    const program = ts.createProgram([path.join(cwd, 'entry.ts')], {...options.options, noEmit: true});
    const observedNative = ts.getPreEmitDiagnostics(program).map(native); assert.deepEqual(observedNative, expected.diagnostics, expected.id + ': Native changed');
    const built = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], {cwd, encoding: 'utf8', timeout: 120000});
    fs.writeFileSync(path.join(cwd, 'stdout.log'), built.stdout || ''); fs.writeFileSync(path.join(cwd, 'stderr.log'), built.stderr || '');
    if (built.error) throw built.error; assert.equal(built.status, 1, expected.id);
    if (fs.existsSync(path.join(cwd, 'dist'))) assert.equal(fs.readdirSync(path.join(cwd, 'dist')).length, 0, 'rejected project published artifacts');
    const observedBlue = built.stderr.trim().split('\n').map(line => blue(JSON.parse(line), expected.source));
    assert.deepEqual(observedBlue, expected.diagnostics, expected.id + ': BlueTS differs');
    report.cases.push({id: expected.id, native: observedNative, blue: observedBlue, agrees: true}); save();
  }
  assert.equal(sha(fs.readFileSync(compiler)), compilerHash); assert.equal(sha(fs.readFileSync(path.join(directory, 'keyof-diagnostics.json'))), report.inputSha256);
  assert.equal(sha(fs.readFileSync(implementation)), original.implementationSha256);
  for (const [name, hash] of Object.entries(original.libraries)) assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), hash);
  report.conclusion = 'pass'; save(); console.log('Verified four independent named-key Native/BlueTS regressions; original 282-observation benchmark unchanged');
} catch (error) { report.conclusion = 'failure'; report.error = error.stack; save(); throw error; }
finally { if (!capture) fs.rmSync(root, {recursive: true, force: true}); }
