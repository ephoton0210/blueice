// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Public CLI proof for complete source expressions; module closure is separate.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const source = require('./ge5_interop_source.cjs'), envelope = require('./ge5_interop_envelope.cjs');
const imports = require('./ge5_interop_imports.cjs'), accounting = require('./ge5_class_accounting.cjs');
const declarationMaps = require('./ge5_interop_declaration_maps.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {mutate} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_decorator_preserved_mutations.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
for (const [name, expected] of [['cases.json', '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e'],
  ['reference.json', '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3']]) {
  assert.equal(graph.sha256(fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps', name))), expected);
}
const parents = graph.parents(root, input), rows = JSON.parse(fs.readFileSync(0, 'utf8'));
const selected = input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/interop-exportstar-cycle' ? [index] : []);
assert.equal(selected.length, 363);
if (rows.length === 1) assert([4884, 4924].includes(rows[0].index), 'explicit public regression configuration');
else assert.deepEqual(rows.map(row => row.index), selected);
const compilerSha256 = rows[0].compilerSha256;
assert(/^[0-9a-f]{64}$/.test(compilerSha256));
const report = {scope: 'Complete original declaration syntax, expression interiors and logical endpoints; module closure pending',
  completeFamily: false, compilerSha256, configurations: rows.length, maps: 0, endpoints: 0, controls: 0, failures: []};
for (const row of rows) {
  const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  assert.deepEqual(row.options, original.options); assert.equal(row.id, expected.id);
  const cap = source.read(ts, original.files, original.entry);
  for (const [mapName, map] of Object.entries(expected.maps)) {
    if (map.kind !== 'javascript') continue;
    try {
      const filename = path.posix.relative(original.options.outDir, map.output).replace(/\.js$/, '.ts');
      const nativeCode = reference.blobs[expected.outputs[map.output]], nativeRaw = authority.raw(reference, expected, mapName);
      const code = row.outputs[map.output], raw = mapName.endsWith('#inline')
        ? Buffer.from(/sourceMappingURL=data:[^,]+,([^\s]+)/.exec(code)[1], 'base64').toString('utf8') : row.outputs[mapName];
      function check(actual) {
        const native = stream(ts, nativeCode, nativeRaw), models = [native, actual].map((view, index) => {
          const item = envelope.read(ts, view, original.options, cap, filename, index === 1);
          const binding = imports.read(ts, item, cap, filename, index === 1);
          return {binding};
        });
        assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
        const roles = accounting.create(ts, native, actual, [original.files[filename]]);
        const proof = declarationMaps.compare(ts, native, models[0].binding, actual, models[1].binding, roles);
        return [...proof.expressionPairs, ...proof.endpoints];
      }
      const actual = stream(ts, code, raw), pairs = check(actual);
      check(stream(ts, code, raw)); // The exact negative-control entry also accepts unchanged artifacts.
      const claimed = new Set(actual.decoded.segments.filter(point => point.length > 2).map(point => JSON.stringify(point.slice(2))));
      const line = original.files[filename].split('\n')[0];
      const column = Array.from({length: line.length + 1}, (_, index) => index)
        .find(column => !claimed.has(JSON.stringify([0, 0, column])));
      assert.notEqual(column, undefined); const wrong = [0, 0, column], seen = new Set();
      for (const pair of pairs) {
        if (seen.has(pair.actual.end)) continue; seen.add(pair.actual.end);
        for (const owner of [wrong, ...(actual.owner(pair.actual.end) ? [null] : [])]) {
          assert.throws(() => check(coherent(ts, mutate(ts, actual, pair.actual.end, owner)))); report.controls++;
        }
      }
      report.maps++; report.endpoints += pairs.length;
    } catch (error) { report.failures.push({index: row.index, map: mapName, error: error.message, stack: error.stack}); }
  }
}
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'interop-expression-maps.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify(report));
assert.equal(report.failures.length, 0, 'complete original expression role and effective endpoint controls');
assert.equal(report.maps, rows.length * 4);
