// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Output-option observations preserve raw declarations and literal contents.
const assert = require('node:assert/strict');
const provenance = new Set(['/* bluets-class-helper-v1 */']);

function javascript(acorn, row, name, source) {
  const tokens = [], comments = [];
  acorn.parse(source, {ecmaVersion: 2022,
    sourceType: row.runtimeModule === 'CommonJS' ? 'script' : 'module',
    onToken: tokens, onComment: comments});
  const protectedRanges = tokens.filter(token =>
    ['string', 'regexp', 'template'].includes(token.type.label));
  const endings = new Set();
  let range = 0, interiorBoms = 0;
  for (let index = 0; index < source.length; index++) {
    while (range < protectedRanges.length && protectedRanges[range].end <= index) range++;
    const token = protectedRanges[range];
    if (token && token.start <= index && index < token.end) {
      index = token.end - 1;
      continue;
    }
    if (source.charCodeAt(index) === 0xfeff && index !== 0) interiorBoms++;
    if (source[index] === '\r') {
      if (source[index + 1] === '\n') { endings.add('crlf'); index++; }
      else endings.add('cr');
    } else if (source[index] === '\n') endings.add('lf');
  }
  const inputs = Object.values(row.files);
  const sourceComments = [], helperProvenance = [];
  for (const comment of comments) {
    const raw = source.slice(comment.start, comment.end);
    if (inputs.some(input => input.includes(raw))) {
      sourceComments.push({type: comment.type, text: raw});
    } else {
      // Exact versioned compiler provenance is checked separately from source
      // trivia. No arbitrary generated comment or source-text normalization.
      assert(provenance.has(raw), name + ': unowned generated comment: ' + raw);
      helperProvenance.push(raw);
    }
  }
  return {bom: source.charCodeAt(0) === 0xfeff, interiorBoms,
    terminalNewline: /(?:\r\n|[\r\n])$/.test(source),
    newlineKinds: [...endings].sort(), sourceComments, helperProvenance};
}
function profiles(acorn, row, outputs) {
  return Object.fromEntries(Object.entries(outputs)
    .filter(([name]) => /\.(?:js|mjs|cjs)$/.test(name))
    .map(([name, source]) => [name, javascript(acorn, row, name, source)]));
}
function compare(actual, expected, id) {
  assert.deepEqual(Object.keys(actual).sort(), Object.keys(expected).sort(), id + ': JavaScript inventory');
  for (const name of Object.keys(expected)) {
    const {helperProvenance: actualHelpers, ...a} = actual[name];
    const {helperProvenance: nativeHelpers, ...e} = expected[name];
    assert.deepEqual(a, e, id + ': exact BOM/newline/source-comment observations: ' + name);
    assert(nativeHelpers.every(raw => provenance.has(raw)), id + ': native provenance');
    assert(actualHelpers.every(raw => provenance.has(raw)), id + ': BlueTS provenance');
  }
}
module.exports = {javascript, profiles, compare};
