// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Challenge source boundaries and the asynchronous receiver/continuation ABI.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs'), { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
function run(ts, native, actual, originals, proof, form) {
  const trace = proof.actualPaths[0], fn = trace.machine, returned = trace.events.at(-1);
  const sent = trace.values.find(value => value.kind === 'sent'), method = trace.values.find(value => value.kind === 'property' && value.name === 'resolve');
  const yielded = trace.events.find(event => event.kind === 'yield'); assert(sent && method && yielded);
  const header = actual.tokens(fn.node).filter(token => token.end <= fn.node.body.getStart(actual.source) + 1);
  const moved = header.find(token => token.text === '('), synthetic = actual.tokens(fn.outer.node).find(token => token.text === '(');
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === fn.outer.helper); assert(helper);
  const tests = [
    ['erase moved async signature', moved.start, null],
    ['grant generated argument slots source', synthetic.start, [0, 0, 0]],
    ['grant awaiter definition source', helper.name.getStart(actual.source), [0, 0, 0]],
    ['grant dispatcher source', fn.dispatcher.getStart(actual.source), [0, 0, 0]],
    ['erase await yield opcode', yielded.statement.getStart(actual.source), null],
    ['erase resumed context interior', sent.node.getStart(actual.source) + 1, null],
    ['retarget resumed expression endpoint', sent.node.end, [0, 0, 0]],
    ['erase captured method interior', method.node.name.getStart(actual.source) + 1, null],
    ['erase return opcode endpoint', returned.root.end - 1, null],
  ];
  const controls = [];
  function mapMutation([label, offset, owner]) {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals, { asynchronous: true }), undefined, label);
    controls.push({ label, rejected: true });
  }
  tests.forEach(mapMutation);
  function codeMutation(label, start, end, replacement) {
    assert.equal(replacement.length, end - start);
    const changed = stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw);
    assert.throws(() => compare(ts, native, changed, originals, { asynchronous: true }), undefined, label);
    controls.push({ label, rejected: true });
  }
  codeMutation('replace the original method', method.node.name.getStart(actual.source), method.node.name.end, 'rejectx');
  const state = [...fn.states.values()].find(statements => statements.at(-1) === yielded.statement), target = state[0].expression.right;
  codeMutation('skip an async resume state', target.getStart(actual.source), target.end, '0'.padStart(target.getText().length, ' '));
  if (form === 'helper-name-collisions') {
    const operation = returned.value.node.operatorToken; assert.equal(operation.getText(), '+');
    codeMutation('replace the return operation', operation.getStart(actual.source), operation.end, '-');
  } else if (form === 'nested-await-call') {
    const call = trace.values.find(value => value.kind === 'call' && value.dispatch); assert(call);
    const receiver = call.node.arguments[1], callee = call.node.arguments[0];
    codeMutation('invoke on the wrong captured receiver', receiver.getStart(actual.source), receiver.end, callee.getText());
    mapMutation(['erase captured-call close', call.node.end - 1, null]);
    mapMutation(['grant intrinsic transport source', call.node.getStart(actual.source), [0, 0, 0]]);
  } else {
    assert(['conditional-expression-await', 'conditional-await'].includes(form)); assert.equal(proof.actualPaths.length, 2);
    const branch = trace.events.find(event => event.kind === 'branch'), node = branch.statement.expression.right.condition;
    const position = node.getStart(actual.source) - 1; assert.equal(actual.code[position], ' ');
    codeMutation('invert the async branch', position, position + 1, '!');
    assert(ts.isIdentifier(returned.operand)); mapMutation(['erase carried return value', returned.operand.getStart(actual.source) + 1, null]);
    if (form === 'conditional-await') {
      const store = trace.events.find(event => event.kind === 'store' && event.value.kind === 'binary'); assert(store);
      codeMutation('write to the wrong user binding', store.left.getStart(actual.source), store.left.end, 'other');
      const operation = store.value.node.operatorToken; assert.equal(operation.getText(), '+');
      codeMutation('replace the compound write operation', operation.getStart(actual.source), operation.end, '-');
      mapMutation(['erase user write interior', store.left.getStart(actual.source) + 1, null]);
      mapMutation(['erase compound write terminator', store.statement.end - 1, null]);
      mapMutation(['erase compound write endpoint', store.statement.end, null]);
    }
  }
  return controls;
}
module.exports = { run };
