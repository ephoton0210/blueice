// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Public emitted modules preserve all pinned switch/delegate target ABI profiles.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const rows = process.argv[2]
  ? require('./ge5_callable_observe.cjs').collect(path.resolve(process.argv[2]), process.argv[3], 'ge2_module_helpers/generator-switch-delegation', 363)
  : JSON.parse(fs.readFileSync(0, 'utf8'));
assert.deepEqual(rows.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/generator-switch-delegation' ? [index] : []));
(async () => {
  const verified = [], failures = [];
  for (const row of rows) {
    const expected = reference.cases[row.index]; assert.equal(row.id, expected.id); assert.equal(row.repeatBuilds, 2);
    const native = Object.fromEntries(Object.entries(expected.outputs).map(([name, hash]) => [name, reference.blobs[hash]]));
    const proof = await require('./ge5_generator_switch_execution.cjs').run(ts, native, row.outputs, row.files, row.options);
    verified.push({id: row.id, ...proof}); failures.push(...proof.failures.map(failure => ({id: row.id, ...failure})));
  }
  const report = {scope: 'public generator switch/delegation semantics', completeFamily: false,
    configurations: rows.length, repeatBuilds: 2, compilerSha256: rows[0].compilerSha256, verified, failures};
  if (process.env.BLUEICE_GE5_REPORT) fs.writeFileSync(process.env.BLUEICE_GE5_REPORT, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({...report, verified: verified.length, failures: failures.slice(0, 6)}, null, 2));
  assert.equal(failures.length, 0, 'all original and supplemental generator target/provider executions');
})().catch(error => {console.error(error); process.exitCode = 1;});
