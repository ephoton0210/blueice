// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Mutate real switch selection, fallthrough and source-anchored break roles.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs');
const { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');

function run(ts, native, actual, originals, proof) {
  assert.equal(proof.actualPaths.length, 3, 'both cases and default');
  const selected = proof.actualPaths.find(path => path.choices[0]);
  const fallback = proof.actualPaths.find(path => path.choices.every(choice => !choice));
  const cases = proof.actualPaths.find(path => !path.choices[0] && path.choices[1]).events.filter(event => event.kind === 'case');
  const stores = selected.events.filter(event => event.kind === 'store');
  const yields = selected.events.filter(event => event.kind === 'yield');
  const returned = selected.events.at(-1), jump = selected.events.find(event => event.kind === 'jump');
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === selected.machine.helper);
  const fallbackStore = fallback.events.filter(event => event.kind === 'store').at(-1);
  const start = node => node.getStart(actual.source);
  const tests = [
    ['erase original binding interior', start(stores[0].left) + 1, null],
    ['erase intact initializer statement end', stores[0].statement.end, null],
    ['erase carried user write', start(stores[1].right) + 1, null],
    ['retarget resumed write target', start(stores[1].left) + 1, [0, 0, 0]],
    ['erase compound write operation', start(stores[2].assignment.operatorToken) + 1, null],
    ['erase compound write operand', start(stores[2].right), null],
    ['erase compound statement end', stores[2].statement.end, null],
    ['erase fallback write target', start(fallbackStore.left) + 1, null],
    ['erase source break prefix', start(jump.statement), null],
    ['retarget source break interior', start(jump.statement) + 10, [0, 0, 0]],
    ['erase source break endpoint', jump.statement.end, null],
    ['erase returned user value interior', start(returned.operand) + 1, null],
    ['erase returned statement end', returned.statement.end, null],
    ['grant switch selector transport source', start(cases[0].statement), [0, 0, 0]],
    ['grant default transport source', start(cases[0].statement.caseBlock.clauses.at(-1)), [0, 0, 0]],
    ['grant helper definition source', start(helper.name), [0, 0, 0]],
    ['grant dispatcher source', start(selected.machine.dispatcher), [0, 0, 0]],
    ['grant map comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, [0, 0, 0]],
  ];
  for (const [index, event] of cases.entries()) {
    tests.push(['erase case ' + index + ' value', start(event.test.node), null]);
    tests.push(['erase case ' + index + ' colon', actual.tokens(event.clause).find(token => token.text === ':').start, null]);
    tests.push(['erase case ' + index + ' jump interior', start(event.opcode) + 1, null]);
    tests.push(['erase case ' + index + ' jump endpoint', event.opcode.end, null]);
  }
  for (const [index, event] of yields.entries()) {
    tests.push(['erase yield ' + index + ' operand', start(event.value.node), null]);
    tests.push(['erase yield ' + index + ' opcode', start(event.statement), null]);
  }
  for (const [index, value] of selected.values.filter(value => value.kind === 'sent').entries()) {
    tests.push(['erase resume ' + index + ' interior', start(value.node) + 1, null]);
  }
  const controls = tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals, { switch: true }), undefined, label);
    return { label, rejected: true };
  });
  function codeMutation(label, node, replacement) {
    const from = start(node), length = node.end - from;
    assert(replacement.length <= length, label + ' preserves physical offsets');
    replacement = replacement.padEnd(length, ' ');
    const changed = stream(ts, actual.code.slice(0, from) + replacement + actual.code.slice(node.end), actual.raw);
    assert.throws(() => compare(ts, native, changed, originals, { switch: true }), undefined, label);
    controls.push({ label, rejected: true });
  }
  codeMutation('replace first case with duplicate second', cases[0].test.node, '4');
  codeMutation('replace second case with duplicate first', cases[1].test.node, '2');
  codeMutation('jump from first case directly to return', cases[0].opcode.expression.right, '1');
  const defaultClause = cases[0].statement.caseBlock.clauses.at(-1);
  codeMutation('redirect default to first case', defaultClause.statements[0].expression.right, cases[0].opcode.expression.right.getText());
  codeMutation('change fallthrough compound operation', stores[2].assignment.operatorToken, '-=');
  codeMutation('change original initial value', stores[0].right, '9');
  codeMutation('change fallback value', fallbackStore.right, '8');
  codeMutation('change second yield value', yields[1].value.node, '4');
  codeMutation('write another binding after resume', stores[1].left, 'other');
  const properties = jump.statement.expression.properties;
  const destination = properties.find(property => property.name.getText() === 'label').initializer;
  codeMutation('break into the fallback body', destination, defaultClause.statements[0].expression.right.getText());
  const storeState = [...selected.machine.states.values()].find(statements => statements.includes(stores[1].statement));
  const transfer = storeState.find(statement => ts.isExpressionStatement(statement) && ts.isBinaryExpression(statement.expression)
    && statement.expression.left.getText() === selected.machine.context + '.label');
  assert(transfer);
  codeMutation('skip case fallthrough after resumed write', transfer.expression.right, '1');
  return controls;
}
module.exports = { run };
