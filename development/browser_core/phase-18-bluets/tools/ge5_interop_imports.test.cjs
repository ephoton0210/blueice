// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), imports = require('./ge5_interop_imports.cjs');
const {stream} = require('./ge5_role_stream.cjs'), authority = require('./ge5_native_authority.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index, transform = code => code) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const map = expected.maps['out/main.js.map'];
  const code = transform(reference.blobs[expected.outputs[map.output]]);
  const view = stream(ts, code, authority.raw(reference, expected, 'out/main.js.map'));
  const cap = source.read(ts, original.files, original.entry);
  const item = envelope.read(ts, view, original.options, cap, original.entry, false);
  return {view, cap, item};
}

test('source imports bind exact symbols through every Native module ABI', () => {
  for (const index of [4884, 4924, 4964, 5004, 5044, 5084, 5164]) {
    const f = fixture(index), model = imports.checked(imports.read(ts, f.item, f.cap, 'main.ts', false), f.view);
    assert.deepEqual(Object.keys(model.symbols), ['value', 'dep', 'read']);
    assert.equal(model.symbols.value.imported, 'default');
    assert.equal(model.symbols.dep.namespace, 'dep');
    assert.equal(model.symbols.read.specifier, './left.js');
    assert.equal(model.acquisitions.length, 4);
    assert.equal(model.helperAlgorithmsValidated, false, 'binding structure alone cannot establish complete helper parity');
  }
});

test('copied and foreign-view import proofs cannot approve references', () => {
  const f = fixture(4884), proof = imports.read(ts, f.item, f.cap, 'main.ts', false);
  assert.throws(() => imports.checked({...proof}, f.view), /owned import binding proof/);
  assert.throws(() => imports.checked(proof, stream(ts, f.view.code, f.view.raw)), /owned import binding proof/);
});

test('an original namespace import cannot become an unrelated local binding', () => {
  const f = fixture(4884), declaration = f.item.body.filter(ts.isVariableStatement)
    .flatMap(node => [...node.declarationList.declarations]).find(node => node.name.getText(f.view.source) === 'dep');
  assert(declaration); const at = declaration.name.getStart(f.view.source);
  const changed = fixture(4884, code => code.slice(0, at) + 'abc' + code.slice(declaration.name.end));
  assert.throws(() => imports.read(ts, changed.item, changed.cap, 'main.ts', false), /original namespace import identity/);
});

test('a changed require edge cannot retain the original import proof', () => {
  const changed = fixture(4884, code => code.replace('require("./dep.js")', 'require("./bad.js")'));
  assert.throws(() => imports.read(ts, changed.item, changed.cap, 'main.ts', false), /original import\/re-export acquisitions/);
});
