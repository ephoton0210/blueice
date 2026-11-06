// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each effective mutation re-enters the complete Native-derived module contract.
const assert = require('node:assert/strict');
const {stream} = require('./ge5_role_stream.cjs'), {mutate, encode} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_decorator_preserved_mutations.cjs'), modules = require('./ge5_interop_module.cjs');

function run(ts, native, actual, options, files, entry, filename, proof) {
  const controls = [], claimed = new Set(native.decoded.segments.filter(point => point.length > 2).map(point => JSON.stringify(point.slice(2))));
  const line = files[filename].split('\n')[0], column = Array.from({length: line.length + 1}, (_, index) => index)
    .find(column => !claimed.has(JSON.stringify([0, 0, column])));
  assert.notEqual(column, undefined); const wrong = [0, 0, column];
  function check(view) {
    return modules.compare(ts, stream(ts, native.code, native.raw), view, options, files, entry, filename);
  }
  check(stream(ts, actual.code, actual.raw)); // Same negative entry must accept an unchanged fresh graph.
  function rejected(label, changed, offset) {
    assert.throws(() => check(coherent(ts, changed)), undefined, label); controls.push({label, offset, rejected: true});
  }
  function mapping(label, offset, owner) {
    assert.notDeepEqual(actual.owner(offset), owner, 'effective ' + label);
    rejected(label, mutate(ts, actual, offset, owner), offset);
  }
  function code(label, start, end, replacement) {
    assert.equal(replacement.length, end - start); assert.notEqual(actual.code.slice(start, end), replacement);
    const changed = stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw);
    assert.equal(changed.source.parseDiagnostics.length, 0, 'valid JavaScript control'); rejected(label, changed, start);
  }
  const endpoints = new Set();
  for (const pair of [...proof.declarations.expressionPairs, ...proof.declarations.endpoints, ...proof.acquisitionPairs]) {
    const node = pair.actual; if (!node || endpoints.has(node.end)) continue; endpoints.add(node.end);
    mapping('retarget exact original declaration/reference/acquisition end', node.end, wrong);
    if (actual.owner(node.end)) mapping('erase exact original declaration/reference/acquisition end', node.end, null);
  }
  const tokens = actual.tokens(actual.source), interiors = new Set();
  for (const pair of proof.declarations.expressionPairs) {
    if (!['import', 'local', 'parameter'].includes(pair.role)) continue;
    const token = actual.tokens(pair.actual)[0]; if (!token || interiors.has(token.start)) continue;
    interiors.add(token.start); const at = token.start + Math.min(1, token.end - token.start - 1);
    mapping('retarget complete source binding UTF-16 interior', at, wrong);
    if (actual.owner(at)) mapping('erase complete source binding UTF-16 interior', at, null);
  }
  for (const node of proof.actual.item.sourceNodes) {
    const words = actual.tokens(node);
    if (words.length && words.every(token => token.owners.every(owner => owner === null))) {
      mapping('generated module statement cannot acquire source', words[0].start + Math.min(1, words[0].text.length - 1), wrong);
    }
  }
  const changed = new Set();
  for (const token of tokens) {
    if (actual.owner(token.start) && ['+', '!==', '&&', '||'].includes(token.text) && !changed.has(token.text)) {
      changed.add(token.text); code('complete original source operator', token.start, token.end,
        token.text === '+' ? '-' : token.text === '!==' ? '===' : token.text === '&&' ? '||' : '&&');
    }
  }
  for (const comment of actual.comments()) {
    if (comment.text.startsWith('//# sourceMappingURL=')) continue;
    if (actual.owner(comment.start)) mapping('original comment exact UTF-16 interior', comment.start + 2, wrong);
    else mapping('generated comment cannot acquire source', comment.start + 2, wrong);
  }
  for (const [field, value] of [['file', 'forged.js'], ['names', ['forged']], ['sourceRoot', '../forged/']]) {
    rejected('complete module map metadata ' + field, stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), [field]: value})), 0);
  }
  const footer = actual.code.indexOf('//# sourceMappingURL='); assert(footer >= 0);
  const position = actual.source.getLineAndCharacterOfPosition(footer + 4), segments = actual.decoded.segments.map(point => [...point]);
  const index = segments.findIndex(point => point[0] > position.line || point[0] === position.line && point[1] >= position.character);
  segments.splice(index < 0 ? segments.length : index, 0, [position.line, position.character, ...wrong], [position.line, position.character]);
  rejected('hidden zero-width source grant cannot disappear from complete decoded map',
    stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), mappings: encode(segments)})), footer + 4);
  return controls;
}

module.exports = {run};
