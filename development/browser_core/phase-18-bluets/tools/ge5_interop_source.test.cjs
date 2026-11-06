// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const parents = graph.parents(root, input);
const original = graph.resolve(input, input.cases[4884], parents);
const contract = require('./ge5_interop_source.cjs');

test('the complete original import graph retains duplicate AMD dependency slots and its cycle', () => {
  const proof = contract.read(ts, original.files, original.entry);
  const model = contract.checked(proof);
  assert.deepEqual(Object.keys(model.modules).sort(), ['dep.ts', 'left.ts', 'main.ts', 'right.ts']);
  assert.deepEqual(contract.dependencies(proof, 'main.ts', 'AMD', false),
    ['./dep.js', './dep.js', './left.js', './left.js']);
  assert.deepEqual(contract.dependencies(proof, 'main.ts', 'AMD', true), ['./dep.js', './left.js']);
  assert.deepEqual(contract.dependencies(proof, 'main.ts', 'System', false), ['./dep.js', './left.js']);
  assert.equal(model.modules['left.ts'].imports[0].resolved, 'right.ts');
  assert.equal(model.modules['right.ts'].imports[0].resolved, 'left.ts');
  assert.deepEqual(model.modules['dep.ts'].exports.map(value => [value.exported, value.local]),
    [['value', 'value'], ['default', 'add']]);
});

test('a copied import-graph capability cannot sign an emitted module', () => {
  const proof = contract.read(ts, original.files, original.entry);
  assert.throws(() => contract.checked({...proof}), /owned original import graph/);
});

test('a changed original dependency AST cannot reuse the source graph proof', () => {
  const proof = contract.read(ts, original.files, original.entry);
  const node = contract.checked(proof).modules['main.ts'].imports[0].node.moduleSpecifier;
  const before = node.text;
  node.text = './wrong.js';
  try { assert.throws(() => contract.checked(proof), /original AST/); }
  finally { node.text = before; }
  assert(contract.checked(proof));
});

test('every original dependency must resolve within the supplied source graph', () => {
  assert.throws(() => contract.read(ts, {...original.files,
    'right.ts': 'import { value } from "../outside.js"; export const rightValue=value;'}, original.entry),
  /original dependency escapes the supplied graph/);
});

test('every frozen interop source graph is covered by the original model', () => {
  let configurations = 0;
  for (const row of input.cases) if (row.form === 'ge2_module_helpers/interop-exportstar-cycle') {
    const original = graph.resolve(input, row, parents);
    assert.equal(Object.keys(contract.checked(contract.read(ts, original.files, original.entry)).modules).length, 4);
    configurations++;
  }
  assert.equal(configurations, 363);
});
