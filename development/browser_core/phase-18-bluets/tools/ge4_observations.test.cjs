// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
const test = require('node:test'), assert = require('node:assert/strict');
const acorn = require('../../../../backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs')();
const observation = require('./ge4_observations.cjs');
const row = {runtimeModule: 'ESNext', files: {'main.ts': '/* visible */\nexport const value = 1;\n'}};
const profile = text => observation.javascript(acorn, row, 'main.js', text);
test('BOM, terminal line ending and source trivia are independently exact', () => {
  const original = profile('\ufeff/* visible */\r\nexport const value = 1;\r\n');
  assert.equal(original.bom, true); assert.equal(original.interiorBoms, 0);
  assert.equal(original.terminalNewline, true); assert.deepEqual(original.newlineKinds, ['crlf']);
  assert.deepEqual(original.sourceComments, [{type: 'Block', text: '/* visible */'}]);
  for (const changed of ['/* visible */\r\nexport const value = 1;\r\n',
    '\ufeff/* visible */\nexport const value = 1;\r\n',
    '\ufeffexport const value = 1;\r\n', '\ufeff/* visible */\r\nexport const value = 1;']) {
    assert.throws(() => observation.compare({'main.js': profile(changed)}, {'main.js': original}, 'changed'));
  }
});
test('literal newlines, comment-like strings and regular expressions do not become output trivia', () => {
  const p = profile('export const value = `raw\n/* not trivia */`;\r\nconst pattern = /\\/\\*/;\r\n');
  assert.deepEqual(p.newlineKinds, ['crlf']); assert.deepEqual(p.sourceComments, []);
});
test('only an exact versioned helper marker is separate provenance', () => {
  const expected = profile('export const value = 1;\n');
  const actual = profile('/* bluets-class-helper-v1 */ export const value = 1;\n');
  observation.compare({'main.js': actual}, {'main.js': expected}, 'helper');
  assert.deepEqual(actual.helperProvenance, ['/* bluets-class-helper-v1 */']);
  assert.throws(() => profile('/* bluets-class-helper-v99 */ export const value = 1;\n'), /unowned generated comment/);
  const sourceRow = {...row, files: {'main.ts': '/* bluets-class-helper-v1 */ export const value = 1;\n'}};
  const sourceMarker = observation.javascript(acorn, sourceRow, 'main.js', sourceRow.files['main.ts']);
  assert.equal(sourceMarker.helperProvenance.length, 0); assert.equal(sourceMarker.sourceComments.length, 1);
});
