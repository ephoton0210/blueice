// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), execution = require('./ge5_interop_execution.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const outputs = Object.fromEntries(Object.entries(expected.outputs).filter(([name]) => name.endsWith('.js'))
    .map(([name, hash]) => [name, reference.blobs[hash]]));
  return {original, outputs};
}

test('the actual Native graph executes cycles and forwards all three subsequent updates', async () => {
  for (const index of [4884, 4886, 4888, 4964, 4968, 5004, 5044, 5084, 5164]) {
    const f = fixture(index), result = await execution.run(ts, f.outputs, f.outputs, f.original.files, f.original.options);
    assert.equal(result.executions, f.original.options.module === 'UMD' ? 4 : 2);
  }
});

test('a System update without publication cannot pass the live-cycle acceptance', async () => {
  const f = fixture(5044), outputs = {...f.outputs}, before = 'exports_1("leftValue", leftValue = value)';
  assert(outputs['out/left.js'].includes(before)); outputs['out/left.js'] = outputs['out/left.js'].replace(before, '(leftValue = value)');
  await assert.rejects(() => execution.execute(ts, outputs, f.original.files, f.original.options, 'default'), /live imports and star exports/);
});
