// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The Rust caller retains every original K11 assertion and proves source identity
// before this transport canonicalizes the single source label for the comparer.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const root = path.resolve(__dirname, '../../../..');
const loadTypeScript = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const { stream } = require('./ge5_role_stream.cjs'), { decode } = require('./ge5_maps.cjs');
const { comments } = require('./ge5_decorator_preserved.cjs');
const standard = new Map([
  ['k11-standard-method', ['decorated.ts', 'e242c3674ae8287d9df707a7501580043542b6e0912eaece9569586a890ddfd4']],
  ['k11-compacted-relocation', ['decorated_parenthesized.ts', 'b01d39fc18b5c3b9ed688293e06ef5341ba9ca1260c1ad97951f7a0487fca7ee']],
  ['k11-erased-relocation', ['decorated_erased_assertion.ts', 'f157e75862c138ecb9158af81dc81237311e1f7ba7e1dda47b972e47ffd2b96b']],
  ['k11-composed-decorators', ['decorated_multiple.ts', 'c20b6fe0d901114e100bcc1e0022b9613242e7247bc4a239903ab539a2936f9e']],
]);
const legacySystem = new Map([
  ['k11-legacy-class-member', ['legacy_decorated.ts', '6b5ea6445580bfbe55cb491dde83154b7689ae38c71fdda2753fa8acb819aba4']],
  ['k11-legacy-parameter-metadata', ['legacy_parameter_metadata.ts', '84d4cad4d110aa257b3c00b90344e57390c4ddff460cf2997763b86cdea82d3f']],
  ['k11-system-wrapper', ['system_wrapper.ts', 'f5be563ed189d1c9ba0f5e231f097af4660533868a7bcd7f1030a7fd68d42b3f']],
]);
const sha = text => crypto.createHash('sha256').update(text).digest('hex');
function fixture(input) {
  const frozen = standard.get(input.case) || legacySystem.get(input.case);
  assert(frozen, 'closed original seven-row K11 whole-module dispatcher');
  assert.equal(sha(input.source), frozen[1], 'unchanged original K11 fixture bytes');
  const bytes = fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/k11_source_maps', frozen[0]));
  assert.equal(sha(bytes), frozen[1], 'frozen repository K11 fixture identity'); assert.equal(bytes.toString('utf8'), input.source);
  return standard.has(input.case);
}
function prepare(ts, product, source, identity, inlineSources, { allowApiPublication = false } = {}) {
  assert.equal(typeof product.code, 'string');
  const map = typeof product.map === 'string' ? JSON.parse(product.map) : product.map;
  assert.equal(map.version, 3); assert.equal(map.file, identity.basename.replace(/\.ts$/, '.js'), 'exact tested output basename');
  assert(Array.isArray(map.sources) && map.sources.length === 1, 'one exact original source');
  assert.equal(map.sources[0].split(/[/\\]/).at(-1), identity.basename, 'prevalidated single-source basename');
  if (inlineSources) assert.deepEqual(map.sourcesContent, [source], 'exact original source content');
  else assert(!Object.hasOwn(map, 'sourcesContent'), 'inlineSources-off retains its original absent table');
  const publications = comments(ts, { code: product.code }).filter(comment => comment.text.startsWith('//# sourceMappingURL='));
  assert(publications.length <= 1, 'at most one physical map publication');
  if (!publications.length) assert(allowApiPublication, 'Native physical JavaScript must publish its map');
  const before = decode(JSON.stringify(map), product.code);
  const canonical = { ...map, sources: [identity.basename] }, raw = JSON.stringify(canonical);
  assert.equal(canonical.mappings, map.mappings, 'source identity transport cannot change mappings');
  assert.deepEqual(decode(raw, product.code).segments, before.segments, 'source identity transport preserves every decoded segment');
  let code = product.code;
  const publication = '//# sourceMappingURL=' + map.file + '.map';
  if (publications.length) {
    assert.equal(publications[0].text, publication, 'exact physical map publication');
    assert(/^\s*$/.test(code.slice(publications[0].end)), 'publication is the terminal comment');
  } else {
    // A public API returns an artifact rather than a physical publication. One
    // extra line separates its unchanged mapped EOF from the test-only comment.
    code += '\n' + publication + '\n';
    assert(code.startsWith(product.code), 'publication preserves every API JavaScript byte');
  }
  assert.deepEqual(decode(raw, code).segments, before.segments, 'test publication never adds, drops, or moves a segment');
  return { view: stream(ts, code, raw), sourcesCanonicalized: map.sources[0] !== identity.basename, publicationAdded: !publications.length,
    originalCodeSha256: sha(product.code), originalMapSha256: sha(JSON.stringify(map)), canonicalMapSha256: sha(raw) };
}
function verify(input) {
  assert.equal(input.schema, 1); assert.equal(typeof input.source, 'string');
  assert.deepEqual(Object.keys(input.sourceIdentity).sort(), ['basename', 'sourceCount', 'sourceSha256']);
  const identity = input.sourceIdentity; assert.equal(identity.sourceCount, 1); assert.equal(identity.basename, 'main.ts');
  assert.equal(identity.sourceSha256, sha(input.source), 'Rust-proved original source identity');
  const oracle = process.env.BLUEICE_BLUETSC_ORACLE; assert(oracle, 'set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3');
  const ts = loadTypeScript(oracle), isStandard = fixture(input);
  const n = prepare(ts, input.native, input.source, identity, input.options.inlineSources), a = prepare(ts, input.blue, input.source, identity, input.options.inlineSources, { allowApiPublication: true });
  let proof, runControls;
  if (isStandard) {
    proof = require('./ge5_k11_standard_module.cjs').compare(ts, n.view, a.view, input.options, input.source);
    runControls = require('./ge5_k11_standard_controls.cjs').run;
  } else {
    const other = require('./k11_legacy_system_roles.cjs');
    proof = other.compare(ts, n.view, a.view, input.options, input.source);
    assert.equal(proof.case, input.case, 'complete leaf belongs to the exact original K11 row');
    runControls = other.runControls;
  }
  assert.equal(proof.completeModuleRoles, true, 'leaf must provide the complete module contract');
  n.view.complete('bridge requires every Native module character proved');
  a.view.complete('bridge requires every Blue module character proved');
  const controls = runControls(ts, n.view, a.view, input.options, input.source, proof);
  assert(controls.length > 0 && controls.every(control => control.rejected), 'complete fresh effective negative controls');
  const logicalEndpoints = count(proof.endpoints) ?? [proof.ends, proof.callableEnds, proof.sourceCommentEnds].reduce((sum, value) => sum + (count(value) ?? 0), 0);
  return { schema: 1, case: input.case, sourceSha256: identity.sourceSha256, inlineSources: input.options.inlineSources,
    completeModuleRoles: proof.completeModuleRoles, effectiveRejectedControls: controls.length, tokenPairs: count(proof.nativeTokens),
    logicalEndpoints, nativeCharacters: n.view.code.length, blueCharacters: a.view.code.length,
    sourceIdentityTransport: { native: { sourcesCanonicalized: n.sourcesCanonicalized, publicationAdded: n.publicationAdded }, blue: { sourcesCanonicalized: a.sourcesCanonicalized, publicationAdded: a.publicationAdded } },
    nativeOriginalCodeSha256: n.originalCodeSha256, blueOriginalCodeSha256: a.originalCodeSha256 };
}
function count(value) { return Array.isArray(value) ? value.length : Number.isSafeInteger(value) && value >= 0 ? value : null; }
if (require.main === module) {
  try { console.log(JSON.stringify(verify(JSON.parse(fs.readFileSync(0, 'utf8'))))); }
  catch (error) { console.error(error.stack || String(error)); process.exitCode = 1; }
}
module.exports = { verify, prepare, fixture, count };
