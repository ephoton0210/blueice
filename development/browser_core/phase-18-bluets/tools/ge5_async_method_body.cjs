// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A source await in a binary operand requires only compiler precedence groups.
const assert = require('node:assert/strict');
const {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
const start = (view, node) => node.getStart(view.source);
function compare(ts, native, n, actual, a) {
  const sourceBody = n.originalMethod.body;
  assert.equal(sourceBody.statements.length, 1);
  const sourceReturn = sourceBody.statements[0]; assert(ts.isReturnStatement(sourceReturn));
  const sourceBinary = sourceReturn.expression;
  assert(ts.isBinaryExpression(sourceBinary) && sourceBinary.operatorToken.kind === ts.SyntaxKind.PlusToken);
  assert(ts.isPropertyAccessExpression(sourceBinary.left) && sourceBinary.left.expression.kind === ts.SyntaxKind.ThisKeyword);
  assert(ts.isAwaitExpression(sourceBinary.right) && ts.isCallExpression(sourceBinary.right.expression));
  function body(view, item, blue) {
    assert.equal(item.callback.body.statements.length, 1);
    const returned = item.callback.body.statements[0]; assert(ts.isReturnStatement(returned));
    const binary = returned.expression;
    assert(ts.isBinaryExpression(binary) && binary.operatorToken.kind === sourceBinary.operatorToken.kind);
    assert(ts.isPropertyAccessExpression(binary.left) && binary.left.expression.kind === ts.SyntaxKind.ThisKeyword);
    assert.equal(binary.left.name.text, sourceBinary.left.name.text);
    assert(ts.isParenthesizedExpression(binary.right) && ts.isYieldExpression(binary.right.expression));
    const grouping = [], yieldNode = binary.right.expression;
    assert(!yieldNode.asteriskToken && yieldNode.expression);
    const delimiters = node => [[start(view, node), start(view, node) + 1], [node.end - 1, node.end]];
    grouping.push(...delimiters(binary.right));
    let operand = yieldNode.expression;
    if (blue) { assert(ts.isParenthesizedExpression(operand)); grouping.push(...delimiters(operand)); operand = operand.expression; }
    assert(ts.isCallExpression(operand) && ts.isPropertyAccessExpression(operand.expression));
    assert.equal(operand.expression.getText(view.source), sourceBinary.right.expression.expression.getText(n.source));
    assert.equal(operand.arguments.length, sourceBinary.right.expression.arguments.length);
    for (let index = 0; index < operand.arguments.length; index++) {
      assert.equal(operand.arguments[index].getText(view.source), sourceBinary.right.expression.arguments[index].getText(n.source));
    }
    if (blue) for (const range of grouping) view.noSource(...range, 'additional await precedence delimiter');
    else {
      const owners = [view.owner(start(view, binary.operatorToken)), view.owner(operand.end)];
      for (let index = 0; index < grouping.length; index++) {
        const [from, to] = grouping[index];
        for (let offset = from; offset < to; offset++) assert.deepEqual(view.owner(offset), owners[index], 'exact Native await precedence boundary');
        view.cover(from, to);
      }
    }
    const tokens = view.tokens(item.callback.body).filter(token => token.start > start(view, item.callback.body)
      && !grouping.some(([from, to]) => token.start >= from && token.end <= to));
    return {returned, grouping, tokens};
  }
  const left = body(native, n, false), right = body(actual, a, true);
  compareTokens(native, left.tokens, actual, right.tokens, 'complete method generator body', right.grouping);
  compareGap(native, start(native, n.callback.body) + 1, start(native, left.returned),
    actual, start(actual, a.callback.body) + 1, start(actual, right.returned), 'method body entry trivia');
}
module.exports = {compare};
