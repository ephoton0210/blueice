// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each control starts from a fresh accepted physical view and stays parseable/bounded.
const assert = require('node:assert/strict');
const tools = __dirname + '/';
const { stream } = require(tools + 'ge5_role_stream.cjs');
const { mutate, encode } = require(tools + 'ge5_role_mutations.cjs');
const { compare } = require('./ge5_k11_standard_module.cjs');
function run(ts, native, actual, options, original, proof) {
  const controls = [], claimed = new Set(native.decoded.segments.filter(p => p.length > 2).map(p => JSON.stringify(p.slice(2))));
  let wrong;
  for (const [line, text] of original.split('\n').entries()) {
    for (let column = 0; column <= text.length; column++) if (!claimed.has(JSON.stringify([0, line, column]))) { wrong = [0, line, column]; break; }
    if (wrong) break;
  }
  assert(wrong, 'a bounded source coordinate outside every Native source role');
  function fresh() { return stream(ts, actual.code, actual.raw); }
  function reject(label, kind, changed) {
    assert(changed.code !== actual.code || JSON.stringify(changed.decoded) !== JSON.stringify(actual.decoded), 'effective negative control');
    assert.throws(() => compare(ts, stream(ts, native.code, native.raw), changed, options, original), undefined, label);
    controls.push({ label, kind, validJavaScript: true, validBoundedMap: true, rejected: true });
  }
  function mapping(label, offset, owner) {
    assert.notDeepEqual(actual.owner(offset), owner, 'effective isolated map control');
    reject(label, 'map', mutate(ts, fresh(), offset, owner));
  }
  function code(label, start, end, replacement) {
    assert.equal(replacement.length, end - start, 'bounded code control preserves all generated coordinates');
    assert.notEqual(actual.code.slice(start, end), replacement);
    reject(label, 'code', stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw));
  }
  const roles = new Set();
  for (const token of proof.actualTokens) {
    const offset = token.start + Number(token.end - token.start > 1), owner = actual.owner(offset);
    if (!owner || roles.has(JSON.stringify(owner))) continue;
    roles.add(JSON.stringify(owner)); mapping('source role ' + JSON.stringify(owner) + ': erase UTF-16 interior', offset, null);
    mapping('source role ' + JSON.stringify(owner) + ': retarget UTF-16 interior', offset, wrong);
  }
  const endpoints = new Map();
  for (const point of proof.endpoints) {
    if (!endpoints.has(point.actual)) endpoints.set(point.actual, []); endpoints.get(point.actual).push(point.kind);
  }
  for (const [offset, kinds] of endpoints) {
    const label = 'logical AST endpoint ' + offset + ' (' + [...new Set(kinds)].join('/') + ')';
    mapping(label + ': wrong source', offset, wrong);
    if (actual.owner(offset)) mapping(label + ': erase required source', offset, null);
  }
  const tokens = actual.tokens(actual.source), gapPoints = new Set();
  for (let i = 1; i < tokens.length; i++) if (tokens[i - 1].end < tokens[i].start) {
    const start = tokens[i - 1].end, end = tokens[i].start;
    if (!gapPoints.has(start)) { gapPoints.add(start); mapping('complete physical gap ' + start + '..' + end + ': wrong role', start, wrong); }
  }
  for (const point of [0, actual.code.length - 1]) if (!gapPoints.has(point)) mapping('module boundary ' + point + ': wrong role', point, wrong);
  for (const [index, frame] of proof.classProofs.entries()) {
    for (const [plan, entry] of frame.actual.member.entries) {
      const name = entry.accessors.get('get').expression.name;
      mapping('method ' + plan.key + ': erase exact context getter name end', name.end, null);
      mapping('method ' + plan.key + ': retarget exact context getter name end', name.end, wrong);
      const flag = entry.context.properties.find(p => p.name.getText() === 'private').initializer;
      code('method ' + plan.key + ': alter context private flag', flag.getStart(actual.source), flag.end, 'true ');
      const array = frame.actual.metadata.statements.find(s => ts.isExpressionStatement(s) && ts.isBinaryExpression(s.expression) && s.expression.left.getText() === entry.decorators.text).expression.right;
      const element = require('./ge5_k11_standard_module.cjs').expression(ts, array.elements[0]);
      code('method ' + plan.key + ': alter original decorator identity', element.getStart(actual.source), element.getStart(actual.source) + 1, element.text[0] === 'z' ? 'y' : 'z');
      if (plan.decorators.length > 1) {
        const first = array.elements[0], last = array.elements.at(-1), separator = actual.code.slice(first.end, array.elements[1].getStart(actual.source));
        const reversed = [...array.elements].reverse().map(n => n.getText(actual.source)).join(separator);
        code('method ' + plan.key + ': reverse source decorator evaluation order', first.getStart(actual.source), last.end, reversed);
      }
    }
    const metadataStatements = [...frame.actual.metadata.statements], first = metadataStatements[1], second = metadataStatements[2];
    assert(first && second && first.end <= second.getStart(actual.source));
    const separator = actual.code.slice(first.end, second.getStart(actual.source));
    code('class ' + index + ': apply decoration before evaluating its array', first.getStart(actual.source), second.end, second.getText(actual.source) + separator + first.getText(actual.source));
    const literal = actual.tokens(frame.actual.methods[0]).find(token => token.text === '42');
    if (literal) code('class ' + index + ': change original method result', literal.start, literal.end, '99');
    if (frame.actual.nameSetter) {
      const node = frame.actual.nameSetter.expression.arguments[2].properties.find(p => p.name.getText() === 'value').initializer;
      code('class ' + index + ': forge inferred source class name', node.getStart(actual.source) + 1, node.getStart(actual.source) + 2, 'X');
      const classCall = frame.actual.classBinding.call, memberCall = [...frame.actual.member.entries.values()][0].call;
      const first = memberCall.parent, last = classCall.parent, middle = actual.code.slice(first.end, last.getStart(actual.source));
      code('class ' + index + ': replace class before member decoration', first.getStart(actual.source), last.end, last.getText(actual.source) + middle + first.getText(actual.source));
    }
  }
  for (const [index, node] of proof.helperProof.actual.definitions.entries()) {
    const token = actual.tokens(node).find(token => token.text === '++' || token.text === '--'); assert(token);
    code('generated helper ' + index + ': reverse algorithm advance', token.start, token.end, token.text === '++' ? '--' : '++');
    mapping('generated helper ' + index + ': grant user source', node.getStart(actual.source) + 1, wrong);
  }
  const sourceComments = actual.comments().filter(c => !c.text.startsWith('//# sourceMappingURL='));
  for (const [index, comment] of sourceComments.entries()) {
    mapping('source comment ' + index + ': retarget start', comment.start, wrong);
    mapping('source comment ' + index + ': retarget end', comment.end, wrong);
    if (actual.owner(comment.end)) mapping('source comment ' + index + ': erase mapped end', comment.end, null);
  }
  for (const [key, value] of [['file', 'forged.js'], ['sources', ['forged.ts']], ['names', ['forged']], ['sourceRoot', '../forged/']]) reject('complete metadata table ' + key, 'metadata', stream(ts, actual.code, JSON.stringify({ ...JSON.parse(actual.raw), [key]: value })));
  const footer = actual.comments().find(c => c.text.startsWith('//# sourceMappingURL='));
  mapping('map publication: grant generated comment source', footer.start + 4, wrong);
  const p = actual.source.getLineAndCharacterOfPosition(footer.start + 4), points = actual.decoded.segments.map(p => [...p]);
  let at = points.findIndex(point => point[0] > p.line || point[0] === p.line && point[1] >= p.character); if (at < 0) at = points.length;
  points.splice(at, 0, [p.line, p.character, ...wrong], [p.line, p.character]);
  reject('duplicate decoded entry: hidden zero-width source grant', 'transition', stream(ts, actual.code, JSON.stringify({ ...JSON.parse(actual.raw), mappings: encode(points) })));
  return controls;
}
module.exports = { run };
