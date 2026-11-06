// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every retained token and every surviving mapped gap is challenged separately.
const assert = require('node:assert/strict');
const {stream} = require('./ge5_role_stream.cjs'), {mutate, encode} = require('./ge5_role_mutations.cjs');
const {compare} = require('./ge5_decorator_preserved.cjs');
function coherent(ts, view) {
  const prefix = '//# sourceMappingURL=data:application/json;base64,';
  if (!view.code.includes(prefix)) return view;
  const start = view.code.indexOf(prefix) + prefix.length;
  const end = view.code.indexOf('\n', start);
  const code = view.code.slice(0, start) + Buffer.from(view.raw).toString('base64') + view.code.slice(end < 0 ? view.code.length : end);
  return stream(ts, code, view.raw);
}
function run(ts, native, actual, original, proof) {
  const controls = [], claimed = new Set(native.decoded.segments.filter(point => point.length > 2).map(point => JSON.stringify(point.slice(2))));
  const column = Array.from({length: original.split('\n')[0].length + 1}, (_, column) => column)
    .find(column => !claimed.has(JSON.stringify([0, 0, column])));
  assert.notEqual(column, undefined, 'a bounded source interior exists outside Native emitted roles');
  const wrong = [0, 0, column];
  function mapping(label, offset, owner) {
    const changed = coherent(ts, mutate(ts, actual, offset, owner)); // Complete inline artifacts stay coherent and strictly decode.
    assert.throws(() => compare(ts, native, changed), undefined, label); controls.push({label, offset, rejected: true});
  }
  for (const [index, pair] of proof.classEndpoints.entries()) {
    mapping('class declaration ' + index + ': retarget exact endpoint', pair.actual.end, wrong);
    if (actual.owner(pair.actual.end)) mapping('class declaration ' + index + ': erase exact endpoint', pair.actual.end, null);
  }
  for (const [index, token] of proof.actualTokens.entries()) {
    const offset = token.start + Number(token.end - token.start > 1), owner = actual.owner(offset);
    mapping('token ' + index + ' ' + token.text + ': retarget complete UTF-16 role', offset, wrong);
    if (owner) mapping('token ' + index + ' ' + token.text + ': erase retained UTF-16 role', offset, null);
    if (index) {
      const previous = proof.actualTokens[index - 1];
      for (let position = previous.end; position < token.start; position++) {
        if (actual.owner(position)) { mapping('before token ' + index + ': retarget surviving ordered gap', position, wrong); break; }
      }
    }
  }
  const comment = proof.actualComments.generated[0];
  for (const [index, comment] of proof.actualComments.source.entries()) {
    for (let offset = comment.start; offset < comment.end; offset++) {
      mapping('source comment ' + index + ': retarget UTF-16 interior', offset, wrong);
      if (actual.owner(offset)) mapping('source comment ' + index + ': erase UTF-16 interior', offset, null);
    }
    mapping('source comment ' + index + ': erase exact endpoint', comment.end, null);
    mapping('source comment ' + index + ': retarget exact endpoint', comment.end, wrong);
  }
  mapping('generated source-map comment cannot acquire source authority', comment.start + 4, wrong);
  const segments = actual.decoded.segments.map(point => [...point]);
  const position = actual.source.getLineAndCharacterOfPosition(comment.start + 4), point = [position.line, position.character];
  const at = segments.findIndex(segment => segment[0] > point[0] || segment[0] === point[0] && segment[1] >= point[1]);
  const insertion = at < 0 ? segments.length : at;
  segments.splice(insertion, 0, [...point, ...wrong], [...point]);
  const hidden = coherent(ts, stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), mappings: encode(segments)})));
  assert.throws(() => compare(ts, native, hidden), undefined, 'hidden zero-width source grant');
  controls.push({label: 'hidden zero-width source grant', rejected: true});
  function code(label, start, end, replacement) {
    assert.equal(replacement.length, end - start);
    const changed = stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw);
    assert.throws(() => compare(ts, native, changed), undefined, label); controls.push({label, rejected: true});
  }
  for (const [index, token] of proof.actualTokens.entries()) {
    if (/^\d$/.test(token.text)) code('token ' + index + ': change numeric operation input', token.start, token.end, token.text === '0' ? '1' : '0');
    if (token.text === '&&' || token.text === '||') code('token ' + index + ': invert branch operation', token.start, token.end, token.text === '&&' ? '||' : '&&');
    if (token.text === 'method') code('token ' + index + ': replace original method binding', token.start, token.end, 'methox');
  }
  const privateName = proof.actualTokens.find(token => token.text.startsWith('#')).text;
  const replacement = privateName.slice(0, -1) + (privateName.endsWith('x') ? 'y' : 'x');
  const renamed = actual.code.replaceAll(privateName, replacement);
  assert.notEqual(renamed, actual.code);
  const alias = stream(ts, renamed, actual.raw);
  assert.throws(() => compare(ts, native, alias), undefined, 'source private bindings retain their names');
  controls.push({label: 'source private bindings retain their names', rejected: true});
  for (const [field, value] of [['sourceRoot', '../forged/'], ['names', ['forged']]]) {
    const changed = coherent(ts, stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), [field]: value})));
    assert.throws(() => compare(ts, native, changed), undefined, 'complete metadata ' + field);
    controls.push({label: 'complete metadata ' + field, rejected: true});
  }
  const appended = stream(ts, actual.code + '\n/* added generated code */\n', actual.raw);
  assert.throws(() => compare(ts, native, appended), undefined, 'unexpected generated comment inventory');
  controls.push({label: 'unexpected generated comment inventory', rejected: true});
  return controls;
}
module.exports = {run, coherent};
