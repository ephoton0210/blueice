// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), imports = require('./ge5_interop_imports.cjs');
const declarations = require('./ge5_interop_declarations.cjs');
const {stream} = require('./ge5_role_stream.cjs'), authority = require('./ge5_native_authority.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index, filename, change = value => value) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const mapName = 'out/' + filename.replace(/\.ts$/, '.js.map'), map = expected.maps[mapName];
  const view = stream(ts, change(reference.blobs[expected.outputs[map.output]]), authority.raw(reference, expected, mapName));
  const cap = source.read(ts, original.files, original.entry);
  const item = envelope.read(ts, view, original.options, cap, filename, false);
  return {view, binding: imports.read(ts, item, cap, filename, false)};
}

test('each source declaration has one initializer or complete callable through every module ABI', () => {
  for (const index of [4884, 4924, 4964, 5004, 5044, 5084, 5164]) {
    for (const filename of ['main.ts', 'dep.ts', 'left.ts', 'right.ts']) {
      const f = fixture(index, filename), result = declarations.read(ts, f.binding, f.view);
      assert.equal(result.declarations.length, filename === 'main.ts' ? 1 : filename === 'left.ts' ? 3 : 2);
      assert(result.declarations.every(item => item.expression || item.body));
      assert.equal(result.completeModuleValidated, false);
    }
  }
});

test('a duplicate original initializer cannot hide among generated module statements', () => {
  const f = fixture(4884, 'dep.ts', code => code.replace('exports.value = 3;', 'exports.value = 3; exports.value = 3;'));
  assert.throws(() => declarations.read(ts, f.binding, f.view), /one complete original variable initializer/);
});

test('a renamed source function leaves a missing declaration', () => {
  const f = fixture(4884, 'dep.ts', code => code.replace('function add(', 'function bad('));
  assert.throws(() => declarations.read(ts, f.binding, f.view), /one complete original function declaration/);
});

test('function declarations preserve their complete original callable kind and parameters', () => {
  for (const change of [code => code.replace('function add(', 'function* add('),
    code => code.replace('function add(value)', 'function add(other)')]) {
    const f = fixture(4884, 'dep.ts', change);
    assert.throws(() => declarations.read(ts, f.binding, f.view));
  }
});
