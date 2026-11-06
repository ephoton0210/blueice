// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native AMD keeps original import edges in its unmapped dependency transport.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {mutate} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_decorator_preserved_mutations.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input), rows = JSON.parse(fs.readFileSync(0, 'utf8'));
const expectedIndices = input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/interop-exportstar-cycle'
  && graph.resolve(input, row, parents).options.module === 'AMD' ? [index] : []);
assert.equal(expectedIndices.length, 40);
assert.deepEqual(rows.map(row => row.index), expectedIndices);
const compilerSha256 = rows[0].compilerSha256;
assert(/^[0-9a-f]{64}$/.test(compilerSha256));
assert(rows.every(row => row.compilerSha256 === compilerSha256 && row.repeatBuilds === 2));
let controls = 0;
for (const row of rows) {
  const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
  assert.equal(original.options.module, 'AMD');
  const cap = source.read(ts, original.files, original.entry), module = source.checked(cap).modules[original.entry];
  const imports = module.imports.filter(edge => !edge.star);
  assert.equal(imports.length, 3);
  const entries = Object.entries(expected.maps).filter(([, map]) => map.kind === 'javascript' && map.output === 'out/main.js');
  assert.equal(entries.length, 1); const [mapName, map] = entries[0];
  const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, mapName));
  envelope.read(ts, native, original.options, cap, original.entry, false).classifyTransport();
  for (const segment of native.decoded.segments) if (segment.length > 2) {
    assert.equal(segment[2], 0);
    const offset = module.source.getPositionOfLineAndCharacter(segment[3], segment[4]);
    assert(!imports.some(edge => edge.node.getStart(module.source) <= offset && offset < edge.node.end),
      'Native assigns no emitted source role inside an original AMD import');
  }
  function check(actual) {
    assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
    const item = envelope.read(ts, actual, original.options, cap, original.entry, true);
    item.classifyTransport();
    const acquisitions = item.body.flatMap(statement => {
      if (!ts.isVariableStatement(statement) && !ts.isExpressionStatement(statement)) return [];
      const calls = [];
      function visit(node) {
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'require') calls.push(node);
        ts.forEachChild(node, visit);
      }
      visit(statement);
      return calls.filter(call => call.arguments.length === 1 && ts.isStringLiteral(call.arguments[0])
        && module.imports.some(edge => edge.specifier === call.arguments[0].text)).map(call => ({statement, call}));
    });
    assert.equal(acquisitions.length, module.imports.length, 'all original import and re-export edges are acquired once');
    const owned = [];
    for (let index = 0; index < module.imports.length; index++) {
      const edge = module.imports[index], {statement, call} = acquisitions[index];
      assert.equal(call.arguments[0].text, edge.specifier, 'exact original AMD import edge order');
      if (edge.star) continue;
      actual.noSource(statement.getStart(actual.source), statement.end, 'generated AMD import acquisition');
      assert.equal(actual.owner(statement.end), null, 'generated AMD import completion');
      owned.push(statement);
    }
    return owned;
  }
  const code = row.outputs[map.output], raw = mapName.endsWith('#inline')
    ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString()
    : row.outputs[mapName];
  const actual = stream(ts, code, raw), acquisitions = check(actual);
  for (const [index, statement] of acquisitions.entries()) {
    const at = statement.getStart(actual.source), position = module.source.getLineAndCharacterOfPosition(imports[index].node.getStart(module.source));
    for (const offset of [at, at + 1, statement.end]) {
      assert.throws(() => check(coherent(ts, mutate(ts, actual, offset, [0, position.line, position.character]))));
      controls++;
    }
  }
}
console.log(JSON.stringify({scope: 'AMD import source-grant boundary', configurations: rows.length,
  completeFamily: false, compilerSha256, repeatBuilds: 2, controls, passed: true}));
