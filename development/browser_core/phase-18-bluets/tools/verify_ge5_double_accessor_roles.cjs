// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// All 67 frozen literal-method products retain complete modules and fresh negatives.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../../../..'), graph = require('./ge5_inputs.cjs');
const ts = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs')();
const {stream} = require('./ge5_role_stream.cjs'), authority = require('./ge5_native_authority.cjs');
const inputBytes = fs.readFileSync(root + '/backend/bluets/tests/fixtures/ge5_source_maps/cases.json');
const referenceBytes = fs.readFileSync(root + '/backend/bluets/tests/fixtures/ge5_source_maps/reference.json');
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const parentReference = require(root + '/backend/bluets/tests/fixtures/ge3_decorator_compositions/reference.json');
const compiler = process.argv[2], compilerSha256 = graph.sha256(fs.readFileSync(compiler));
assert.equal(compilerSha256, process.argv[3], 'explicit immutable compiler capability');
const observations = process.env.BLUEICE_GE5_EXISTING_OBSERVATIONS
  ? JSON.parse(fs.readFileSync(process.env.BLUEICE_GE5_EXISTING_OBSERVATIONS))
  : require('./ge5_double_accessor_observe.cjs').collect(compiler, compilerSha256,
    {form: 'ge3_decorator_compositions/rejected-double-decorated-accessors'});
const selected = input.cases.flatMap((row, index) => row.form === 'ge3_decorator_compositions/rejected-double-decorated-accessors' ? [index] : []);
assert.equal(selected.length, 35); assert.deepEqual(observations.map(row => row.index), selected);
const hashes = new Map();
function dependency(name) {
  if (hashes.has(name)) return;
  const bytes = fs.readFileSync(path.join(__dirname, name)); hashes.set(name, graph.sha256(bytes));
  for (const match of bytes.toString().matchAll(/require\(['"]\.\/([^'"]+\.cjs)['"]\)/g)) dependency(match[1]);
}
dependency(path.basename(__filename));
const verified = [], failures = [], executions = [], positiveOnly = process.env.BLUEICE_GE5_POSITIVE_ONLY === '1';
for (const row of observations) {
  const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  assert.equal(row.id, expected.id); assert.deepEqual(row.options, original.options); assert.deepEqual(row.files, original.files);
  assert.deepEqual(Object.keys(row.outputs).sort(), Object.keys(expected.outputs).sort());
  for (const [name, text] of Object.entries(row.outputs)) if (/\.d\.(?:ts|mts|cts)(?:\.map)?$/.test(name)) {
    const wanted = reference.blobs[expected.outputs[name]];
    if (name.endsWith('.map')) {
      const maps = require('./ge5_maps.cjs');
      assert.deepEqual(maps.decode(text, row.outputs[name.slice(0, -4)]),
        maps.decode(wanted, reference.blobs[expected.outputs[name.slice(0, -4)]]));
    } else assert.equal(text, wanted, 'raw declaration artifact');
  }
  for (const [name, map] of Object.entries(expected.maps)) if (map.kind === 'javascript') try {
    const code = row.outputs[map.output], raw = name.endsWith('#inline')
      ? Buffer.from(/sourceMappingURL=data:[^,]+,([^\s]+)/.exec(code)[1], 'base64').toString() : row.outputs[name];
    const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name));
    const actual = stream(ts, code, raw);
    const proof = require('./ge5_decorator_member_module.cjs').compare(ts, native, actual, row.options, row.files['main.ts']);
    const controls = positiveOnly ? [] : require('./ge5_double_accessor_mutations.cjs')
      .run(ts, native, actual, row.options, row.files['main.ts'], proof);
    verified.push({index: row.index, id: row.id, map: name, allCharactersClassified: true,
      actualJavaScriptSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw),
      nativeJavaScriptSha256: expected.outputs[map.output], nativeMapSha256: graph.sha256(native.raw), controls});
  } catch (error) { failures.push({index: row.index, id: row.id, map: name, error: error.message, stack: error.stack}); }
  if (!positiveOnly) try {
    const parent = parents.ge3_decorator_compositions.byId.get(input.cases[row.index].origin.id);
    const native = Object.fromEntries(Object.entries(expected.outputs).map(([name, hash]) => [name, reference.blobs[hash]]));
    const record = parentReference.cases.find(record => record.id === parent.id);
    executions.push({index: row.index, id: row.id, ...require('./ge5_decorator_literal_execution.cjs')
      .run(ts, acorn, parent, original, native, row.outputs, record)});
  } catch (error) { failures.push({index: row.index, id: row.id, runtime: true, error: error.message, stack: error.stack}); }
  if (process.env.BLUEICE_GE5_METHOD_PROGRESS) fs.writeFileSync(process.env.BLUEICE_GE5_METHOD_PROGRESS,
    JSON.stringify({compilerSha256, verified: verified.length, failures: failures.length, lastIndex: row.index,
      controls: verified.reduce((sum, row) => sum + row.controls.length, 0), firstFailure: failures[0]}) + '\n');
}
assert.equal(graph.sha256(fs.readFileSync(compiler)), compilerSha256);
for (const [name, hash] of hashes) assert.equal(graph.sha256(fs.readFileSync(path.join(__dirname, name))), hash, 'immutable tool ' + name);
const report = {family: 'ge3_decorator_compositions/rejected-double-decorated-accessors', configurations: observations.length,
  compilerSha256, repeatBuilds: 2, toolHashes: Object.fromEntries([...hashes].sort()), positiveOnly,
  completeFamily: !positiveOnly && failures.length === 0, verified, failures, executions};
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'decorator-double-accessors.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify({configurations: report.configurations, verified: verified.length, failures: failures.length,
  controls: verified.reduce((sum, row) => sum + row.controls.length, 0),
  executions: executions.reduce((sum, row) => sum + row.executions, 0), completeFamily: report.completeFamily,
  examples: failures.slice(0, 3)}));
if (failures.length) process.exitCode = 1;
