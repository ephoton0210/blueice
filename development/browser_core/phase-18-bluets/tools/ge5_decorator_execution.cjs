// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Run both complete artifact graphs using the already frozen loader scenarios.
const assert = require('node:assert/strict');
const ge1 = require('./ge1_observations.cjs'), ge2 = require('./ge2_observations.cjs');
function run(acorn, suite, parentInput, parent, original, native, actual) {
  const row = {...parent, options: original.options, files: original.files};
  const edition = row.options.target === 'ESNext' ? 'latest' : row.options.target === 'ES5' ? 5 : Number(row.options.target.slice(2));
  for (const outputs of [native, actual]) for (const [name, source] of Object.entries(outputs)) {
    if (!/\.(?:js|mjs|cjs)$/.test(name)) continue;
    acorn.parse(source, {ecmaVersion: edition, sourceType: row.runtimeModule === 'ESNext' ? 'module' : 'script'});
  }
  const expected = suite === 'ge2_module_helpers' ? ge2.execute(parentInput, row, native) : ge1.execute(row, native);
  const observed = suite === 'ge2_module_helpers' ? ge2.execute(parentInput, row, actual) : ge1.execute(row, actual);
  assert.deepEqual(observed, expected, 'actual complete Native/Blue execution scenarios');
  const scenarios = suite === 'ge2_module_helpers' ? Object.keys(expected) : ['configured'];
  const completed = suite === 'ge2_module_helpers' ? expected.configured : expected;
  assert.equal(completed.exit, 0, 'actual Native completion');
  return {executions: scenarios.length * 2, scenarios, observation: observed};
}
module.exports = {run};
