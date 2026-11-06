// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each effective attack reopens the complete Native contract with fresh views.
const assert = require('node:assert/strict');
const {stream} = require('./ge5_role_stream.cjs'), {mutate, encode} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_decorator_preserved_mutations.cjs');
const {compare} = require('./ge5_decorator_member_module.cjs');
function run(ts, native, actual, options, original, proof) {
  const controls = [], claimed = new Set(native.decoded.segments.filter(point => point.length > 2)
    .map(point => JSON.stringify(point.slice(2))));
  const column = Array.from({length: original.split('\n')[0].length + 1}, (_, index) => index)
    .find(index => !claimed.has(JSON.stringify([0, 0, index])));
  assert.notEqual(column, undefined, 'bounded source interior outside Native roles');
  const wrong = [0, 0, column];
  function check(view) {
    return compare(ts, stream(ts, native.code, native.raw), stream(ts, view.code, view.raw), options, original);
  }
  check(actual);
  function reject(label, changed) {
    assert(changed.code !== actual.code || changed.raw !== actual.raw, 'effective counterfactual');
    changed = coherent(ts, changed);
    assert.throws(() => check(changed), undefined, label);
    controls.push({label, rejected: true, freshWholeModuleContract: true});
  }
  function mapping(label, offset, owner) {
    assert.notDeepEqual(actual.owner(offset), owner, 'effective isolated map mutation');
    reject(label, mutate(ts, actual, offset, owner));
  }
  function code(label, start, end, replacement) {
    assert.equal(replacement.length, end - start, 'fixed-width valid code control');
    assert.notEqual(actual.code.slice(start, end), replacement);
    reject(label, stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw));
  }
  const components = proof.delegated?.components;
  const methodProof = components?.methodMembers;
  const points = [...(methodProof?.pairs || []), ...(components?.constructionChain?.pairs || []),
    ...(proof.constructionChain?.pairs || [])];
  if (proof.endpoints) for (const point of proof.endpoints.points) points.push({label: point.kind, actual: {end: point.actual}});
  const ends = new Set();
  for (const point of points) {
    const offset = point.actual.end; if (ends.has(offset)) continue; ends.add(offset);
    mapping('exact original callable/construction end ' + point.label, offset, wrong);
    if (actual.owner(offset)) mapping('erase original callable/construction end ' + point.label, offset, null);
  }
  assert(ends.size >= 10, 'complete callable/construction endpoint controls');
  const roles = new Set(), gaps = new Set(), tokens = actual.tokens(actual.source);
  for (const token of tokens) {
    const offset = token.start + Number(token.end - token.start > 1), owner = actual.owner(offset);
    const key = JSON.stringify(owner);
    if (owner && !roles.has(key)) {
      roles.add(key); mapping('retarget full UTF-16 source role ' + key, offset, wrong);
      mapping('erase full UTF-16 source role ' + key, offset, null);
    }
  }
  for (let index = 1; index < tokens.length; index++) {
    const previous = tokens[index - 1], token = tokens[index];
    if (previous.end >= token.start) continue;
    const key = JSON.stringify(actual.owner(previous.end));
    if (!gaps.has(key)) { gaps.add(key); mapping('retarget ordered physical gap ' + key, previous.end, wrong); }
  }
  let methods = methodProof?.actual.methods;
  if (!methods) {
    const source = ts.createSourceFile('original.ts', original, 99, true).statements.find(ts.isClassDeclaration);
    const typed = require('./ge5_decorator_method_members.cjs'), cap = typed.source(ts, source);
    methods = typed.read(ts, actual, require('./ge5_decorator_member_core.cjs').read(ts, actual, source, options), cap).methods;
  }
  if (methods) for (const [index, method] of methods.entries()) {
    const key = method.keyNode, start = key.getStart(actual.source), text = key.getText(actual.source);
    const offset = ts.isNumericLiteral(key) ? key.end - 1 : start + 1;
    code('replace exact original literal key ' + index, offset, offset + 1,
      actual.code[offset] === '9' ? '8' : '9');
    const literal = actual.tokens(method.fn.body).find(token => /^[123]$/.test(token.text));
    assert(literal); code('change original method result ' + index, literal.start, literal.end, '9');
    if (ts.isFunctionExpression(method.fn)) {
      const keyword = actual.tokens(method.fn)[0]; assert.equal(actual.code[keyword.end], ' ');
      code('replace ordinary method with generator ' + index, keyword.end, keyword.end + 1, '*');
    }
  }
  const footer = actual.comments().find(comment => comment.text.startsWith('//# sourceMappingURL='));
  assert(footer); mapping('generated publication acquires source authority', footer.start + 4, wrong);
  for (const [index, comment] of actual.comments().entries()) if (comment.text.startsWith('/* G-E5')) {
    for (let offset = comment.start; offset < comment.end; offset++) mapping('original comment ' + index + '/' + offset, offset, wrong);
    mapping('original comment exact endpoint', comment.end, wrong);
  }
  const position = actual.source.getLineAndCharacterOfPosition(footer.start + 4);
  const point = [position.line, position.character], segments = actual.decoded.segments.map(point => [...point]);
  let at = segments.findIndex(segment => segment[0] > point[0] || segment[0] === point[0] && segment[1] >= point[1]);
  if (at < 0) at = segments.length;
  segments.splice(at, 0, [...point, ...wrong], [...point]);
  reject('hidden zero-width source grant', stream(ts, actual.code,
    JSON.stringify({...JSON.parse(actual.raw), mappings: encode(segments)})));
  for (const [key, value] of [['names', ['forged']], ['sourceRoot', '../forged/'], ['file', 'forged.js'],
    ['sources', ['forged.ts']], ['sourcesContent', [original + ' ']]]) {
    reject('complete metadata ' + key, stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), [key]: value})));
  }
  reject('unexpected complete comment inventory', stream(ts, actual.code + '\n/* extra effect */\n', actual.raw));
  return controls;
}
module.exports = {run};
