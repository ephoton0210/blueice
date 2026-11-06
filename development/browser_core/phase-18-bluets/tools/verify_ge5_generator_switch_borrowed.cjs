// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A public CLI boundary preserves every frozen helper-provider/target profile.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const graph = require('./ge5_inputs.cjs'), borrowed = require('./ge5_generator_switch_borrowed.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const observations = process.argv[2]
  ? require('./ge5_callable_observe.cjs').collect(path.resolve(process.argv[2]), process.argv[3], 'ge2_module_helpers/generator-switch-delegation', 363)
  : JSON.parse(fs.readFileSync(0, 'utf8'));
assert.deepEqual(observations.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/generator-switch-delegation' ? [index] : []));
(async () => {
  const verified = [], failures = [];
  for (const row of observations) {
    const expected = reference.cases[row.index];
    assert.equal(row.id, expected.id); assert.equal(row.repeatBuilds, 2);
    const nativeOutputs = Object.fromEntries(Object.entries(expected.outputs).map(([name, hash]) => [name, reference.blobs[hash]]));
    const result = await borrowed.run(ts, nativeOutputs, row.outputs, row.files, row.options);
    verified.push({id: row.id, ...result});
    failures.push(...result.failures.map(failure => ({id: row.id, ...failure})));
  }
  const report = {scope: 'public generator borrowed-receiver ABI', completeFamily: false,
    configurations: observations.length, repeatBuilds: 2, compilerSha256: observations[0].compilerSha256, verified, failures};
  if (process.env.BLUEICE_GE5_REPORT) fs.writeFileSync(process.env.BLUEICE_GE5_REPORT, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({...report, verified: report.verified.length, failures: failures.slice(0, 9)}, null, 2));
  assert.equal(failures.length, 0, 'every lowered closure and retained intrinsic receiver profile');
})().catch(error => {console.error(error); process.exitCode = 1;});
