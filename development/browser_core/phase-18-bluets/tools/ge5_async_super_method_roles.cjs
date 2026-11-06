// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A complete derived callable composes with the strict Base/Derived/module ABI.
const assert = require('node:assert/strict'), {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
const start = (view, node) => node.getStart(view.source);
function midBody(ts, native, n, actual, a) {
  assert.equal(n.callback.body.statements.length, 2); assert.equal(a.callback.body.statements.length, 2);
  const statement = (view, item) => {
    const declaration = item.callback.body.statements[0]; assert(ts.isVariableStatement(declaration) && declaration.declarationList.declarations.length === 1);
    const extra = declaration.declarationList.declarations[0]; assert.equal(extra.name.getText(view.source), 'extra');
    assert(ts.isYieldExpression(extra.initializer) || ts.isParenthesizedExpression(extra.initializer));
    const returned = item.callback.body.statements[1]; assert(ts.isReturnStatement(returned) && ts.isBinaryExpression(returned.expression));
    assert.equal(returned.expression.operatorToken.kind, ts.SyntaxKind.PlusToken); assert.equal(returned.expression.left, item.superCall); assert.equal(returned.expression.right.getText(view.source), 'extra');
    return {declaration, extra, returned};
  };
  const nn = statement(native, n), aa = statement(actual, a), groups = [];
  const yieldNode = (view, node, blue) => {
    let current = node.initializer;
    while (ts.isParenthesizedExpression(current)) {
      const range = [[start(view, current), start(view, current) + 1], [current.end - 1, current.end]];
      assert(blue, 'Native standalone await needs no precedence grouping');
      for (const [from, to] of range) {view.noSource(from, to, 'generated standalone await grouping'); groups.push([from, to]);}
      current = current.expression;
    }
    assert(ts.isYieldExpression(current) && !current.asteriskToken); let operand = current.expression;
    if (blue) {
      assert(ts.isParenthesizedExpression(operand));
      for (const [from, to] of [[start(view, operand), start(view, operand) + 1], [operand.end - 1, operand.end]]) {view.noSource(from, to, 'generated await operand grouping'); groups.push([from, to]);}
      operand = operand.expression;
    }
    assert.equal(operand, blue ? a.awaitCall : n.awaitCall); return current;
  };
  yieldNode(native, nn.extra, false); yieldNode(actual, aa.extra, true);
  const tokens = (view, node) => view.tokens(node).filter(token => !groups.some(([from, to]) => view === actual && from <= token.start && token.end <= to));
  compareTokens(native, tokens(native, nn.declaration), actual, tokens(actual, aa.declaration), 'complete original awaited binding declaration', groups);
  const suffix = (view, item, returned) => view.tokens(returned).filter(token => token.start >= item.superCall.end);
  compareTokens(native, [native.tokens(nn.returned)[0]], actual, [actual.tokens(aa.returned)[0]], 'original super return keyword');
  compareGap(native, native.tokens(nn.returned)[0].end, start(native, n.superCall), actual, actual.tokens(aa.returned)[0].end, start(actual, a.superCall), 'return to source-bound super lookup gap');
  compareTokens(native, suffix(native, n, nn.returned), actual, suffix(actual, a, aa.returned), 'complete super return operand, operator and terminator');
  compareGap(native, n.superCall.end, suffix(native, n, nn.returned)[0].start, actual, a.superCall.end, suffix(actual, a, aa.returned)[0].start, 'super call to binary operator gap');
  compareTokens(native, [native.tokens(n.callback.body).at(-1)], actual, [actual.tokens(a.callback.body).at(-1)], 'complete async-super callback ending');
  compareGap(native, nn.returned.end, native.tokens(n.callback.body).at(-1).start,
    actual, aa.returned.end, actual.tokens(a.callback.body).at(-1).start, 'source return to callback closing boundary');
  compareGap(native, start(native, n.callback.body) + 1, start(native, nn.declaration), actual, start(actual, a.callback.body) + 1, start(actual, aa.declaration), 'async-super callback first gap');
  compareGap(native, nn.declaration.end, start(native, nn.returned), actual, aa.declaration.end, start(actual, aa.returned), 'awaited binding to return boundary');
}
function compare(ts, native, actual, options, original) {
  require('./ge5_async_method_roles.cjs').authority(native, actual, options, original);
  let n, a;
  const module = require('./ge5_class_module.cjs').compare(ts, native, actual, options, original, {
    prepare({n: ne, a: ae, original: source}) {
      n = require('./ge5_async_super_method_ir.cjs').read(ts, native, options, source, false, ne);
      a = require('./ge5_async_super_method_ir.cjs').read(ts, actual, options, source, true, ae);
      n.helpers = require('./ge5_async_class_method_helpers.cjs').verify(ts, native, n, options);
      a.helpers = require('./ge5_async_class_method_helpers.cjs').verify(ts, actual, a, options);
      if (n.retained) compareTokens(native, native.tokens(n.method), actual, actual.tokens(a.method), 'complete retained async-super method');
      else {
        const member = require('./ge5_async_super_member_roles.cjs').prove(ts, native, n, actual, a, options);
        const signature = require('./ge5_async_method_core_frames.cjs').frames(ts, native, n, actual, a, 'async-class-method');
        if (options.target === 'ES5') {
          const machine = require('./ge5_continuation_machine.cjs');
          const nm = machine.read(ts, native, n.callback, 'native', [original], {superMember: member}), am = machine.read(ts, actual, a.callback, 'blue', [original], {superMember: member});
          nm.reviewedSignature = signature;
          require('./ge5_linear_roles.cjs').compareFunction(ts, native, nm, actual, am);
        } else midBody(ts, native, n, actual, a);
      }
      require('./ge5_async_operand_endpoints.cjs').compare(ts, native, n, actual, a, options);
      for (const [view, item] of [[native, n], [actual, a]]) for (let offset = start(view, item.method); offset < item.method.end; offset++) if (!view.covered(offset)) {
        assert(/\s/.test(view.code[offset]), 'unclassified async-super syntax at ' + offset);
        assert.equal(view.owner(offset), null, 'unclassified async-super source role at ' + offset); view.cover(offset, offset + 1);
      }
    },
    classRoles(source) {
      return source === n.originalClass ? {pairedCallables: [{source: n.originalMethod, native: n.method, actual: a.method}]} : {};
    },
  });
  return {native: n, actual: a, module};
}
module.exports = {compare};
