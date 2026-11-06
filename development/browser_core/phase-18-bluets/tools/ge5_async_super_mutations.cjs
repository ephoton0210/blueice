// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Coherent mutations challenge every source role and the exact super routing.
const assert = require('node:assert/strict'), {stream} = require('./ge5_role_stream.cjs');
const {mutate, encode} = require('./ge5_role_mutations.cjs'), {coherent} = require('./ge5_async_arrow_guard_mutations.cjs');
function run(ts, actual, options, original, item, check) {
  const controls = [], start = node => node.getStart(actual.source);
  function reject(label, changed) {assert.throws(() => check(coherent(ts, changed)), undefined, label); controls.push({label, rejected: true, publicationCoherent: true});}
  function mapping(label, offset, owner) {assert.notDeepEqual(actual.owner(offset), owner, 'control changes a physical source role'); reject(label, mutate(ts, actual, offset, owner));}
  function code(label, node, text) {const from = start(node); assert(text.length <= node.end - from); reject(label, stream(ts, actual.code.slice(0, from) + text.padEnd(node.end - from, ' ') + actual.code.slice(node.end), actual.raw));}
  for (const [index, token] of actual.tokens(actual.source).entries()) if (actual.owner(token.start)) {
    mapping('erase source token ' + index, token.start, null);
    if (token.end > token.start + 1) mapping('erase source token interior ' + index, token.start + 1, null);
  }
  for (const [label, node] of [['await call endpoint', item.awaitCall], ['super member endpoint', item.superMember], ['super invocation endpoint', item.superCall]]) mapping(label, node.end, actual.owner(node.end) ? null : [0, 0, 0]);
  code('change awaited operation', item.awaitCall.expression.name, 'reject'); code('change awaited value', item.awaitCall.arguments[0], '0');
  code('change result method', item.resultCall.expression.name, 'read');
  if (!item.retained) {
    code('change invocation transport', item.superCall.expression.name, 'bind'); code('change super receiver', item.superCall.arguments[0], 'null');
    code('change apply receiver', item.apply.arguments[0], 'null'); code('change apply arguments', item.apply.arguments[1], 'undefined');
    code('change promise driver receiver', item.driver.arguments[0], 'null');
    mapping('grant apply transport source', start(item.apply.arguments[0]), [0, 0, 0]);
    mapping('grant driver helper source', start(item.driver.expression) + 1, [0, 0, 0]);
    if (options.target === 'ES5') {code('change Reflect operation', item.superMember.expression.name, 'set'); code('change original super key', item.superMember.arguments[1], '"nope"'); mapping('grant Reflect dispatch source', start(item.superMember) + 1, [0, 0, 0]);}
    else {code('change original getter key', item.superMember.arguments[0], '"nope"'); mapping('grant getter closing source', item.superMember.end - 1, [0, 0, 0]); mapping('grant bridge definition source', start(item.bridge) + 1, [0, 0, 0]);}
    for (const helper of item.helpers) mapping('grant private helper source ' + helper.name.text, start(helper.name) + 1, [0, 0, 0]);
  }
  const raw = JSON.parse(actual.raw);
  for (const [label, key, value] of [
    ['wrong output file', 'file', 'wrong.js'], ['wrong source root', 'sourceRoot', 'https://wrong.invalid/'],
    ['wrong source table', 'sources', ['../wrong.ts']], ['unexpected names', 'names', ['unreviewed']],
    ['wrong source content', 'sourcesContent', ['x' + original.slice(1)]], ['unknown metadata', 'unreviewed', true],
  ]) reject(label, stream(ts, actual.code, JSON.stringify({...raw, [key]: value})));
  mapping('grant map publication source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]);
  if (!options.inlineSourceMap) reject('retarget external map', stream(ts, actual.code.replace(/(\/\/# sourceMappingURL=)([^\r\n]+)/, (_, prefix, name) => prefix + 'z' + name.slice(1)), actual.raw));
  if (!Object.hasOwn(raw, 'sourcesContent')) {
    const segments = actual.decoded.segments.map(segment => [...segment]), index = segments.findIndex(segment => segment.length > 2), point = segments[index];
    segments.splice(index, 0, [point[0], point[1], 0, original.split('\n').length + 1, 0]);
    reject('hidden source coordinate outside bounds', stream(ts, actual.code, JSON.stringify({...raw, mappings: encode(segments)})));
  }
  return controls;
}
module.exports = {run};
