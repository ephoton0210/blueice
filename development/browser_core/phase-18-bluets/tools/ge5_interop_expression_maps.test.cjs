// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), imports = require('./ge5_interop_imports.cjs');
const declarations = require('./ge5_interop_declarations.cjs'), expressions = require('./ge5_interop_expression_maps.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {mutate} = require('./ge5_role_mutations.cjs');
const authority = require('./ge5_native_authority.cjs'), accounting = require('./ge5_class_accounting.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index, filename, change = value => value) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const mapName = 'out/' + filename.replace(/\.ts$/, '.js.map'), map = expected.maps[mapName];
  const code = reference.blobs[expected.outputs[map.output]], raw = authority.raw(reference, expected, mapName);
  const cap = source.read(ts, original.files, original.entry), native = stream(ts, code, raw), actual = change(stream(ts, code, raw));
  const models = [native, actual].map(view => {
    const item = envelope.read(ts, view, original.options, cap, filename, false);
    const binding = imports.read(ts, item, cap, filename, false);
    return {binding, declarations: declarations.read(ts, binding, view).declarations};
  });
  const roles = accounting.create(ts, native, actual, [original.files[filename]]), proofs = [];
  for (let index = 0; index < models[0].declarations.length; index++) {
    const n = models[0].declarations[index], a = models[1].declarations[index];
    function compare(original, nn, an) {
      proofs.push(expressions.compare(ts, native, models[0].binding, actual, models[1].binding, original, nn, an, roles));
    }
    if (n.expression) compare(n.original.node.initializer, n.expression, a.expression);
    else n.original.node.body.statements.forEach((node, index) => compare(node.expression,
      n.body.statements[index].expression, a.body.statements[index].expression));
  }
  return {native, actual, proofs};
}

test('complete original expressions retain UTF-16 roles and endpoints through every module ABI', () => {
  for (const index of [4884, 4924, 4964, 5004, 5044, 5084, 5164]) {
    for (const filename of ['main.ts', 'dep.ts', 'left.ts', 'right.ts']) assert(fixture(index, filename).proofs.length);
  }
});

test('imported callee logical endpoints reject both erasure and retargeting', () => {
  for (const owner of [null, [0, 0, 0]]) {
    assert.throws(() => fixture(4884, 'main.ts', actual => {
      let callee;
      function visit(node) {
        if (ts.isPropertyAccessExpression(node) && node.name.text === 'default') callee = node;
        ts.forEachChild(node, visit);
      }
      visit(actual.source); assert(callee); return mutate(ts, actual, callee.end, owner);
    }), /exact interop expression logical endpoint/);
  }
});

test('a retained source reference rejects a UTF-16 interior source grant mutation', () => {
  assert.throws(() => fixture(4884, 'left.ts', actual => {
    let reference;
    function visit(node) {
      if (ts.isPropertyAccessExpression(node) && node.name.text === 'rightValue') reference = node;
      ts.forEachChild(node, visit);
    }
    visit(actual.source); assert(reference);
    return mutate(ts, actual, reference.getStart(actual.source) + 1, [0, 0, 0]);
  }));
});
