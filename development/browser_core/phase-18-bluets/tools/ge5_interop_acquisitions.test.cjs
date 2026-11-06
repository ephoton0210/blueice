// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), imports = require('./ge5_interop_imports.cjs');
const helpers = require('./ge5_interop_helpers.cjs'), acquisitions = require('./ge5_interop_acquisitions.cjs');
const {stream} = require('./ge5_role_stream.cjs'), authority = require('./ge5_native_authority.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index, filename = 'main.ts', change = value => value) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const mapName = 'out/' + filename.replace(/\.ts$/, '.js.map'), map = expected.maps[mapName];
  const view = stream(ts, change(reference.blobs[expected.outputs[map.output]]), authority.raw(reference, expected, mapName));
  const cap = source.read(ts, original.files, original.entry);
  const item = envelope.read(ts, view, original.options, cap, filename, false);
  const binding = imports.read(ts, item, cap, filename, false);
  const helper = helpers.read(ts, item, cap, filename, original.options, false);
  return {view, binding, helper, options: original.options};
}
function read(f) { return acquisitions.read(ts, f.binding, f.helper, f.view, f.options, false); }

test('every original acquisition retains its exact wrapper and provider across Native module ABIs', () => {
  for (const index of [4884, 4886, 4888, 4890, 4924, 4964, 4966, 4968, 4970, 5004, 5044, 5084, 5164]) {
    for (const filename of ['main.ts', 'dep.ts', 'left.ts', 'right.ts']) {
      const f = fixture(index, filename), result = read(f);
      assert.equal(result.edges.length, imports.checked(f.binding, f.view).module.imports.length);
      assert.equal(result.completeModuleValidated, false);
    }
  }
});

test('an import wrapper cannot call an unrelated private helper', () => {
  const f = fixture(4884, 'main.ts', code => {
    const before = '= __importDefault(require(';
    assert(code.includes(before)); return code.replace(before, '= __importDefaulx(require(');
  });
  assert.throws(() => read(f), /exact private or ambient helper reference/);
});

test('an imported wrapper must use its independently proved library namespace', () => {
  const f = fixture(4888, 'main.ts', code => {
    const before = '= tslib_1.__importDefault(require(';
    assert(code.includes(before)); return code.replace(before, '= tslib_9.__importDefault(require(');
  });
  assert.throws(() => read(f), /exact independently bound imported helper reference/);
});

test('AMD wrapper assignment must target its exact dependency parameter', () => {
  const f = fixture(4964, 'main.ts', code => {
    const before = 'dep_js_1 = __importDefault(dep_js_1)';
    assert(code.includes(before)); return code.replace(before, 'dep_js_9 = __importDefault(dep_js_1)');
  });
  assert.throws(() => read(f), /one exact AMD interop wrapper assignment/);
});
