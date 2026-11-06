// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const inventory = require('./ge5_accessor_ending_inventory.cjs');
const code = 'class C { get x() { return 4; } set x(n) { sink(n); } }';
const klass = text => ts.createSourceFile('input.ts', text, 99, true).statements[0];
function fixture(actual = code) {
  const source = klass(code), n = klass(code), a = klass(actual);
  return {n, a, proof: {constructionChain: {source, native: {native: true, node: n},
    actual: {native: true, node: a}, pairs: [{label: 'class', native: n, actual: a}]}}};
}
test('retained accessor inventory includes getter value, setter parameter and complete call ends', () => {
  const f = fixture(), result = inventory.read(ts, f.proof), [getter, setter] = f.n.members;
  for (const node of [getter, getter.body, getter.body.statements[0].expression,
    setter, setter.parameters[0], setter.body, setter.body.statements[0].expression]) {
    assert(result.points.some(point => point.native === node));
  }
  assert.equal(result.methods.length, 2);
});
test('a missing or extra accessor cannot shrink the required inventory', () => {
  for (const changed of ['class C { get x(){return 4;} }',
    'class C { get x(){return 4;} set x(n){sink(n);} set x(n){sink(n);} }']) {
    assert.throws(() => inventory.read(ts, fixture(changed).proof));
  }
});
test('changed original callable leaves cannot acquire matching logical endpoints', () => {
  for (const changed of [code.replace('return 4', 'return 5'), code.replace('sink(n)', 'sink(z)')]) {
    assert.throws(() => inventory.read(ts, fixture(changed).proof), /callable leaf/);
  }
});
