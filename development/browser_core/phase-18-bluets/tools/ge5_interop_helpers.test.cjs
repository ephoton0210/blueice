// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path'), test = require('node:test');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), helper = require('./ge5_interop_helpers.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {mutate} = require('./ge5_role_mutations.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const parents = graph.parents(root, input);
const originalInput = graph.resolve(input, input.cases[4884], parents);
const directory = path.join(root, 'backend/bluets/src/emitter');
const literal = /const IMPORT_DEFAULT_HELPER: &str = ("(?:[^"\\]|\\.)*");/.exec(fs.readFileSync(path.join(directory, 'commonjs.rs'), 'utf8'));
assert(literal);
const privateCode = JSON.parse(literal[1]) + '\n' + fs.readFileSync(path.join(directory, 'commonjs/import_star.v1.js'), 'utf8');
const privateRow = {index: 4884, options: originalInput.options, code: privateCode};
function fixture(row, change = value => value) {
  const original = {...originalInput, options: row.options};
  const raw = JSON.stringify({version: 3, file: 'main.js', sources: ['main.ts'],
    sourcesContent: [original.files['main.ts']], names: [], mappings: ''});
  const cap = source.read(ts, original.files, original.entry), view = change(stream(ts, row.code, raw));
  const item = envelope.read(ts, view, row.options, cap, original.entry, true);
  return {view, item, cap, row, original};
}

test('complete private helpers match their canonical algorithms and acquire no source', () => {
  const f = fixture(privateRow), proof = helper.checked(helper.read(ts, f.item, f.cap, 'main.ts', f.row.options, true), f.view);
  assert.equal(proof.helpers.length, 2);
  assert.equal(proof.imported, false); assert.equal(proof.ambient, false);
});

test('changed private helper logic cannot retain a helper proof', () => {
  const f = fixture(privateRow, view => {
    const changed = view.code.replace('mod && mod.__esModule', 'mod || mod.__esModule');
    assert.notEqual(changed, view.code); assert.equal(changed.length, view.code.length);
    return stream(ts, changed, view.raw);
  });
  assert.throws(() => helper.read(ts, f.item, f.cap, 'main.ts', f.row.options, true));
});

test('a canonical helper still cannot acquire a source grant', () => {
  const original = fixture(privateRow);
  const proof = helper.checked(helper.read(ts, original.item, original.cap, 'main.ts', original.row.options, true), original.view);
  const at = proof.helpers[0].getStart(original.view.source);
  const changed = fixture(privateRow, view => mutate(ts, view, at, [0, 0, 0]));
  assert.throws(() => helper.read(ts, changed.item, changed.cap, 'main.ts', changed.row.options, true));
});

test('an imported helper reference must use the independently bound namespace and member', () => {
  const row = {index: 4884, options: {...originalInput.options, importHelpers: true},
    code: "var tslib_1=require('tslib'); tslib_1.__importDefault({});"};
  const f = fixture(row), cap = helper.read(ts, f.item, f.cap, 'main.ts', row.options, true);
  const proof = helper.checked(cap, f.view); assert(proof.namespace);
  const calls = [];
  function visit(node) {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
      && node.expression.name.text === '__importDefault') calls.push(node.expression);
    ts.forEachChild(node, visit);
  }
  visit(f.view.source); assert.equal(calls.length, 1);
  assert(helper.reference(ts, cap, f.view, calls[0], '__importDefault'));
  assert.throws(() => helper.reference(ts, cap, f.view, calls[0], '__importStar'));
  assert.throws(() => helper.checked({...cap}, f.view), /owned complete interop helper proof/);
});
