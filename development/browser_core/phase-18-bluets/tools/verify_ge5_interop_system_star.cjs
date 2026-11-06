// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A System star export has no original source role in its generated forwarding.
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
  && graph.resolve(input, row, parents).options.module === 'System' ? [index] : []);
assert.equal(expectedIndices.length, 40);
assert.deepEqual(rows.map(row => row.index), expectedIndices);
const compilerSha256 = rows[0].compilerSha256;
assert(/^[0-9a-f]{64}$/.test(compilerSha256));
assert(rows.every(row => row.compilerSha256 === compilerSha256 && row.repeatBuilds === 2));
let controls = 0;
for (const row of rows) {
  const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
  assert.equal(original.options.module, 'System');
  const cap = source.read(ts, original.files, original.entry), model = source.checked(cap).modules[original.entry];
  const stars = model.imports.filter(edge => edge.star); assert.equal(stars.length, 1);
  const entries = Object.entries(expected.maps).filter(([, map]) => map.kind === 'javascript' && map.output === 'out/main.js');
  assert.equal(entries.length, 1); const [mapName, map] = entries[0];
  const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, mapName));
  const transport = envelope.read(ts, native, original.options, cap, original.entry, false);
  const slot = transport.dependencies.indexOf(stars[0].specifier); assert(slot >= 0);
  const calls = transport.setterNodes[slot].body.statements.filter(node => ts.isExpressionStatement(node)
    && ts.isCallExpression(node.expression) && ts.isIdentifier(node.expression.expression));
  assert.equal(calls.length, 1, 'one exact star-forwarding helper in its original dependency setter');
  const helper = transport.prelude.filter(node => ts.isFunctionDeclaration(node)
    && node.name?.text === calls[0].expression.expression.text);
  assert.equal(helper.length, 1);
  native.noSource(helper[0].getStart(native.source), helper[0].end, 'Native generated star-forwarding algorithm');
  function check(actual) {
    assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
    const item = envelope.read(ts, actual, original.options, cap, original.entry, true);
    item.classifyTransport();
    const loops = item.body.filter(ts.isForInStatement);
    assert.equal(loops.length, 1, 'one complete generated source star-forwarding loop');
    const loop = loops[0];
    actual.noSource(loop.getStart(actual.source), loop.end, 'generated System star-forwarding transport');
    assert.equal(actual.owner(loop.end), null, 'generated System star-forwarding logical end');
    return loop;
  }
  const code = row.outputs[map.output], raw = mapName.endsWith('#inline')
    ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString()
    : row.outputs[mapName];
  const actual = stream(ts, code, raw), loop = check(actual);
  const position = model.source.getLineAndCharacterOfPosition(stars[0].node.getStart(model.source));
  for (const offset of [loop.getStart(actual.source), loop.getStart(actual.source) + 1, loop.end]) {
    assert.throws(() => check(coherent(ts, mutate(ts, actual, offset, [0, position.line, position.character]))));
    controls++;
  }
}
console.log(JSON.stringify({scope: 'System star-forwarding source-grant boundary', configurations: rows.length,
  completeFamily: false, compilerSha256, repeatBuilds: 2, controls, passed: true}));
