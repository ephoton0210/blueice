// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every loop edge and capture remains checked even when an exit is not selected.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs');
const { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
function run(ts, native, actual, originals, proof) {
  const trace = proof.actual, start = node => node.getStart(actual.source);
  const unique = events => [...new Map(events.map(event => [event.statement, event])).values()];
  const yielded = unique(trace.events.filter(event => event.kind === 'yield'));
  const resumed = unique(trace.events.filter(event => event.kind === 'resume'));
  const branches = unique(trace.events.filter(event => event.kind === 'branch'));
  const jumped = unique(trace.events.filter(event => event.kind === 'jump'));
  const updated = unique(trace.events.filter(event => event.kind === 'update'))[0];
  const stored = trace.events.find(event => event.kind === 'store'), returned = trace.events.at(-1);
  assert.equal(trace.regions.length, 3); assert.equal(yielded.length, 2); assert.equal(jumped.length, 2);
  const guard = branches.find(event => event.value.operator === '<');
  const conditions = branches.filter(event => event.value.operator === '===');
  const predicates = conditions.map(event => trace.definitions.find(definition => definition.value === event.value));
  const exit = guard.statement.expression.right.whenFalse;
  const region = trace.regions[0], field = name => region.statement.expression.properties.find(property => property.name.getText() === name).initializer;
  const actions = [...trace.machine.states.values()].flat().filter(statement => ts.isReturnStatement(statement)
    && ts.isObjectLiteralExpression(statement.expression) && statement.expression.properties.length === 1);
  const leave = actions.find(statement => statement.getText().includes('kind:"leave"'));
  const finish = actions.find(statement => statement.getText().includes('kind:"finish"'));
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === trace.machine.helper);
  const tests = [
    ['erase for initializer binding', start(stored.left) + 1, null],
    ['erase for initializer endpoint', stored.statement.end, null],
    ['erase loop exit source', start(exit), null],
    ['retarget loop exit source', start(exit), [0, 0, 0]],
    ['erase update binding interior', start(updated.value.node) + 1, null],
    ['erase update operator', updated.value.node.end, null],
    ['erase update statement endpoint', updated.statement.end, null],
    ['erase final return value interior', start(returned.value.node) + 1, null],
    ['erase final return endpoint', returned.statement.end, null],
    ['grant region registration source', start(region.statement), [0, 0, 0]],
    ['grant normal leave source', start(leave), [0, 0, 0]],
    ['grant end-finally source', start(finish), [0, 0, 0]],
    ['grant helper definition source', start(helper.name), [0, 0, 0]],
    ['grant dispatcher source', start(trace.machine.dispatcher), [0, 0, 0]],
    ['grant map comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]],
  ];
  for (const [index, definition] of predicates.entries()) {
    tests.push(['erase source if ' + index + ' prefix', start(definition.statement), null]);
    tests.push(['erase source if ' + index + ' close', definition.statement.end - 1, null]);
    tests.push(['erase source if ' + index + ' endpoint', definition.statement.end, null]);
  }
  for (const [index, event] of jumped.entries()) {
    tests.push(['erase source jump ' + index + ' interior', start(event.statement) + 10, null]);
    tests.push(['erase source jump ' + index + ' endpoint', event.statement.end, null]);
  }
  for (const [index, event] of yielded.entries()) {
    tests.push(['erase yield ' + index + ' source value', start(event.value.node) + 1, null]);
    tests.push(['erase yield ' + index + ' opcode', start(event.statement), null]);
    tests.push(['erase yield ' + index + ' endpoint', event.root.end - 1, null]);
  }
  for (const [index, event] of resumed.entries()) tests.push(['erase resume ' + index + ' interior', start(event.value.node) + 1, null]);
  const options = { loop: true, exceptions: 'loop-finally' };
  const controls = tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals, options), undefined, label);
    return { label, rejected: true };
  });
  function codeMutation(label, node, replacement) {
    const from = start(node), length = node.end - from;
    assert(replacement.length <= length, label); replacement = replacement.padEnd(length, ' ');
    const changed = stream(ts, actual.code.slice(0, from) + replacement + actual.code.slice(node.end), actual.raw);
    assert.throws(() => compare(ts, native, changed, originals, options), undefined, label);
    controls.push({ label, rejected: true });
  }
  codeMutation('change the original initializer', stored.right, '1');
  codeMutation('retarget the unselected guard exit to implicit completion', exit, '0');
  codeMutation('change the loop bound', guard.value.right.node, '9');
  codeMutation('decrement instead of incrementing', updated.statement.expression, updated.value.node.getText() + '--');
  codeMutation('change the final result', returned.value.node, '98');
  codeMutation('skip the region body', field('entry'), String(region.after));
  codeMutation('skip the finalizer', field('finalizer'), String(region.after));
  codeMutation('leave the region into the wrong state', field('after'), '0');
  codeMutation('invent a catch target', field('caught'), '0');
  for (const [index, event] of conditions.entries()) codeMutation('change abrupt condition ' + index, event.value.right.node, '9');
  for (const [index, event] of jumped.entries()) {
    const properties = event.statement.expression.properties;
    const target = properties.find(property => property.name.getText() === 'label').initializer;
    const depth = properties.find(property => property.name.getText() === 'depth').initializer;
    codeMutation('misroute source jump ' + index, target, '0');
    codeMutation('retain the wrong region depth ' + index, depth, '1');
  }
  const offset = yielded.find(event => event.value.kind === 'binary').value.right.node;
  codeMutation('change the finally yield offset', offset, '11');
  codeMutation('re-enter instead of leaving the loop region', leave.expression.properties[0].initializer, '"enter"');
  codeMutation('leave instead of finishing the loop region', finish.expression.properties[0].initializer, '"leave"');
  const extra = stream(ts, actual.code.slice(0, finish.end) + '0;' + actual.code.slice(finish.end), actual.raw);
  assert.throws(() => compare(ts, native, extra, originals, options), undefined, 'reject unreachable code after finish');
  controls.push({ label: 'reject unreachable code after finish', rejected: true });
  return controls;
}
module.exports = { run };
