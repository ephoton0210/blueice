// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Bridge regression over genuine immutable CLI observations. API-shaped inputs
// remove only their physical publication and use its public URI source identity.
const fs = require('node:fs'), assert = require('node:assert/strict'), crypto = require('node:crypto');
const { verify, prepare, count } = require('./verify_k11_whole_module.cjs');
assert.equal(count(147), 147); assert.equal(count([{}, {}]), 2); assert.equal(count(undefined), null);
const path = require('node:path'), root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE);
const { decode } = require('./ge5_maps.cjs');
assert(process.argv[2] && process.argv[3], 'usage: node test.cjs RAW_REPORT NEW_OUTPUT');
assert(!fs.existsSync(process.argv[3]), 'fresh bridge report output');
const bytes = fs.readFileSync(process.argv[2]), report = JSON.parse(bytes), observations = [];
const selected = new Set(['k11-standard-method', 'k11-compacted-relocation', 'k11-erased-relocation', 'k11-composed-decorators', 'k11-legacy-class-member', 'k11-legacy-parameter-metadata', 'k11-system-wrapper']);
const rows = report.observations.filter(row => selected.has(row.row.case)); assert.equal(rows.length, selected.size);
for (const row of rows) for (const mode of row.modes) {
  const source = fs.readFileSync(path.join(row.directory, 'main.ts'), 'utf8');
  const originalMap = JSON.parse(fs.readFileSync(path.join(mode.products.blue.project, 'out/main.js.map'), 'utf8'));
  const blueMap = { ...originalMap, sources: ['memory:///main.ts'] };
  const at = mode.products.blue.code.lastIndexOf('\n//# sourceMappingURL='); assert(at >= 0);
  const code = mode.products.blue.code.slice(0, at);
  const input = { schema: 1, case: row.row.case, source, options: { ...mode.products.native.options, useDefineForClassFields: true },
    sourceIdentity: { basename: 'main.ts', sourceCount: 1, sourceSha256: crypto.createHash('sha256').update(source).digest('hex') },
    native: { code: mode.products.native.code, map: JSON.parse(fs.readFileSync(path.join(mode.products.native.project, 'out/main.js.map'), 'utf8')) }, blue: { code, map: blueMap } };
  const result = verify(input);
  const transported = prepare(ts, input.blue, source, input.sourceIdentity, mode.inlineSources, { allowApiPublication: true });
  assert.equal(transported.view.code.slice(0, code.length), code, 'all input API bytes survive');
  assert.deepEqual(decode(transported.view.raw, transported.view.code).segments, decode(JSON.stringify(blueMap), code).segments, 'all original decoded entries survive');
  const rejected = [];
  function refuses(label, change, reason) { const modified = structuredClone(input); change(modified); assert.throws(() => verify(modified), reason, label); rejected.push(label); }
  refuses('wrong source identity', i => { i.sourceIdentity.sourceSha256 = '0'.repeat(64); });
  refuses('ambiguous sources', i => { i.blue.map.sources.push('other.ts'); });
  refuses('unvalidated basename', i => { i.blue.map.sources[0] = 'memory:///other.ts'; });
  refuses('wrong output file', i => { i.blue.map.file = 'other.js'; });
  refuses('wrong physical map publication', i => { i.blue.code += '\n//# sourceMappingURL=forged.js.map\n'; });
  refuses('Native physical publication is mandatory', i => { i.native.code = i.native.code.slice(0, i.native.code.lastIndexOf('\n//# sourceMappingURL=')); }, /Native physical JavaScript must publish its map/);
  refuses('publication followed by an effect', i => { i.blue.code += '\n//# sourceMappingURL=main.js.map\n42;\n'; });
  refuses('changed frozen source', i => { i.source += ' '; i.sourceIdentity.sourceSha256 = crypto.createHash('sha256').update(i.source).digest('hex'); });
  refuses('unknown case', i => { i.case = 'unknown'; });
  if (mode.inlineSources) refuses('wrong source contents', i => { i.blue.map.sourcesContent = [source + ' ']; });
  if (row.row.case === 'k11-standard-method') {
    const leaf = require('./ge5_k11_standard_module.cjs'), complete = leaf.compare;
    try { leaf.compare = (...args) => ({ ...complete(...args), completeModuleRoles: false }); assert.throws(() => verify(input), /leaf must provide the complete module contract/); rejected.push('subset leaf cannot claim full module acceptance'); }
    finally { leaf.compare = complete; }
  }
  observations.push({ ...result, sourceIdentityAndPublicationControlsRejected: rejected });
  console.log(result.case, result.inlineSources, result.effectiveRejectedControls, rejected.length);
}
const result = { scope: 'Portable bridge regression: real CLI41 map entries and API-shaped URI/publication transport only; not an API compiler replay',
  reportSha256: crypto.createHash('sha256').update(bytes).digest('hex'), compilerSha256: report.compilerSha256,
  configurations: observations.length, effectiveRejectedControls: observations.reduce((n, row) => n + row.effectiveRejectedControls, 0),
  bridgeControlsRejected: observations.reduce((n, row) => n + row.sourceIdentityAndPublicationControlsRejected.length, 0), observations };
fs.writeFileSync(process.argv[3], JSON.stringify(result, null, 2) + '\n');
