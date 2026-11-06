// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Challenge every template boundary and the captured concat/resume data flow.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs'), { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
function run(ts, native, actual, originals, proof, asynchronous) {
  assert.equal(proof.actualPaths.length, 1);
  const trace = proof.actualPaths[0], fn = trace.machine, returned = trace.events.at(-1);
  const calls = trace.values.filter(value => value.kind === 'call' && value.dispatch);
  const methods = trace.values.filter(value => value.kind === 'property' && value.name === 'concat');
  const literals = trace.values.filter(value => value.kind === 'literal');
  const base = literals.find(value => value.text === '"a"'), middle = literals.find(value => value.text === '"b"');
  const sent = trace.values.find(value => value.kind === 'sent'), yielded = trace.events.find(event => event.kind === 'yield');
  const groups = trace.generatedGroups.filter(group => group.template);
  assert.equal(calls.length, 2); assert.equal(methods.length, 2); assert.equal(groups.length, 1);
  assert(base && middle && sent && yielded);
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === fn.helper); assert(helper);
  const controls = [];
  function mapping(label, offset, owner) {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals, { asynchronous }), undefined, label);
    controls.push({ label, rejected: true });
  }
  function code(label, start, end, replacement) {
    assert.equal(replacement.length, end - start);
    const changed = stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw);
    assert.throws(() => compare(ts, native, changed, originals, { asynchronous }), undefined, label);
    controls.push({ label, rejected: true });
  }
  mapping('grant the generator helper source', helper.name.getStart(actual.source), [0, 0, 0]);
  mapping('grant the continuation dispatcher source', fn.dispatcher.getStart(actual.source), [0, 0, 0]);
  mapping('erase the first suspension opcode', yielded.statement.getStart(actual.source), null);
  mapping('erase the first suspension endpoint', yielded.root.end - 1, null);
  mapping('erase a resumed context interior', sent.node.getStart(actual.source) + 1, null);
  mapping('retarget a resumed value endpoint', sent.node.end, [0, 0, 0]);
  mapping('grant the generated initial literal source', base.node.getStart(actual.source) + 1, [0, 0, 0]);
  mapping('erase the middle literal interior', middle.node.getStart(actual.source) + 1, null);
  mapping('erase the carried concat property interior', methods[1].node.name.getStart(actual.source) + 1, null);
  mapping('grant captured intrinsic transport source', calls[0].node.getStart(actual.source), [0, 0, 0]);
  mapping('erase the logical captured-call close', calls[0].node.end - 1, null);
  mapping('erase the logical template opening', groups[0].node.getStart(actual.source), null);
  mapping('erase the logical template closing', groups[0].node.end - 1, null);
  mapping('erase the final return value interior', returned.operand.getStart(actual.source) + 1, null);
  code('change the captured concat method', methods[0].node.name.getStart(actual.source), methods[0].node.name.end, 'reject');
  code('change the middle template text', middle.node.getStart(actual.source), middle.node.end, '"c"');
  const state = [...fn.states.values()].find(statements => statements.at(-1) === yielded.statement), target = state[0].expression.right;
  code('skip the first template resume', target.getStart(actual.source), target.end, '0'.padStart(target.getText().length, ' '));
  const receiver = calls[0].node.arguments[1], wrong = receiver.getText().replace(/[0-9]$/, digit => digit === '0' ? '1' : '0');
  assert(fn.slots.has(wrong) && wrong !== receiver.getText());
  code('use a different captured concat receiver', receiver.getStart(actual.source), receiver.end, wrong);
  if (asynchronous) {
    const header = actual.tokens(fn.node).filter(token => token.end <= fn.node.body.getStart(actual.source) + 1);
    const moved = header.find(token => token.text === '('), outer = actual.tokens(fn.outer.node).find(token => token.text === '(');
    const awaiter = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === fn.outer.helper); assert(awaiter);
    mapping('erase the moved async signature', moved.start, null);
    mapping('grant generated outer parameter slots source', outer.start, [0, 0, 0]);
    mapping('grant the generated awaiter source', awaiter.name.getStart(actual.source), [0, 0, 0]);
  } else {
    const resumed = trace.events.find(event => event.kind === 'resume'); assert(resumed);
    mapping('erase the discarded template resume endpoint', resumed.value.node.end, null);
    code('replace the discarded resume operation', resumed.value.node.expression.name.getStart(actual.source), resumed.value.node.expression.name.end, 'nope');
  }
  return controls;
}
module.exports = { run };
