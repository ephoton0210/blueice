// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Named ES5 async functions bind a separate, explicit awaiter ABI.
const assert = require('node:assert/strict');
function unwrap(ts, view, node, dialect) {
  assert.equal(node.body.statements.length, 1);
  const returned = node.body.statements[0]; assert(ts.isReturnStatement(returned) && ts.isCallExpression(returned.expression));
  const call = returned.expression, blue = dialect === 'blue';
  assert.match(call.expression.getText(), blue ? /^__blueice_target_async_driver_\d+$/ : /^__awaiter(?:_\d+)?$/);
  assert.equal(call.arguments.length, blue ? 3 : 4);
  assert.equal(call.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
  if (blue) assert.equal(call.arguments[1].getText(), 'arguments');
  else for (const argument of call.arguments.slice(1, 3)) assert.equal(argument.getText().replace(/\s/g, ''), 'void0');
  const callback = call.arguments.at(-1);
  assert(ts.isFunctionExpression(callback) && !callback.asteriskToken && !callback.modifiers?.length);
  assert.equal(node.parameters.length, 0); assert.equal(callback.parameters.length, 0);
  const outer = { node, returned, call, helper: call.expression.getText(), callback };
  return { callback, outer };
}
module.exports = { unwrap };
