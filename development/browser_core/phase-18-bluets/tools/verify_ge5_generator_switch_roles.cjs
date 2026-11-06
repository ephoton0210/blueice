// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every frozen generator product closes complete artifacts, roles and execution.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), contract = require('./ge5_generator_switch_roles.cjs');
const root = path.resolve(__dirname, '../../../..'), fixture = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(fixture, 'cases.json')), referenceBytes = fs.readFileSync(path.join(fixture, 'reference.json'));
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'))();
const rows = process.argv[2]
  ? require('./ge5_callable_observe.cjs').collect(path.resolve(process.argv[2]), process.argv[3], 'ge2_module_helpers/generator-switch-delegation', 363)
  : JSON.parse(fs.readFileSync(0, 'utf8'));
assert.deepEqual(rows.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/generator-switch-delegation' ? [index] : []));
if (process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY, 'generator-switch-delegation.json'), JSON.stringify(rows) + '\n');
}
(async () => {
  const verified = [], failures = [];
  for (const row of rows) {
    const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
    assert.equal(row.id, expected.id); assert.equal(row.repeatBuilds, 2);
    assert.deepEqual(row.options, original.options); assert.deepEqual(row.files, original.files);
    for (const [name, map] of Object.entries(expected.maps)) if (map.kind === 'javascript') try {
      const code = row.outputs[map.output], raw = name.endsWith('#inline')
        ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString()
        : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name));
      const actual = stream(ts, code, raw);
      for (const view of [native, actual]) acorn.parse(view.code, {ecmaVersion: row.options.target === 'ES5' ? 5 : Number(row.options.target.slice(2)), sourceType: 'module'});
      const proof = contract.compare(ts, native, actual, row.options, original.files['main.ts']);
      const controls = require('./ge5_generator_switch_mutations.cjs').run(ts, native, actual, row.options, original.files['main.ts'], proof, contract.compare);
      const nativeOutputs = Object.fromEntries(Object.entries(expected.outputs).map(([file, hash]) => [file, reference.blobs[hash]]));
      const runtime = await require('./ge5_generator_switch_execution.cjs').run(ts, nativeOutputs, row.outputs, row.files, row.options);
      assert.deepEqual(runtime.failures, [], 'all source switch/finally/delegate target ABI executions');
      const borrowed = await require('./ge5_generator_switch_borrowed.cjs').run(ts, nativeOutputs, row.outputs, row.files, row.options);
      assert.deepEqual(borrowed.failures, [], 'all three borrowed methods retain their pinned target ABI');
      verified.push({id: row.id, map: name, controls, runtime, borrowed, allCharactersClassified: true,
        actualJavaScriptSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw)});
    } catch (error) {failures.push({id: row.id, map: name, error: error.message, stack: error.stack});}
    if ((verified.length + failures.length) % 33 === 0) console.error('generator-switch complete products ' + (verified.length + failures.length) + '/363; failures=' + failures.length);
  }
  const report = {form: 'generator-switch-delegation', configurations: rows.length, maps: verified.length + failures.length,
    repeatBuilds: 2, compilerSha256: rows[0].compilerSha256, completeFamily: failures.length === 0, verified, failures};
  assert(rows.every(row => row.compilerSha256 === report.compilerSha256));
  if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
    fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
    fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'generator-switch-delegation.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify(report, null, 2)); if (failures.length) process.exitCode = 1;
})().catch(error => {console.error(error); process.exitCode = 1;});
