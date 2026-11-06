// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs'), { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
function run(ts, native, actual, originals, proof) {
  assert.equal(proof.actualPaths.length, 4, 'both complete two-branch paths');
  const path = proof.actualPaths.find(path => path.choices.every(Boolean)), stores = path.events.filter(event => event.kind === 'store');
  const branch = path.events.find(event => event.kind === 'branch'), returned = path.events.at(-1);
  const skipped = proof.actualPaths.find(path => !path.choices[0]).events.find(event => event.kind === 'store');
  assert(ts.isIdentifier(skipped.right));
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === path.machine.helper);
  const tests = [
    ['retarget user write interior', stores[0].left.getStart(actual.source) + 1, [0, 0, 0]],
    ['erase initializer endpoint', stores[0].statement.end - 1, null],
    ['erase logical grouping close', stores[0].value.node.end - 1, null],
    ['erase carried initializer value', skipped.right.getStart(actual.source) + 1, null],
    ['erase returned statement endpoint', returned.root.end - 1, null],
    ['grant branch predicate source', branch.statement.getStart(actual.source), [0, 0, 0]],
    ['grant dispatcher source', path.machine.dispatcher.getStart(actual.source), [0, 0, 0]],
    ['grant helper definition source', helper.name.getStart(actual.source), [0, 0, 0]],
  ];
  const controls = tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => compare(ts, native, changed, originals), undefined, label); return { label, rejected: true };
  });
  function codeMutation(label, start, end, replacement) {
    assert.equal(replacement.length, end - start);
    const changed = stream(ts, actual.code.slice(0, start) + replacement + actual.code.slice(end), actual.raw);
    assert.throws(() => compare(ts, native, changed, originals), undefined, label);
    controls.push({ label, rejected: true });
  }
  const condition = branch.statement.expression.right.condition, before = condition.getStart(actual.source) - 1;
  assert.equal(actual.code[before], ' '); codeMutation('invert a logical branch', before, before + 1, '!');
  assert.equal(stores[1].left.text, 'second');
  codeMutation('write the wrong user binding', stores[1].left.getStart(actual.source), stores[1].left.end, 'first ');
  const event = path.events.find(event => event.kind === 'yield');
  const state = [...path.machine.states.values()].find(statements => statements.at(-1) === event.statement);
  const target = state[0].expression.right;
  codeMutation('skip a reachable resume state', target.getStart(actual.source), target.end, '0'.padStart(target.getText().length, ' '));
  return controls;
}
module.exports = { run };
