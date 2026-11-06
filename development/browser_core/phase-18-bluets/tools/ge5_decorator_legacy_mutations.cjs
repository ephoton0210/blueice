// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every mutation keeps a valid bounded map and a coherent inline publication.
const assert = require('node:assert/strict');
const {stream} = require('./ge5_role_stream.cjs'), {mutate, encode} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_decorator_preserved_mutations.cjs'), {compare} = require('./ge5_decorator_legacy_module.cjs');
function run(ts, native, actual, options, original, proof) {
  const check = (ts, n, a, options, source) => compare(ts, n, coherent(ts, a), options, source);
  const controls = require('./ge5_class_mutations.cjs').run(ts, native, actual, options, original, proof, check);
  const lines = original.split('\n'), wrong = [0, lines.length - 1, lines.at(-1).length];
  function rejects(label, changed) {
    const valid = coherent(ts, changed);
    assert.throws(() => compare(ts, stream(ts, native.code, native.raw), valid, options, original), undefined, label);
    controls.push({label, rejected: true});
  }
  function mapping(label, offset, owner) { assert.notDeepEqual(actual.owner(offset), owner, 'effective map control'); rejects(label, mutate(ts, actual, offset, owner)); }
  function code(label, start, end, replacement) {
    assert.equal(replacement.length, end - start); rejects(label, stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw));
  }
  const phases = proof.delegated;
  for (const [index, pair] of phases.pairs.entries()) {
    const tokens = actual.tokens(pair.actual), retained = tokens.find(token => token.text.length > 1 && token.owners.every(Boolean));
    assert(retained); mapping('decorator ' + index + ': retarget source interior', retained.start + 1, wrong);
    mapping('decorator ' + index + ': erase source interior', retained.start + 1, null);
    const zero = tokens.find(token => token.text === '0');
    if (zero) code('decorator ' + index + ': wrong parameter index', zero.start, zero.end, '1');
    let statement = pair.actual;
    while (!ts.isExpressionStatement(statement)) { statement = statement.parent; assert(statement, 'decorator is a complete initialization statement'); }
    const begin = statement.getStart(actual.source), end = statement.end;
    code('decorator ' + index + ': omit the complete class initialization phase', begin, end, actual.code.slice(begin, end).replace(/[^\r\n]/g, ' '));
  }
  for (const [index, node] of phases.actual.definitions.entries()) {
    mapping('decorator helper ' + index + ': grant generated body source', node.getStart(actual.source) + 1, [0, 0, 0]);
    const operation = actual.tokens(node).find(token => token.text === '--' || token.text === '++');
    if (operation) code('decorator helper ' + index + ': invert helper iteration', operation.start, operation.end, operation.text === '--' ? '++' : '--');
  }
  for (const [index, fn] of phases.functions.entries()) {
    if (!fn.rest) continue;
    const {array, loop, copy} = fn.rest.actual;
    mapping('rest ' + index + ': erase original array prologue', array.getStart(actual.source) + 1, null);
    mapping('rest ' + index + ': grant generated loop source', loop.getStart(actual.source), [0, 0, 0]);
    code('rest ' + index + ': invert argument bound', loop.condition.operatorToken.getStart(actual.source), loop.condition.operatorToken.end, '>');
    const increment = loop.incrementor; code('rest ' + index + ': reverse argument advance', increment.end - 2, increment.end, '--');
    const receiver = copy.expression.right.expression; code('rest ' + index + ': read wrong argument storage', receiver.getStart(actual.source), receiver.end, 'argumentX');
  }
  for (const [index, node] of phases.publications.entries()) {
    code('publication ' + index + ': omit final replacement update', node.getStart(actual.source), node.end, actual.code.slice(node.getStart(actual.source), node.end).replace(/[^\r\n]/g, ' '));
  }
  for (const node of proof.actual.sourceNodes) {
    if (!ts.isExportDeclaration(node) || !node.exportClause || !ts.isNamedExports(node.exportClause)) continue;
    for (const element of node.exportClause.elements) {
      const token = actual.token(element.name), offset = token.start + Number(token.end - token.start > 1);
      mapping('ESM publication: retarget complete binding interior', offset, wrong);
      if (actual.owner(offset)) mapping('ESM publication: erase original class binding interior', offset, null);
    }
  }
  const comments = actual.comments();
  for (const [index, comment] of comments.entries()) {
    if (!comment.text.startsWith('/* G-E5')) continue;
    mapping('source comment ' + index + ': retarget complete endpoint', comment.end, wrong);
    mapping('source comment ' + index + ': erase complete endpoint', comment.end, null);
  }
  const footer = comments.find(comment => comment.text.startsWith('//# sourceMappingURL=')); assert(footer);
  const segments = actual.decoded.segments.map(point => [...point]);
  const position = actual.source.getLineAndCharacterOfPosition(footer.start + 4), point = [position.line, position.character];
  const insertion = segments.findIndex(segment => segment[0] > point[0] || segment[0] === point[0] && segment[1] >= point[1]);
  segments.splice(insertion < 0 ? segments.length : insertion, 0, [...point, ...wrong], [...point]);
  rejects('hidden zero-width generated source grant', stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), mappings: encode(segments)})));
  for (const [key, value] of [['names', ['forged']], ['sourceRoot', '../forged/']]) rejects('complete map metadata ' + key, stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), [key]: value})));
  rejects('extra unreviewed generated comment', stream(ts, actual.code + '\n/* extra generated effect */\n', actual.raw));
  return controls;
}
module.exports = {run};
