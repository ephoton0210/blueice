// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A lowered original method retains Native's assignment mode and exact receiver.
const assert = require('node:assert/strict');
function compare(ts, native, n, actual, a, contract) {
  const [nm] = n.classRead.methods, [am] = a.classRead.methods;
  assert(nm && am && !nm.descriptor && !am.descriptor, 'paired Native method assignments');
  const name = n.member.name.text;
  assert.equal(nm.key.text, name, 'original Native method property');
  assert.equal(am.key.text, name, 'original actual method property');
  for (const [method, item] of [[nm, n], [am, a]]) {
    const receiver = method.target.expression;
    assert(ts.isPropertyAccessExpression(receiver) && ts.isIdentifier(receiver.expression)
      && receiver.expression.text === item.classRead.name && receiver.name.text === 'prototype',
    'exact original method prototype receiver');
    assert.equal(method.operation.kind, ts.SyntaxKind.EqualsToken, 'original method assignment operation');
  }
  const aliases = new Map([[a.classRead.name, n.classRead.name]]);
  if (n.classRead.base) {
    assert(a.classRead.base); aliases.set(a.classRead.base, n.classRead.base);
  }
  contract.pair(native.tokens(nm.statement), actual.tokens(am.statement),
    'complete original method assignment reference/signature/body/terminator', aliases);
  for (const [label, left, right] of [['property', nm.key, am.key], ['reference', nm.target, am.target],
    ['function', nm.fn, am.fn], ['body', nm.fn.body, am.fn.body], ['definition', nm.statement, am.statement]]) {
    assert.deepEqual(actual.owner(right.end), native.owner(left.end),
      'original method assignment logical endpoint ' + label);
  }
}
module.exports = {compare};
