// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = require(root + '/backend/bluets/tests/fixtures/ge5_source_maps/cases.json');
const reference = require(root + '/backend/bluets/tests/fixtures/ge5_source_maps/reference.json');
const graph = require('./ge5_inputs.cjs'), parents = graph.parents(root, input);
const authority = require('./ge5_native_authority.cjs'), frame = require('./ge5_class_expression_frame.cjs');
const members = require('./ge5_class_expression_members.cjs');
const {stream, compareTokens} = require('./ge5_role_stream.cjs');

function fixture(index, change = code => code) {
  const row = graph.resolve(input, input.cases[index], parents), record = reference.cases[index];
  const source = ts.createSourceFile('original.ts', row.files['main.ts'], 99, true);
  const originals = [];
  function visit(node) {
    if (ts.isClassExpression(node) && ts.getDecorators(node)?.length) originals.push(node);
    ts.forEachChild(node, visit);
  }
  visit(source); assert.equal(originals.length, 1);
  const name = 'out/main.js.map', map = record.maps[name];
  const code = reference.blobs[record.outputs[map.output]], raw = authority.raw(reference, record, name);
  const n = stream(ts, code, raw), a = stream(ts, change(code), raw);
  const nn = frame.read(ts, n, originals[0], row.options), aa = frame.read(ts, a, originals[0], row.options);
  const check = () => members.compare(ts, n, nn, a, aa, row.options, {
    pair(left, right, label) { compareTokens(n, left, a, right, label); }
  });
  return {n, a, nn, aa, check};
}

test('the complete class-expression core accepts Native ES5 method assignment', () => {
  for (const index of [6390, 6391]) {
    const f = fixture(index); assert.equal(f.nn.classRead.methods[0].descriptor, false);
    assert(f.check().implicit);
  }
});

test('the complete class-expression core retains Native ES5 descriptor mode', () => {
  const f = fixture(6392); assert.equal(f.nn.classRead.methods[0].descriptor, true);
  assert(f.check().implicit);
});

test('an assignment cannot borrow another class prototype receiver', () => {
  const baseline = fixture(6390), receiver = baseline.nn.classRead.methods[0].target.expression.expression;
  const at = receiver.getStart(baseline.n.source);
  const f = fixture(6390, code => code.slice(0, at) + 'Z' + code.slice(at + 1));
  assert.throws(f.check, /receiver/);
});

test('an assignment cannot replace the original literal method key', () => {
  const baseline = fixture(6390), key = baseline.nn.classRead.methods[0].key;
  const at = key.getStart(baseline.n.source) + 1;
  const f = fixture(6390, code => code.slice(0, at) + 'Z' + code.slice(at + 1));
  assert.throws(f.check, /property|reference/);
});
