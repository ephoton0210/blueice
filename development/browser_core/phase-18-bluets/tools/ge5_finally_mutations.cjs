// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Keep pending returns, finalizer overrides and every exception edge observable.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs');
const { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');

function run(ts, native, actual, originals, proof) {
  assert.equal(proof.actualPaths.length, 1);
  const trace = proof.actual, start = node => node.getStart(actual.source);
  const returned = trace.events.filter(event => event.kind === 'return');
  const yielded = trace.events.filter(event => event.kind === 'yield');
  const resumed = trace.events.filter(event => event.kind === 'resume');
  assert.equal(returned.length, 2); assert.equal(yielded.length, 2); assert.equal(resumed.length, 2);
  assert.deepEqual(returned.map(event => event.value.phase), ['body', 'finally']);
  const region = trace.regions[0], properties = region.statement.expression.properties;
  const field = name => properties.find(property => property.name.getText() === name).initializer;
  const dead = [...trace.machine.states].filter(([state]) => !trace.visited.has(state)).map(([, statements]) => statements);
  const returnState = [...trace.machine.states].find(([, statements]) => statements.includes(returned[0].statement))[0];
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === trace.machine.helper);
  const tests = [
    ['retarget function identity interior', start(trace.machine.node.name) + 1, [0, 0, 0]],
    ['grant region registration source', start(region.statement), [0, 0, 0]],
    ['grant helper definition source', start(helper.name) + 1, [0, 0, 0]],
    ['grant dispatcher source', start(trace.machine.dispatcher), [0, 0, 0]],
    ['grant generated declaration source', start(trace.machine.node.body.statements[0]), [0, 0, 0]],
    ['grant map comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]],
  ];
  for (const [index, event] of returned.entries()) {
    tests.push(['erase return ' + index + ' value', start(event.value.node), null]);
    tests.push(['erase return ' + index + ' opcode', start(event.statement), null]);
    tests.push(['erase return ' + index + ' expression end', event.root.end - 1, null]);
    tests.push(['erase return ' + index + ' statement end', event.statement.end, null]);
  }
  for (const [index, event] of yielded.entries()) {
    tests.push(['erase yield ' + index + ' operand', start(event.value.node), null]);
    tests.push(['erase yield ' + index + ' opcode interior', start(event.statement) + 10, null]);
    tests.push(['erase yield ' + index + ' endpoint', event.root.end - 1, null]);
  }
  for (const [index, event] of resumed.entries()) {
    tests.push(['erase resume ' + index + ' interior', start(event.value.node) + 1, null]);
    tests.push(['erase resume ' + index + ' terminator', event.statement.end - 1, null]);
  }
  for (const [index, statements] of dead.entries()) {
    tests.push(['grant dead exception scaffold ' + index + ' source', start(statements[0]), [0, 0, 0]]);
  }
  const controls = tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals, { exceptions: true }), undefined, label);
    return { label, rejected: true };
  });
  function codeMutation(label, node, replacement) {
    const from = start(node), length = node.end - from;
    assert(replacement.length <= length); replacement = replacement.padEnd(length, ' ');
    const changed = stream(ts, actual.code.slice(0, from) + replacement + actual.code.slice(node.end), actual.raw);
    assert.throws(() => compare(ts, native, changed, originals, { exceptions: true }), undefined, label);
    controls.push({ label, rejected: true });
  }
  codeMutation('skip the try body', field('entry'), String(region.after));
  codeMutation('re-enter the body completion instead of finally', field('finalizer'), String(returnState));
  codeMutation('bypass finally using the after target', field('finalizer'), String(region.after));
  codeMutation('substitute the region after target', field('after'), String(region.finalizer));
  codeMutation('invent a catch target', field('caught'), '0');
  codeMutation('replace region entry with a leave', field('kind'), '"leave"');
  codeMutation('change the masked pending return', returned[0].value.node, '9');
  codeMutation('change the finalizer override', returned[1].value.node, '8');
  codeMutation('change the try yield', yielded[0].value.node, '7');
  codeMutation('change the finally yield', yielded[1].value.node, '8');
  const finalKind = returned[1].statement.expression.properties.find(property => property.name.getText() === 'kind').initializer;
  codeMutation('yield instead of completing the finalizer', finalKind, '"yield"');
  const sentName = resumed[0].value.node.expression.name;
  codeMutation('invoke another resume operation', sentName, 'next');
  const yieldState = [...trace.machine.states.values()].find(statements => statements.includes(yielded[0].statement));
  const transfer = yieldState.find(statement => ts.isExpressionStatement(statement) && ts.isBinaryExpression(statement.expression)
    && statement.expression.left.getText() === trace.machine.context + '.label');
  codeMutation('skip the discarded try resume', transfer.expression.right, String(returnState));
  const finish = dead.flat().find(statement => statement.getText().includes('kind:"finish"'));
  assert(finish);
  codeMutation('duplicate leave and erase finish scaffold', finish.expression.properties[0].initializer, '"leave"');
  const extra = stream(ts, actual.code.slice(0, region.statement.end) + '0;' + actual.code.slice(region.statement.end), actual.raw);
  assert.throws(() => compare(ts, native, extra, originals, { exceptions: true }), undefined, 'reject an extra unreachable region statement');
  controls.push({ label: 'reject an extra unreachable region statement', rejected: true });
  return controls;
}
module.exports = { run };
