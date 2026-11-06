// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), imports = require('./ge5_interop_imports.cjs');
const declarations = require('./ge5_interop_declaration_maps.cjs'), accounting = require('./ge5_class_accounting.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {mutate} = require('./ge5_role_mutations.cjs');
const authority = require('./ge5_native_authority.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index, filename, change = value => value) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const mapName = 'out/' + filename.replace(/\.ts$/, '.js.map'), map = expected.maps[mapName];
  const code = reference.blobs[expected.outputs[map.output]], raw = authority.raw(reference, expected, mapName);
  const cap = source.read(ts, original.files, original.entry), native = stream(ts, code, raw), actual = change(stream(ts, code, raw));
  const bindings = [native, actual].map(view => imports.read(ts,
    envelope.read(ts, view, original.options, cap, filename, false), cap, filename, false));
  const roles = accounting.create(ts, native, actual, [original.files[filename]]);
  return declarations.compare(ts, native, bindings[0], actual, bindings[1], roles);
}

test('all original declaration headers, bodies and endpoints retain their complete Native roles', () => {
  for (const index of [4884, 4924, 4964, 5004, 5044, 5084, 5164]) {
    for (const filename of ['main.ts', 'dep.ts', 'left.ts', 'right.ts']) assert(fixture(index, filename).endpoints.length);
  }
});

test('original callable body completion cannot be erased', () => {
  assert.throws(() => fixture(4884, 'dep.ts', actual => {
    const fn = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name.text === 'add');
    assert(fn && actual.owner(fn.body.end)); return mutate(ts, actual, fn.body.end, null);
  }), /exact interop declaration logical endpoint/);
});

test('original variable statement completion cannot be retargeted', () => {
  assert.throws(() => fixture(4884, 'dep.ts', actual => {
    const statement = actual.source.statements.find(node => ts.isExpressionStatement(node)
      && ts.isBinaryExpression(node.expression) && node.expression.left.getText(actual.source) === 'exports.value'
      && ts.isNumericLiteral(node.expression.right));
    assert(statement); return mutate(ts, actual, statement.end, [0, 0, 0]);
  }), /exact interop declaration logical endpoint/);
});
