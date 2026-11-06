// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Isolate one valid, bounded map mutation; the original authority stays intact.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs');
const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function vlq(value) {
  let bits = Math.abs(value) * 2 + Number(value < 0), result = '';
  do { const digit = bits % 32; bits = Math.floor(bits / 32); result += alphabet[digit + (bits ? 32 : 0)]; } while (bits);
  return result;
}
function encode(segments) {
  const lines = []; let source = 0, originalLine = 0, originalColumn = 0, name = 0, generatedLine = -1, column = 0;
  for (const segment of segments) {
    if (generatedLine !== segment[0]) { generatedLine = segment[0]; column = 0; }
    while (lines.length <= generatedLine) lines.push([]);
    const fields = [segment[1] - column]; column = segment[1];
    if (segment.length > 2) {
      fields.push(segment[2] - source, segment[3] - originalLine, segment[4] - originalColumn);
      [source, originalLine, originalColumn] = segment.slice(2, 5);
      if (segment.length === 6) { fields.push(segment[5] - name); name = segment[5]; }
    }
    lines[generatedLine].push(fields.map(vlq).join(''));
  }
  return lines.map(line => line.join(',')).join(';');
}
function mutate(ts, view, offset, owner) {
  const segments = view.decoded.segments.map(segment => [...segment]);
  const changes = [[offset, owner], [offset + 1, view.owner(offset + 1)]];
  for (const [position, next] of changes) {
    const { line, character } = view.source.getLineAndCharacterOfPosition(position);
    const index = segments.findIndex(segment => segment[0] > line || segment[0] === line && segment[1] >= character);
    const at = index < 0 ? segments.length : index;
    while (segments[at]?.[0] === line && segments[at]?.[1] === character) segments.splice(at, 1);
    segments.splice(at, 0, [line, character, ...(next || [])]);
  }
  return stream(ts, view.code, JSON.stringify({ ...JSON.parse(view.raw), mappings: encode(segments) }));
}
function run(ts, native, actual, check) {
  const tokens = actual.tokens(actual.source), mapped = tokens.find(token => token.text === 'resolve' && token.owners.every(Boolean));
  assert(mapped, 'mutation input must retain an await operand');
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && /^__blueice_target_async_\d+$/.test(node.name?.text));
  const wrapper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.body?.statements.some(ts.isIfStatement));
  assert(helper && wrapper, 'mutation input must contain the explicit async ABI');
  const call = wrapper.body.statements.at(-1).expression, callback = call.arguments.at(-1);
  const grouping = tokens.find(token => token.text === '(' && token.start > callback.body.getStart(actual.source) && actual.owner(token.start) === null);
  assert(grouping, 'mutation input must contain generated yield grouping');
  const tests = [
    ['retarget UTF-16 token interior', mapped.start + 1, [0, 0, 0]],
    ['erase UTF-16 token interior', mapped.start + 1, null],
    ['grant helper user source', helper.name.getStart(actual.source) + 1, [0, 0, 0]],
    ['grant constructor guard user source', wrapper.body.statements[0].getStart(actual.source), [0, 0, 0]],
    ['grant yield grouping user source', grouping.start, [0, 0, 0]],
    ['erase logical body close', callback.body.end - 1, null],
    ['erase driver completion', callback.body.end, null],
    ['retarget mapped gap', mapped.end, [0, 0, 0]],
    ['grant trailing comment user source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]],
  ];
  return tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner); // strict decoder must accept the isolated mutation
    assert.throws(() => check(native, changed, true), undefined, label);
    return { label, rejected: true };
  });
}
module.exports = { run, encode, mutate };
