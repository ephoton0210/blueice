// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Decode exception-region descriptors independently for the two iterator ABIs.
const assert = require('node:assert/strict');

function regions(ts, machine) {
  const registrations = [];
  let active = null, exception = null;
  const caughtProfile = machine.exceptions === 'catch-finally';
  const delegatedProfile = machine.exceptions === 'delegate-finally';
  const normalProfile = machine.exceptions === 'normal-finally';
  const pendingReturnProfile = machine.exceptions === 'pending-return-finally';
  const switchDelegateProfile = machine.exceptions === 'switch-delegate-finally';
  if (switchDelegateProfile) require('./ge5_generator_switch_machine.cjs').checked(machine);
  const finishProfile = caughtProfile || delegatedProfile || normalProfile || pendingReturnProfile || switchDelegateProfile;
  const number = node => {
    assert(ts.isNumericLiteral(node));
    const result = Number(node.text);
    assert(Number.isSafeInteger(result) && machine.states.has(result), 'existing region continuation');
    return result;
  };
  function enter(statement, state) {
    let frame;
    if (machine.blue) {
      if (!ts.isReturnStatement(statement) || !ts.isObjectLiteralExpression(statement.expression)) return null;
      const properties = statement.expression.properties;
      if (!properties.some(property => ts.isPropertyAssignment(property) && property.name.getText() === 'kind'
        && property.initializer.getText() === '"enter"')) return null;
      assert.deepEqual(properties.map(property => property.name.getText()), ['kind', 'entry', 'caught', 'finalizer', 'after']);
      assert(properties.every(ts.isPropertyAssignment));
      const caught = caughtProfile ? number(properties[2].initializer) : null;
      if (!caughtProfile) assert.equal(properties[2].initializer.getText(), '-1', 'no catch in the finally-return profile');
      frame = { statement, caught, entry: number(properties[1].initializer), finalizer: number(properties[3].initializer), after: number(properties[4].initializer) };
    } else {
      if (!ts.isExpressionStatement(statement) || !ts.isCallExpression(statement.expression)
        || statement.expression.expression.getText() !== machine.context + '.trys.push') return null;
      const call = statement.expression; assert.equal(call.arguments.length, 1);
      const array = call.arguments[0]; assert(ts.isArrayLiteralExpression(array) && array.elements.length === 4);
      assert.equal(number(array.elements[0]), state, 'region begins at its registered state');
      const caught = caughtProfile ? number(array.elements[1]) : null;
      if (!caughtProfile) assert(ts.isOmittedExpression(array.elements[1]), 'no catch in the finally-return profile');
      frame = { statement, caught, entry: state, finalizer: number(array.elements[2]), after: number(array.elements[3]) };
    }
    assert.equal(active, null, 'one reviewed non-nested exception region');
    assert(!registrations.length, 'one complete exception-region inventory');
    assert.notEqual(frame.entry, frame.finalizer); assert.notEqual(frame.finalizer, frame.after);
    const after = machine.states.get(frame.after);
    assert.equal(after.length, 1); assert(ts.isReturnStatement(after[0]));
    if (!caughtProfile) assert.equal(after[0].getText().replace(/\s/g, ''), machine.blue
      ? 'return{kind:"return",value:void0};' : 'return[2/*return*/];', 'region after-target is the implicit completion');
    active = { ...frame, phase: 'body', pending: null };
    registrations.push(frame);
    return { next: machine.blue ? frame.entry : null };
  }
  function complete(kind, value) {
    assert(kind === 'return' || caughtProfile && kind === 'throw', 'reviewed region completion');
    if (!active) { assert(caughtProfile && kind === 'return'); return { done: true, value }; }
    if (kind === 'throw' && active.phase === 'body' && active.caught !== null) {
      active.phase = 'catch'; exception = value; return { next: active.caught };
    }
    if (active.phase !== 'finally') {
      active.phase = 'finally'; active.pending = { kind, value };
      return { next: active.finalizer };
    }
    assert.equal(active.phase, 'finally'); assert(active.pending);
    active = null;
    return { done: true, value };
  }
  function leave() {
    assert(finishProfile && active && active.phase !== 'finally');
    active.phase = 'finally'; active.pending = { kind: 'normal' };
    return { next: active.finalizer };
  }
  function finish() {
    assert(finishProfile && active?.phase === 'finally' && active.pending);
    const { pending, after } = active; active = null;
    if (pending.kind === 'normal') return { next: after };
    assert.equal(pending.kind, 'return'); return { done: true, value: pending.value };
  }
  function generated(statement) {
    if (!finishProfile || !ts.isReturnStatement(statement)) return null;
    if (normalProfile && active === null && statement.getText().replace(/\s/g, '') === (machine.blue
      ? 'return{kind:"return",value:void0};' : 'return[2/*return*/];')) return { done: true, value: null };
    if (machine.blue) {
      if (!ts.isObjectLiteralExpression(statement.expression)) return null;
      const properties = statement.expression.properties;
      if (properties.length !== 1 || !ts.isPropertyAssignment(properties[0]) || properties[0].name.getText() !== 'kind') return null;
      const kind = properties[0].initializer.getText();
      if (kind === '"leave"') return leave();
      if (kind === '"finish"') return finish();
      return null;
    }
    if (!ts.isArrayLiteralExpression(statement.expression)) return null;
    const elements = statement.expression.elements;
    if (elements.length === 1 && elements[0].getText() === '7') return finish();
    if (elements.length === 2 && elements[0].getText() === '3' && active
      && number(elements[1]) === active.after) return leave();
    return null;
  }
  function dead(statements) {
    assert.equal(statements.length, 1);
    const statement = statements[0]; assert(ts.isReturnStatement(statement));
    const text = statement.getText().replace(/\s/g, '');
    let kind;
    if (machine.blue) kind = ({ 'return{kind:"leave"};': 'leave', 'return{kind:"finish"};': 'finish',
      'return{kind:"return",value:void0};': 'after' })[text];
    else if (text === 'return[2/*return*/];') kind = 'after';
    else if (caughtProfile && text === 'return[3/*break*/,' + registrations[0].after + '];') kind = 'exit';
    assert(kind, 'no unreviewed unreachable exception state');
    machine.view.noSource(statement.getStart(machine.view.source), statement.end, 'unreachable exception scaffold');
    return kind;
  }
  function audit(visited) {
    assert.equal(registrations.length, 1); assert.equal(active, null);
    const counts = { leave: 0, finish: 0, after: 0, exit: 0 };
    for (const [state, statements] of machine.states) if (!visited.has(state)) {
      const kind = dead(statements); counts[kind]++;
      if (kind === 'after' && !caughtProfile) assert.equal(state, registrations[0].after);
    }
    assert.deepEqual(counts, normalProfile ? { leave: 0, finish: 0, after: 0, exit: 0 } : delegatedProfile || pendingReturnProfile || switchDelegateProfile
      ? machine.blue ? { leave: 1, finish: 0, after: 1, exit: 0 } : { leave: 0, finish: 0, after: 1, exit: 0 }
      : caughtProfile
      ? machine.blue ? { leave: 0, finish: 0, after: 1, exit: 0 } : { leave: 0, finish: 0, after: 0, exit: 1 }
      : machine.blue ? { leave: 1, finish: 1, after: 1, exit: 0 } : { leave: 0, finish: 0, after: 1, exit: 0 });
  }
  return { enter, complete, generated, audit, registrations, phase: () => active?.phase ?? 'outside',
    exception: () => { assert.equal(active?.phase, 'catch'); assert(exception); return exception; } };
}
module.exports = { regions };
