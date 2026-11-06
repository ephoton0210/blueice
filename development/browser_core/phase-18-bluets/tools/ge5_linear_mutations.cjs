// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Mapping mutations remain bounded; semantic mutations preserve generated width.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs'), { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
function run(ts, native, actual, originals, proof) {
  const trace = proof.actual, returned = trace.events.at(-1), binary = returned.value;
  assert.equal(binary.kind, 'binary');
  const group = binary.left; assert.equal(group.kind, 'group'); assert.equal(group.inner.kind, 'sent');
  const fn = trace.machine, helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === fn.helper);
  assert(helper);
  const tests = [
    ['retarget arithmetic operator', binary.node.operatorToken.getStart(actual.source), [0, 0, 0]],
    ['erase arithmetic operator', binary.node.operatorToken.getStart(actual.source), null],
    ['erase original grouping open', group.node.getStart(actual.source), null],
    ['erase original grouping close', group.node.end - 1, null],
    ['retarget resumed value interior', group.inner.node.getStart(actual.source) + 1, [0, 0, 0]],
    ['erase resumed value endpoint', group.inner.node.end, null],
    ['erase carried value source', group.uses[0].getStart(actual.source) + 1, null],
    ['erase return opcode endpoint', returned.root.end - 1, null],
    ['retarget return statement endpoint', returned.statement.end, [0, 0, 0]],
    ['grant dispatcher user source', fn.dispatcher.getStart(actual.source), [0, 0, 0]],
    ['grant helper definition user source', helper.name.getStart(actual.source), [0, 0, 0]],
  ];
  const controls = tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals), undefined, label);
    return { label, rejected: true };
  });
  function codeMutation(label, start, end, replacement) {
    assert.equal(replacement.length, end - start);
    const code = actual.code.slice(0, start) + replacement + actual.code.slice(end);
    const changed = stream(ts, code, actual.raw);
    assert.throws(() => compare(ts, native, changed, originals), undefined, label);
    controls.push({ label, rejected: true });
  }
  const operation = binary.node.operatorToken;
  codeMutation('replace arithmetic operation', operation.getStart(actual.source), operation.end, '-');
  const reference = group.uses[0], alternative = [...fn.slots].find(name => name !== reference.text
    && name.length === reference.text.length && trace.definitions.some(definition => definition.left.text === name));
  assert(alternative); codeMutation('read the wrong initialized slot', reference.getStart(actual.source), reference.end, alternative);
  const yieldEvent = trace.events.find(event => event.kind === 'yield');
  const clause = [...fn.states.values()].find(statements => statements.at(-1) === yieldEvent.statement);
  const target = clause[0].expression.right;
  codeMutation('skip the resume state', target.getStart(actual.source), target.end, '0'.padStart(target.getText().length, ' '));
  return controls;
}
module.exports = { run };
