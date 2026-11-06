// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// All frozen cyclic modules cross one complete source/generated-role contract.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), roles = require('./ge5_interop_module.cjs');
const root = path.resolve(__dirname, '../../../..'), directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), referenceBytes = fs.readFileSync(path.join(directory, 'reference.json'));
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const rows = JSON.parse(fs.readFileSync(0, 'utf8'));
const selected = input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/interop-exportstar-cycle' ? [index] : []);
assert.equal(selected.length, 363); assert.deepEqual(rows.map(row => row.index), rows.length === 3 ? [4892, 4933, 5013] : selected);
const compilerSha256 = rows[0].compilerSha256;
const report = {scope: 'Complete module syntax, declaration/reference/import/publication roles and UTF-16 gaps',
  completeFamily: false, compilerSha256, configurations: rows.length, maps: 0, verified: [], failures: [],
  remaining: ['effective complete-contract negative controls', 'cyclic graph execution and live binding acceptance']};
for (const row of rows) {
  const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  assert.equal(row.id, expected.id); assert.deepEqual(row.options, original.options);
  for (const [name, map] of Object.entries(expected.maps)) {
    if (map.kind !== 'javascript') continue; report.maps++;
    try {
      const filename = path.posix.relative(original.options.outDir, map.output).replace(/\.js$/, '.ts');
      const code = row.outputs[map.output], raw = name.endsWith('#inline')
        ? Buffer.from(/sourceMappingURL=data:[^,]+,([^\s]+)/.exec(code)[1], 'base64').toString('utf8') : row.outputs[name];
      roles.compare(ts, stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name)),
        stream(ts, code, raw), original.options, original.files, original.entry, filename);
      report.verified.push({index: row.index, map: name, actualCodeSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw)});
    } catch (error) { report.failures.push({index: row.index, map: name, error: error.message, stack: error.stack}); }
  }
}
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'interop-complete-module-positive.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify({...report, verified: report.verified.length, failures: report.failures.slice(0, 5), failureCount: report.failures.length}));
assert.equal(report.maps, rows.length * 4); assert.equal(report.failures.length, 0, 'complete original cyclic module roles');
