// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test'), assert = require('node:assert/strict'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const inputs = require('./ge5_inputs.cjs'), graphs = require('./ge5_metadata_class_graph.cjs');
const input = require(root + '/backend/bluets/tests/fixtures/ge5_source_maps/cases.json');
const parents = inputs.parents(root, input), spec = input.cases.find(row => row.form === 'ge3_decorator_compositions/imported-callable-alias');
const original = inputs.resolve(input, spec, parents);
const read = (files = original.files, options = original.options) => graphs.read(ts, files, original.entry, options);

test('an exact original exported function type supplies Function without provider acquisition', () => {
  const graph = read(); assert(graphs.valid(graph)); assert.equal(graph.runtime, false);
  assert.equal(graph.aliases.length, 1); const alias = graph.aliases[0];
  assert(ts.isTypeAliasDeclaration(alias) && ts.isFunctionTypeNode(alias.type));
  assert.equal(alias.getSourceFile(), graph.provider);
  const types = graph.expected.flatMap(role => [role.type, ...(role.types || [])].filter(Boolean));
  assert.equal(types.length, 4);
  for (const type of types) {
    assert.equal(type.alias, alias); assert.equal(type.callable, alias.type);
    assert.equal(type.builtin, 'Function'); assert.equal(type.runtime, false);
  }
});
test('disabled metadata preserves the original alias proof and emits no helper roles', () => {
  const graph = read(original.files, {...original.options, emitDecoratorMetadata: false});
  assert.equal(graph.runtime, false); assert.equal(graph.aliases.length, 1); assert.equal(graph.expected.length, 0);
});
test('a function type cannot authorize a runtime use of the imported alias', () => {
  assert.throws(() => read({...original.files, 'main.ts': original.files['main.ts'] + '\nexport const leaked=Callback;\n'}),
    /solely by original constructor metadata types/);
});
test('generic, missing and duplicated aliases cannot borrow this graph identity', () => {
  for (const replacement of ['export type Callback<T> = (n:number)=>number;', '',
    'export type Callback = (n:number)=>number; export type Callback = (n:number)=>number;']) {
    assert.throws(() => read({...original.files, 'types.ts': original.files['types.ts']
      .replace('export type Callback = (n: number) => number;', replacement)}));
  }
});
test('a copied graph cannot forge the original function-type capability', () => {
  assert.equal(graphs.valid({...read()}), false);
});
