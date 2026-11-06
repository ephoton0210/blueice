// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original method identity/formals and generated dynamic-receiver transports.
const assert = require('node:assert/strict');
const {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
const start = (view, node) => node.getStart(view.source);
function frames(ts, native, n, actual, a) {
  assert(!n.retained && !a.retained);
  const prefix = (view, item) => view.tokens(item.boxStatement).filter(token => token.end <= item.member.name.end);
  compareTokens(native, prefix(native, n), actual, prefix(actual, a), 'method object prefix and identity');
  const property = (view, item) => view.tokens(item.member).filter(token => token.start >= item.member.name.end && token.end <= start(view, item.method));
  const np = property(native, n), ap = property(actual, a);
  compareTokens(native, np, actual, ap, 'method property definition transport');
  if (np.length || ap.length) {
    assert(np.length && ap.length, 'matching property definition transport');
    compareGap(native, np.at(-1).end, start(native, n.method),
      actual, ap.at(-1).end, start(actual, a.method), 'method property and function boundary');
  }
  const cap = require('./ge5_async_method_core_frames.cjs').frames(ts, native, n, actual, a);
  const suffix = (view, item) => view.tokens(item.boxStatement).filter(token => token.start >= item.method.end);
  compareTokens(native, suffix(native, n), actual, suffix(actual, a), 'complete method object closing');
  compareGap(native, n.method.end, suffix(native, n)[0].start,
    actual, a.method.end, suffix(actual, a)[0].start, 'method/object closing boundary');
  return cap;
}
module.exports = {frames};
