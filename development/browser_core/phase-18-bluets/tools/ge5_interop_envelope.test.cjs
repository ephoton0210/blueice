// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const {stream} = require('./ge5_role_stream.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input), original = graph.resolve(input, input.cases[4964], parents);
const expected = reference.cases[4964], code = reference.blobs[expected.outputs['out/main.js']];
const raw = require('./ge5_native_authority.cjs').raw(reference, expected, 'out/main.js.map');
const envelope = require('./ge5_interop_envelope.cjs');
function read(text = code) {
  return envelope.read(ts, stream(ts, text, raw), original.options,
    source.read(ts, original.files, original.entry), 'main.ts', false);
}

test('Native AMD namespace parameters retain their original import identity', () => {
  const item = read();
  assert.equal(item.format, 'wrapped');
  assert.deepEqual(item.dependencies, ['./dep.js', './dep.js', './left.js', './left.js']);
  assert.equal(item.parameterBindings[1].binding, 'dep');
  item.classifyTransport();
});

test('AMD duplicate dependency slots cannot change their module identity', () => {
  const item = read(), literal = item.registration.expression.arguments[0].elements[2];
  const at = literal.getStart(item.view.source);
  const changed = code.slice(0, at) + '"./bad.js"' + code.slice(literal.end);
  assert.equal(changed.length, code.length);
  assert.throws(() => read(changed), /dependency slots/);
});

test('an AMD namespace parameter cannot lose its original namespace binding', () => {
  const item = read(), parameter = item.factory.parameters[3].name;
  const at = parameter.getStart(item.view.source);
  const changed = code.slice(0, at) + 'abc' + code.slice(parameter.end);
  assert.equal(changed.length, code.length);
  assert.throws(() => read(changed), /original namespace import/);
});

test('a generated AMD dependency parameter cannot capture a source binding', () => {
  const item = read(), parameter = item.factory.parameters[2].name;
  const at = parameter.getStart(item.view.source);
  const changed = code.slice(0, at) + 'value'.padEnd(parameter.end - at) + code.slice(parameter.end);
  assert.equal(changed.length, code.length);
  assert.throws(() => read(changed), /fresh AMD dependency/);
});
