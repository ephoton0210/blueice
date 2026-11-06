// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original operands, default routing and helper code each have independent controls.
const assert = require('node:assert/strict');
const { mutate } = require('./ge5_role_mutations.cjs'), { stream } = require('./ge5_role_stream.cjs');
function run(ts, native, actual, options, original, proof, compare) {
  const controls = [], item = proof.actual, start = node => node.getStart(actual.source);
  function mapping(label, offset, generated = false) {
    assert.equal(actual.owner(offset) === null, generated, label + ' precondition');
    const changed = mutate(ts, actual, offset, generated ? [0, 0, 0] : null);
    assert.throws(() => compare(ts, native, changed, options, original), undefined, label);
    controls.push({ label, rejected: true });
  }
  function code(label, node, text) {
    const first = start(node); assert(text.length <= node.end - first);
    const changed = stream(ts, actual.code.slice(0, first) + text.padEnd(node.end - first, ' ') + actual.code.slice(node.end), actual.raw);
    assert.throws(() => compare(ts, native, changed, options, original), undefined, label);
    controls.push({ label, rejected: true });
  }
  for (const [index, token] of actual.tokens(item.awaitCall).entries()) {
    mapping('erase awaited call token ' + index, token.start);
    if (token.end > token.start + 1) mapping('erase awaited call interior ' + index, token.start + 1);
  }
  mapping('erase callable binding interior', start(item.declaration.name) + 1);
  mapping('erase default operand', start(item.defaultValue));
  mapping('grant map comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, true);
  code('change awaited operation', item.awaitCall.expression.name, 'reject');
  code('change awaited operand', item.awaitCall.arguments[0], '3');
  code('change default operand', item.defaultValue, '0');
  if (item.defaultCondition) code('invert default selection', item.defaultCondition.operatorToken, '!==');
  if (item.helper) {
    mapping('grant helper name source', start(item.helper.name) + 1, true);
    const operations = [];
    function visit(node) { if (ts.isStringLiteral(node) && ['next', 'throw'].includes(node.text)) operations.push(node); ts.forEachChild(node, visit); }
    visit(item.helper); assert(operations.length > 0);
    code('corrupt helper resume operation', operations[0], '"x"');
  }
  return controls;
}
module.exports = { run };
