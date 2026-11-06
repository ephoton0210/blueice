// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Valid, coherent controls challenge source roles and dynamic receiver routing.
const assert = require('node:assert/strict');
const {stream} = require('./ge5_role_stream.cjs');
const {mutate, encode} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_async_arrow_guard_mutations.cjs');
function run(ts, actual, options, original, item, check) {
  const controls = [], start = node => node.getStart(actual.source);
  function reject(label, changed) {
    assert.throws(() => check(coherent(ts, changed)), undefined, label);
    controls.push({label, rejected: true, publicationCoherent: true});
  }
  function mapping(label, offset, owner) {
    assert.notDeepEqual(actual.owner(offset), owner, 'control must change a physical source role');
    reject(label, mutate(ts, actual, offset, owner));
  }
  function code(label, node, text) {
    const from = start(node); assert(text.length <= node.end - from);
    reject(label, stream(ts, actual.code.slice(0, from) + text.padEnd(node.end - from, ' ') + actual.code.slice(node.end), actual.raw));
  }
  const container = item.boxStatement ?? item.classNode; assert(container);
  for (const [index, token] of [...actual.tokens(container), ...actual.tokens(item.result)].entries()) {
    if (!actual.owner(token.start)) continue;
    mapping('erase source token ' + index, token.start, null);
    if (token.end > token.start + 1) mapping('erase source token interior ' + index, token.start + 1, null);
  }
  const calls = [];
  function visit(node) {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
      && node.expression.expression.getText(actual.source) === 'Promise') calls.push(node);
    ts.forEachChild(node, visit);
  }
  visit(item.callback.body); assert.equal(calls.length, 1); const awaited = calls[0];
  mapping('change awaited operand ending', awaited.end, actual.owner(awaited.end) ? null : [0, 0, 0]);
  mapping('grant map publication comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]);
  code('change awaited operation', awaited.expression.name, 'reject');
  code('change awaited argument', awaited.arguments[0], '0');
  code('change method result argument', item.resultCall.arguments[0], '0');
  const value = ts.isPropertyAssignment(item.value) ? item.value.initializer : item.value;
  code('change original receiver value', value, '41');
  if (!item.retained) {
    for (const [label, node, replacement] of [
      ['change apply receiver', item.apply.arguments[0], 'null'],
      ['change apply argument list', item.apply.arguments[1], 'undefined'],
      ['change apply transport operation', item.apply.expression.name, 'call'],
      ['change awaiter receiver', item.driver.arguments[0], 'null'],
      ['change awaiter arguments', item.driver.arguments[1], 'undefined'],
    ]) code(label, node, replacement);
    for (const [label, node] of [
      ['grant apply receiver source', item.apply.arguments[0]],
      ['grant arity parameter source', item.method.parameters[0].name],
      ['grant generated callable identity source', item.entry.name],
      ['grant awaiter identifier source', item.driver.expression],
    ]) mapping(label, start(node) + 1, [0, 0, 0]);
    const completionOwner = actual.owner(item.callback.end);
    mapping('change callback logical endpoint', item.callback.end, completionOwner ? null : [0, 0, 0]);
    for (const helper of item.helpers) mapping('grant generated helper source', start(helper.name) + 1, [0, 0, 0]);
    if (item.guard) mapping('grant constructor guard source', start(item.guard) + 1, [0, 0, 0]);
    code('change relocated runtime formal', item.callback.parameters[0].name, 'other');
  }
  const raw = JSON.parse(actual.raw);
  for (const [label, key, value] of [
    ['wrong artifact file', 'file', 'wrong.js'], ['wrong source root', 'sourceRoot', 'https://wrong.invalid/'],
    ['unexpected names', 'names', ['unreviewed']], ['wrong source table', 'sources', ['../wrong.ts']],
    ['wrong or added source content', 'sourcesContent', [Object.hasOwn(raw, 'sourcesContent') ? 'x' + original.slice(1) : original]],
    ['unreviewed metadata', 'unreviewed', true],
  ]) reject(label, stream(ts, actual.code, JSON.stringify({...raw, [key]: value})));
  if (!options.inlineSourceMap) {
    const changed = actual.code.replace(/(\/\/# sourceMappingURL=)([^\r\n]+)/, (_, prefix, name) => prefix + 'z' + name.slice(1));
    assert.notEqual(changed, actual.code); reject('retarget external map publication', stream(ts, changed, actual.raw));
  }
  if (!Object.hasOwn(raw, 'sourcesContent')) {
    const segments = actual.decoded.segments.map(segment => [...segment]), index = segments.findIndex(segment => segment.length > 2), point = segments[index]; assert(point);
    segments.splice(index, 0, [point[0], point[1], 0, original.split('\n').length + 1, 0]);
    const changed = stream(ts, actual.code, JSON.stringify({...raw, mappings: encode(segments)}));
    for (const token of actual.tokens(actual.source)) assert.deepEqual(changed.owner(token.start), actual.owner(token.start));
    reject('shadowed coordinate outside source bounds', changed);
  }
  return controls;
}
module.exports = {run};
