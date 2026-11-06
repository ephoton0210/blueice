// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const { test } = require('node:test'), assert = require('node:assert/strict');
const { decode, artifacts } = require('./ge5_maps.cjs');
const { coordinate } = require('./ge5_inputs.cjs');
const base = { version: 3, file: 'main.js', sourceRoot: '', sources: ['main.ts'],
  names: ['first', 'second'], sourcesContent: ['🧭x'], mappings: 'AAAA,CAAE,C,CAACC',
  x_retained_metadata: { authority: 'fixture' } };

test('every segment, name, trivia transition and metadata field survives decoding', () => {
  const actual = decode(JSON.stringify(base), 'a b c');
  assert.deepEqual(actual.segments, [[0, 0, 0, 0, 0], [0, 1, 0, 0, 2], [0, 2], [0, 3, 0, 0, 3, 1]]);
  assert.deepEqual(actual.transitions, [[0, 0, true], [0, 2, false], [0, 3, true]]);
  const { mappings, ...metadata } = base;
  assert.deepEqual(actual.metadata, metadata);
});

test('invalid indices, UTF-16 bounds, negative deltas and unfinished fields fail', () => {
  for (const mappings of ['ACAA', 'AADA', 'AAAI', 'MAAA', 'AAAA,DAAA', 'g', 'AAAA,', 'A?AA']) {
    assert.throws(() => decode(JSON.stringify({ ...base, mappings }), 'a b c'), mappings);
  }
  assert.throws(() => decode(JSON.stringify({ ...base, names: [], mappings: 'AAAAC' }), 'a b c'));
  assert.throws(() => decode(JSON.stringify({ ...base, sourcesContent: [] }), 'a b c'));
});

test('same-position Native class-closure segments retain their order and distinct source owners', () => {
  const actual = decode(JSON.stringify({ ...base, sourcesContent: ['abc'], mappings: 'AAAA,AAAC' }), 'x');
  assert.deepEqual(actual.segments, [[0, 0, 0, 0, 0], [0, 0, 0, 0, 1]]);
});

test('Native bad-coordinate evidence is retained explicitly while actual-map validation stays strict', () => {
  const raw = JSON.stringify({ ...base, mappings: 'AAAI' });
  assert.throws(() => decode(raw, 'x'));
  const native = decode(raw, 'x', { recordInvalidCoordinates: true });
  assert.deepEqual(native.segments, [[0, 0, 0, 0, 4]]);
  assert.deepEqual(native.coordinateIssues, [{ kind: 'UTF-16 source bounds', segment: [0, 0, 0, 0, 4] }]);
});

test('line changes reset mapped state and coordinate variants preserve astral UTF-16 width', () => {
  const actual = decode(JSON.stringify({ ...base, mappings: 'AAAA;C' }), 'a\nb');
  assert.deepEqual(actual.segments, [[0, 0, 0, 0, 0], [1, 1]]);
  assert.deepEqual(actual.transitions, [[0, 0, true]]);
  assert.equal(coordinate('export const x = 1;\n', 'crlfAstral'), '/* G-E5 🧭 */ export const x = 1;\r\n');
  assert.equal(coordinate('\ufeffx\r\ny\n', 'original'), '\ufeffx\r\ny\n');
});

test('inline map publication must be an actual final directive; declaration maps retain identity', () => {
  const raw = JSON.stringify(base), directive = '//# sourceMappingURL=data:application/json;base64,' + Buffer.from(raw).toString('base64');
  const inline = artifacts({ 'main.js': 'a b c\n' + directive + '\n' });
  assert.deepEqual(Object.keys(inline), ['main.js#inline']);
  assert.equal(inline['main.js#inline'].text, raw);
  assert.deepEqual(artifacts({ 'main.js': JSON.stringify(directive) + ';\n' }), {});
  const declaration = artifacts({ 'main.d.ts': 'a b c', 'main.d.ts.map': raw });
  assert.equal(declaration['main.d.ts.map'].kind, 'declaration');
  assert.throws(() => artifacts({ 'main.js.map': raw }));
});
