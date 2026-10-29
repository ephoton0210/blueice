// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test');
const assert = require('node:assert/strict');
const { canonicalStateSeeds } = require('./gm2_native.cjs');
const hash = 'a'.repeat(64);
const hashes = { 'lib.es5.d.ts': hash };
const seed = name => ({ seeds: { sample: { info: {
  fileNames: [name, '../src/main.ts'],
  fileInfos: [{ version: hash, affectsGlobalScope: true }, 'program-version'],
  root: [2], options: { composite: true }, version: '5.9.3',
}, outputs: { 'out/main.d.ts': 'export declare const value: number;\n' } } }, cases: [] });

test('native state library identities survive oracle relocation without changing records', () => {
  const expected = seed('@typescript/lib/lib.es5.d.ts');
  for (const name of [
    '../../../../tmp/bluets-gm2-oracles.old/node_modules/typescript/lib/lib.es5.d.ts',
    '/private/tmp/another-oracle/node_modules/typescript/lib/lib.es5.d.ts',
    'C:\\pinned\\node_modules\\typescript\\lib\\lib.es5.d.ts',
    '@typescript/lib/lib.es5.d.ts',
  ]) {
    const record = seed(name), before = structuredClone(record);
    assert.deepEqual(canonicalStateSeeds(record, hashes), expected);
    assert.deepEqual(record, before);
  }
});

test('program paths, signatures, state options and output bytes remain exact', () => {
  const record = seed('../src/lib.es5.d.ts');
  assert.deepEqual(canonicalStateSeeds(record, hashes), record);
  const changed = seed('@typescript/lib/lib.es5.d.ts');
  changed.seeds.sample.info.fileInfos[1] = 'different-program-version';
  assert.notDeepEqual(canonicalStateSeeds(changed, hashes), canonicalStateSeeds(seed('@typescript/lib/lib.es5.d.ts'), hashes));
});

test('library path normalization requires the pinned content hash', () => {
  const changed = seed('../node_modules/typescript/lib/lib.es5.d.ts');
  changed.seeds.sample.info.fileInfos[0].version = 'b'.repeat(64);
  assert.throws(() => canonicalStateSeeds(changed, hashes), /library content/);
  assert.throws(() => canonicalStateSeeds(seed('../node_modules/typescript/lib/lib.unknown.d.ts'), hashes), /library content/);
});
