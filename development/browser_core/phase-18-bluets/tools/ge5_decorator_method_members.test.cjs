// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), core = require('./ge5_decorator_member_core.cjs');
const methods = require('./ge5_decorator_method_members.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);

function fixture(index, change = value => value) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const source = ts.createSourceFile('original.ts', original.files['main.ts'], 99, true);
  const klass = source.statements.find(ts.isClassDeclaration), capability = methods.source(ts, klass);
  const map = expected.maps['out/main.js.map'];
  const view = stream(ts, change(reference.blobs[expected.outputs[map.output]]),
    authority.raw(reference, expected, 'out/main.js.map'));
  const item = core.read(ts, view, klass, original.options);
  return {original, capability, view, item, read: () => methods.read(ts, view, item, capability)};
}

test('all original literal methods have one complete Native callable identity', () => {
  for (const index of [6054, 6056, 6062, 6070, 6078, 6086, 6088, 6094]) {
    const f = fixture(index), result = f.read();
    assert.equal(result.methods.length, 3);
    assert.deepEqual(result.methods.map(method => method.source.name.text), ['a-b', '16', 'escapeda']);
    const proof = methods.compare(ts, f.view, result, f.view, result, f.capability);
    assert(proof.pairs.length >= 15);
  }
});

test('an emitted method cannot change the exact original literal key', () => {
  const f = fixture(6054, code => { assert(code.includes("C.prototype['a-b']")); return code.replace("C.prototype['a-b']", "C.prototype['x-b']"); });
  assert.throws(f.read, /original literal method/);
});

test('a method body must preserve the original return constant', () => {
  const f = fixture(6054, code => code.replace('return 1;', 'return 9;'));
  assert.throws(f.read, /original method body/);
});

test('the prototype assignment belongs to the exact original class receiver', () => {
  const f = fixture(6054, code => { assert(code.includes("C.prototype['a-b']")); return code.replace("C.prototype['a-b']", "X.prototype['a-b']"); });
  assert.throws(f.read, /original literal method/);
});

test('a cloned source capability cannot authorize method projection', () => {
  const f = fixture(6054);
  assert.throws(() => methods.read(ts, f.view, f.item, {...f.capability}), /original method capability/);
});

test('descriptor flags must keep the complete Native method definition', () => {
  const f = fixture(6056, code => {
    assert(code.includes('enumerable: false')); return code.replace('enumerable: false', 'enumerable: true');
  });
  assert.throws(f.read, /original method descriptor flags/);
});
