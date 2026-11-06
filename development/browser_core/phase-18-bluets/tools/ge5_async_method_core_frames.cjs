// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original method identity/formals and generated dynamic-receiver transports.
const assert = require('node:assert/strict');
const {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
const start = (view, node) => node.getStart(view.source);
function frames(ts, native, n, actual, a, sourceKind = 'async-object-method') {
  assert(!n.retained && !a.retained);
  const bodyOpen = (view, item) => start(view, item.method.body);
  const header = (view, item) => view.tokens(item.method).filter(token => token.end <= bodyOpen(view, item) + 1);
  const nh = header(native, n), ah = header(actual, a);
  const withoutFormal = (view, item, tokens) => tokens.filter(token => !item.method.parameters.some(parameter =>
    token.start >= start(view, parameter) && token.end <= parameter.end));
  const actualParameters = a.method.parameters.map(parameter => [start(actual, parameter), parameter.end]);
  for (const range of actualParameters) actual.noSource(...range, 'generated method arity parameter');
  compareTokens(native, withoutFormal(native, n, nh), actual, withoutFormal(actual, a, ah),
    'original method header and body entry', actualParameters);
  const signature = nh.slice(nh.findIndex(token => token.text === '('));
  const callback = actual.tokens(a.callback).filter(token => token.end <= start(actual, a.callback.body) + 1);
  const callbackOpen = callback.findIndex(token => token.text === '('); assert(callbackOpen > 0);
  compareTokens(native, signature, actual, callback.slice(callbackOpen), 'relocated complete method signature');
  actual.noSource(start(actual, a.callback), callback[callbackOpen].start, 'generated method callback keyword');
  function uniform(view, from, to, owner, label) {
    for (let offset = from; offset < to; offset++) assert.deepEqual(view.owner(offset), owner, label + ': ' + offset);
    view.cover(from, to);
  }
  const entryLineEnd = native.code.indexOf('\n', bodyOpen(native, n)) + 1;
  assert(entryLineEnd > bodyOpen(native, n) && entryLineEnd <= start(native, n.returned));
  uniform(native, bodyOpen(native, n) + 1, entryLineEnd, native.owner(bodyOpen(native, n)),
    'Native method signature trivia');
  native.noSource(entryLineEnd, start(native, n.returned), 'generated Native method indentation');
  native.noSource(start(native, n.returned), start(native, n.callback.body) + 1, 'Native method awaiter transport');
  actual.noSource(bodyOpen(actual, a) + 1, start(actual, a.callback), 'method apply and awaiter setup');
  const completion = native.owner(n.callback.end);
  uniform(native, n.callback.end, n.returned.end, completion, 'Native method awaiter completion');
  uniform(actual, a.callback.end, a.returned.end, completion, 'method awaiter completion');
  const close = (view, node) => view.tokens(node).at(-1);
  compareTokens(native, [close(native, n.method)], actual, [close(actual, a.entry)], 'generated method entry closing');
  compareGap(native, n.returned.end, close(native, n.method).start,
    actual, a.returned.end, close(actual, a.entry).start, 'method completion and entry closing boundary');
  actual.noSource(a.entry.end, a.method.end, 'method apply and outer closing');
  return {
    kind: 'verified-relocated-callable-signature-v1', sourceKind, source: n.originalMethod,
    nativeOuter: n.method, actualOuter: a.method, nativeCallback: n.callback, actualCallback: a.callback,
    formalPairs: n.parameters.map((source, index) => ({source, native: n.method.parameters[index], actual: a.callback.parameters[index]})),
    ranges: [[start(native, n.callback), start(native, n.callback.body) + 1],
      [start(actual, a.callback), start(actual, a.callback.body) + 1]],
  };
}
module.exports = {frames};
