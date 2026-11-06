// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Read-only complete module verifier. All compiler products are pre-existing immutable observations.
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto'), assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const loadTypeScript = require(path.join(support, 'load_typescript.cjs'));
const resolvePackageFile = require(path.join(support, 'resolve_package.cjs'));
const oracle = process.env.BLUEICE_BLUETSC_ORACLE || 'tsc';
const ts = loadTypeScript(oracle);
const tools = __dirname + '/';
const { stream } = require(tools + 'ge5_role_stream.cjs');
const { compare } = require('./ge5_k11_standard_module.cjs'), controls = require('./ge5_k11_standard_controls.cjs');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) { assert(process.argv[index].startsWith('--') && process.argv[index + 1]); args.set(process.argv[index].slice(2), process.argv[index + 1]); }
for (const name of ['report', 'output-dir', 'compiler', 'compiler-sha256', 'source-snapshot']) assert(args.has(name), 'required --' + name);
const out = path.resolve(args.get('output-dir')); assert(!fs.existsSync(out), 'fresh output directory'); fs.mkdirSync(out);
const sha = value => crypto.createHash('sha256').update(value).digest('hex'), fileSha = file => sha(fs.readFileSync(file));
const reportPath = path.resolve(args.get('report')), reportBytes = fs.readFileSync(reportPath), report = JSON.parse(reportBytes);
assert.equal(report.compilerSha256, args.get('compiler-sha256')); assert.equal(fileSha(args.get('compiler')), args.get('compiler-sha256'));
const selected = new Set(['k11-standard-method', 'k11-compacted-relocation', 'k11-erased-relocation', 'k11-composed-decorators']);
const rows = report.observations.filter(row => selected.has(row.row.case)); assert.equal(rows.length, selected.size);
assert.equal(new Set(rows.map(row => row.row.case)).size, selected.size);
const inputs = new Map();
function bind(file) { file = path.resolve(file); inputs.set(file, fileSha(file)); }
bind(reportPath); bind(args.get('compiler')); bind(args.get('source-snapshot')); bind(resolvePackageFile(oracle, 'typescript', 'lib/typescript.js'));
const closure = new Set();
function toolClosure(file) {
  file = path.resolve(file); if (closure.has(file)) return; closure.add(file); bind(file);
  for (const match of fs.readFileSync(file, 'utf8').matchAll(/require\(['"]([^'"]+)['"]\)/g)) {
    if (!match[1].startsWith('.')) continue;
    let child = match[1].startsWith('.') ? path.resolve(path.dirname(file), match[1]) : match[1];
    if (!path.extname(child)) child += '.cjs'; toolClosure(child);
  }
}
for (const file of [__filename, 'ge5_k11_standard_module.cjs', 'ge5_decorator_member_plan.cjs', 'ge5_k11_standard_controls.cjs']) toolClosure(path.resolve(__dirname, file));
for (const file of ['ge5_decorator_helpers.cjs', 'ge5_class_decorator_binding.cjs', 'ge5_decorator_replacement_frame.cjs', 'ge5_class_expression_frame.cjs', 'ge5_class_accounting.cjs', 'ge5_role_stream.cjs', 'ge5_decorator_preserved.cjs', 'ge5_role_mutations.cjs']) toolClosure(path.join(tools, file));
toolClosure(path.join(support, 'load_typescript.cjs'));
bind(loadTypeScript.cliPath(oracle)); bind(resolvePackageFile(oracle, 'typescript', 'package.json'));
for (const file of ['class_helpers.v1.js', 'spread_helpers.v1.js', 'iteration_helpers.v1.js']) bind(path.resolve(__dirname, '../../../../backend/bluets/src/emitter/targets', file));
for (const row of rows) {
  bind(path.join(row.directory, 'main.ts'));
  assert.equal(row.modes.length, 2); assert.deepEqual(row.modes.map(mode => mode.inlineSources).sort(), [false, true]);
  for (const mode of row.modes) for (const product of Object.values(mode.products)) for (const file of ['main.ts', 'tsconfig.json', 'out/main.js', 'out/main.js.map']) bind(path.join(product.project, file));
}
const observations = [], started = new Date().toISOString();
fs.writeFileSync(path.join(out, 'input-binding.json'), JSON.stringify({ compilerSha256: report.compilerSha256, sourceSnapshot: args.get('source-snapshot'), inputs: Object.fromEntries(inputs) }, null, 2) + '\n');
for (const row of rows) for (const mode of row.modes) {
  const source = fs.readFileSync(path.join(row.directory, 'main.ts'), 'utf8');
  function make(product) {
    assert.equal(product.compile.exit, 0); assert.equal(product.compile.signal, null);
    assert.equal(fs.readFileSync(path.join(product.project, 'main.ts'), 'utf8'), source, 'same physical original source');
    const code = fs.readFileSync(path.join(product.project, 'out/main.js'), 'utf8'); assert.equal(code, product.code, 'raw JavaScript agrees with bound observation');
    return stream(ts, code, fs.readFileSync(path.join(product.project, 'out/main.js.map'), 'utf8'));
  }
  const native = make(mode.products.native), blue = make(mode.products.blue), options = { ...mode.products.native.options, useDefineForClassFields: true };
  const item = { case: row.row.case, inlineSources: mode.inlineSources, options, sourceSha256: sha(source),
    native: { javascriptSha256: sha(native.code), mapSha256: sha(native.raw), characters: native.code.length, runtime: mode.products.native.runtime },
    blue: { javascriptSha256: sha(blue.code), mapSha256: sha(blue.raw), characters: blue.code.length, runtime: mode.products.blue.runtime }, positiveAccepted: false, controlsComplete: false };
  try {
    const proof = compare(ts, native, blue, options, source); item.positiveAccepted = true;
    item.proof = { tokenPairs: proof.nativeTokens.length, logicalEndpoints: proof.endpoints, originalFunctions: proof.inventory.functions.map(fn => fn.name.text),
      originalClasses: proof.classProofs.map(p => ({ name: p.source.name.text, methods: p.source.members.map(m => m.name.text), classDecorators: ts.getDecorators(p.source)?.length || 0, memberDecorators: p.source.members.map(m => ts.getDecorators(m)?.length || 0) })),
      nativeCharacters: proof.nativeCharacters, blueCharacters: proof.actualCharacters, fullUTF16RolesAndGaps: proof.completeModuleRoles };
    item.controls = controls.run(ts, native, blue, options, source, proof); item.controlsComplete = true;
  } catch (error) { item.error = { message: error.message, stack: error.stack }; }
  observations.push(item);
  fs.writeFileSync(path.join(out, 'progress.jsonl'), JSON.stringify({ case: item.case, inlineSources: item.inlineSources, positiveAccepted: item.positiveAccepted, controlsComplete: item.controlsComplete, controls: item.controls?.length || 0, error: item.error?.message }) + '\n', { flag: 'a' });
  console.log(item.case, item.inlineSources, item.positiveAccepted && item.controlsComplete ? 'PASS ' + item.controls.length + ' controls' : 'FAIL ' + item.error?.message.split('\n')[0]);
}
for (const [file, hash] of inputs) assert.equal(fileSha(file), hash, 'immutable verifier input: ' + file);
const counts = { forms: rows.length, configurations: observations.length, positiveAccepted: observations.filter(row => row.positiveAccepted).length,
  controlsComplete: observations.filter(row => row.controlsComplete).length, effectiveRejectedControls: observations.reduce((total, row) => total + (row.controls?.length || 0), 0),
  fullModuleConfigurations: observations.filter(row => row.positiveAccepted && row.controlsComplete).length, failures: observations.filter(row => !row.positiveAccepted || !row.controlsComplete).length };
const result = { schema: 1, scope: 'Original unchanged four K11 standard decorator forms; complete Native-derived module proof and fresh effective controls',
  compilerSha256: report.compilerSha256, sourceSnapshot: args.get('source-snapshot'), sourceSnapshotSha256: fileSha(args.get('source-snapshot')),
  reportPath, reportSha256: sha(reportBytes), inputsUnchanged: true, readerClosureFiles: closure.size, started, finished: new Date().toISOString(), counts, observations };
fs.writeFileSync(path.join(out, 'verification.json'), JSON.stringify(result, null, 2) + '\n'); console.log(JSON.stringify(counts));
if (counts.failures) process.exitCode = 1;
