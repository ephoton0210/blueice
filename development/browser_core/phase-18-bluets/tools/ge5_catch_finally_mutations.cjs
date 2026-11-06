// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Catch binding identity, effects and region edges cannot be replaced silently.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs');
const { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
function run(ts, native, actual, originals, proof) {
  assert.equal(proof.actualPaths.length, 2, 'case and default source paths');
  const trace = proof.actualPaths.find(path => path.choices[0]);
  const other = proof.actualPaths.find(path => !path.choices[0]);
  const start = node => node.getStart(actual.source);
  const thrown = trace.events.find(event => event.kind === 'throw');
  const returned = trace.events.find(event => event.kind === 'return');
  const alternate = other.events.find(event => event.kind === 'return');
  const yielded = trace.events.filter(event => event.kind === 'yield');
  const resumed = trace.events.filter(event => event.kind === 'resume');
  const effects = trace.events.filter(event => event.kind === 'effect');
  const caught = trace.values.find(value => value.kind === 'catch-read');
  const payload = trace.definitions.find(definition => definition.value.kind === 'exception');
  const selection = trace.events.find(event => event.kind === 'case');
  const region = trace.regions[0], properties = region.statement.expression.properties;
  const field = name => properties.find(property => property.name.getText() === name).initializer;
  const allVisited = new Set(proof.actualPaths.flatMap(path => [...path.visited]));
  const dead = [...trace.machine.states].filter(([state]) => !allVisited.has(state)).flatMap(([, statements]) => statements);
  assert.equal(dead.length, 1);
  const actions = [...trace.machine.states.values()].flat().filter(statement => ts.isReturnStatement(statement)
    && ts.isObjectLiteralExpression(statement.expression) && statement.expression.properties.length === 1);
  const leave = actions.find(statement => statement.getText().includes('kind:"leave"'));
  const finish = actions.find(statement => statement.getText().includes('kind:"finish"'));
  assert(leave && finish);
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === trace.machine.helper);
  const tests = [
    ['erase caught binding read interior', start(caught.node) + 1, null],
    ['erase caught binding endpoint', caught.node.end, null],
    ['grant catch transport source', start(payload.right), [0, 0, 0]],
    ['grant catch assignment source', start(payload.statement), [0, 0, 0]],
    ['erase source throw prefix', start(thrown.statement), null],
    ['erase source throw value', start(thrown.value.node), null],
    ['erase source throw end', thrown.statement.end, null],
    ['erase post-finally return value', start(returned.value.node), null],
    ['erase post-finally return end', returned.statement.end, null],
    ['erase pending default return value', start(alternate.value.node), null],
    ['erase pending default return end', alternate.statement.end, null],
    ['erase source case colon', actual.tokens(selection.clause).find(token => token.text === ':').start, null],
    ['erase source case jump interior', start(selection.opcode) + 1, null],
    ['erase source case jump end', selection.opcode.end, null],
    ['grant source selector transport', start(selection.statement), [0, 0, 0]],
    ['grant region registration source', start(region.statement), [0, 0, 0]],
    ['grant normal leave source', start(leave), [0, 0, 0]],
    ['grant end-finally source', start(finish), [0, 0, 0]],
    ['grant implicit scaffold source', start(dead[0]), [0, 0, 0]],
    ['grant helper definition source', start(helper.name), [0, 0, 0]],
    ['grant dispatcher source', start(trace.machine.dispatcher), [0, 0, 0]],
    ['grant map comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]],
  ];
  for (const [index, event] of effects.entries()) {
    tests.push(['erase effect ' + index + ' receiver interior', start(event.value.callee.object.node) + 1, null]);
    tests.push(['erase effect ' + index + ' method interior', start(event.value.callee.node.name) + 1, null]);
    tests.push(['erase effect ' + index + ' statement end', event.statement.end, null]);
  }
  for (const [index, event] of yielded.entries()) {
    tests.push(['erase yield ' + index + ' operand', start(event.value.node), null]);
    tests.push(['erase yield ' + index + ' opcode', start(event.statement), null]);
    tests.push(['erase yield ' + index + ' endpoint', event.root.end - 1, null]);
  }
  for (const [index, event] of resumed.entries()) tests.push(['erase resume ' + index + ' interior', start(event.value.node) + 1, null]);
  const options = { switch: true, exceptions: 'catch-finally' };
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
  codeMutation('skip the try body', field('entry'), String(region.after));
  codeMutation('route catch directly into finally', field('caught'), String(region.finalizer));
  codeMutation('substitute the finally target', field('finalizer'), String(region.after));
  codeMutation('substitute the after target', field('after'), '0');
  codeMutation('replace region entry with leave', field('kind'), '"leave"');
  codeMutation('read another user binding in catch', caught.node, 'log');
  codeMutation('read context label as the exception', payload.right.name, 'label');
  codeMutation('change caught exception conversion', effects[0].value.arguments[0].callee.node, 'Number');
  codeMutation('replace logging with removal', effects[0].value.callee.node.name, 'pop');
  codeMutation('change the finalizer log value', effects[1].value.arguments[0].node, '"missing"');
  codeMutation('change the thrown value', thrown.value.node, '9');
  codeMutation('change the final return', returned.value.node, '6');
  codeMutation('change the pending default return', alternate.value.node, '8');
  codeMutation('change the switch selector', selection.value.node, '0');
  codeMutation('change the case value', selection.test.node, '0');
  codeMutation('skip source throw and catch', selection.opcode.expression.right, String(region.after));
  for (const [index, event] of yielded.entries()) codeMutation('change yield ' + index, event.value.node, '8');
  codeMutation('re-enter instead of leaving catch', leave.expression.properties[0].initializer, '"enter"');
  codeMutation('leave instead of completing finally', finish.expression.properties[0].initializer, '"leave"');
  const extra = stream(ts, actual.code.slice(0, finish.end) + '0;' + actual.code.slice(finish.end), actual.raw);
  assert.throws(() => compare(ts, native, extra, originals, options), undefined, 'reject an extra unreachable finish statement');
  controls.push({ label: 'reject an extra unreachable finish statement', rejected: true });
  return controls;
}
module.exports = { run };
