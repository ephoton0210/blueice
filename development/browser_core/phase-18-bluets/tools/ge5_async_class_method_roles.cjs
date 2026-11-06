// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A proved callable subtree delegates to the enclosing complete class/module.
const assert = require('node:assert/strict');
const {read} = require('./ge5_async_class_method_ir.cjs');
const start = (view, node) => node.getStart(view.source);
function method(ts, native, n, actual, a, options, original) {
  assert.equal(a.retained, n.retained);
  if (n.retained) {
    require('./ge5_role_stream.cjs').compareTokens(native, native.tokens(n.method),
      actual, actual.tokens(a.method), 'complete retained async class method');
  } else {
    const cap = require('./ge5_async_method_core_frames.cjs').frames(ts, native, n, actual, a, 'async-class-method');
    if (options.target === 'ES5') {
      const machine = require('./ge5_continuation_machine.cjs');
      const nm = machine.read(ts, native, n.callback, 'native', [original]), am = machine.read(ts, actual, a.callback, 'blue', [original]);
      nm.reviewedSignature = cap;
      require('./ge5_linear_roles.cjs').compareFunction(ts, native, nm, actual, am);
    } else require('./ge5_async_method_body.cjs').compare(ts, native, n, actual, a);
  }
  require('./ge5_async_operand_endpoints.cjs').compare(ts, native, n, actual, a, options);
  for (const [view, item] of [[native, n], [actual, a]]) {
    for (let offset = start(view, item.method); offset < item.method.end; offset++) if (!view.covered(offset)) {
      assert(/\s/.test(view.code[offset]), 'unclassified async class method syntax at ' + offset);
      assert.equal(view.owner(offset), null, 'unclassified async class method source role at ' + offset);
      view.cover(offset, offset + 1);
    }
  }
}
function compare(ts, native, actual, options, original) {
  require('./ge5_async_method_roles.cjs').authority(native, actual, options, original);
  let n, a;
  const module = require('./ge5_class_module.cjs').compare(ts, native, actual, options, original, {
    prepare({n: ne, a: ae, original: source}) {
      n = read(ts, native, options, source, false, ne); a = read(ts, actual, options, source, true, ae);
      n.helpers = require('./ge5_async_class_method_helpers.cjs').verify(ts, native, n, options);
      a.helpers = require('./ge5_async_class_method_helpers.cjs').verify(ts, actual, a, options);
      method(ts, native, n, actual, a, options, original);
    },
    classRoles(source, nn, an) {
      assert.equal(source, n.originalClass); assert.equal(nn, n.classNode); assert.equal(an, a.classNode);
      return {pairedCallables: [{source: n.originalMethod, native: n.method, actual: a.method}]};
    },
  });
  return {native: n, actual: a, module};
}
module.exports = {compare};
