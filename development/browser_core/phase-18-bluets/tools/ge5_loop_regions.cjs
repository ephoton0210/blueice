// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Non-nested loop regions retain normal, continue and break destinations.
const assert = require('node:assert/strict');
function regions(ts, machine) {
  const registrations = [];
  let active = null;
  const number = node => {
    assert(ts.isNumericLiteral(node)); const result = Number(node.text);
    assert(Number.isSafeInteger(result) && machine.states.has(result)); return result;
  };
  function enter(statement, state) {
    let frame;
    if (machine.blue) {
      if (!ts.isReturnStatement(statement) || !ts.isObjectLiteralExpression(statement.expression)) return null;
      const properties = statement.expression.properties;
      if (!properties.some(property => ts.isPropertyAssignment(property) && property.name.getText() === 'kind'
        && property.initializer.getText() === '"enter"')) return null;
      assert.deepEqual(properties.map(property => property.name.getText()), ['kind', 'entry', 'caught', 'finalizer', 'after']);
      assert(properties.every(ts.isPropertyAssignment)); assert.equal(properties[2].initializer.getText(), '-1');
      frame = { statement, entry: number(properties[1].initializer), finalizer: number(properties[3].initializer), after: number(properties[4].initializer) };
    } else {
      if (!ts.isExpressionStatement(statement) || !ts.isCallExpression(statement.expression)
        || statement.expression.expression.getText() !== machine.context + '.trys.push') return null;
      assert.equal(statement.expression.arguments.length, 1); const array = statement.expression.arguments[0];
      assert(ts.isArrayLiteralExpression(array) && array.elements.length === 4);
      assert.equal(number(array.elements[0]), state); assert(ts.isOmittedExpression(array.elements[1]));
      frame = { statement, entry: state, finalizer: number(array.elements[2]), after: number(array.elements[3]) };
    }
    assert.equal(active, null); assert(registrations.length < 16, 'bounded region visits');
    assert.notEqual(frame.finalizer, frame.after); assert.notEqual(frame.entry, frame.finalizer);
    if (registrations.length) {
      const first = registrations[0];
      assert.equal(frame.statement, first.statement, 'one static region across loop visits');
      assert.deepEqual([frame.entry, frame.finalizer, frame.after], [first.entry, first.finalizer, first.after]);
    }
    registrations.push(frame); active = { ...frame, phase: 'body', pending: null };
    return { next: machine.blue ? frame.entry : null };
  }
  function jump(target, depth) {
    assert(machine.states.has(target)); assert.equal(depth, 0);
    assert.equal(active?.phase, 'body');
    active.phase = 'finally'; active.pending = { kind: 'jump', target };
    return { next: active.finalizer };
  }
  function leave() {
    assert.equal(active?.phase, 'body');
    active.phase = 'finally'; active.pending = { kind: 'normal', target: active.after };
    return { next: active.finalizer };
  }
  function finish() {
    assert.equal(active?.phase, 'finally'); assert(active.pending);
    const next = active.pending.target; assert(machine.states.has(next)); active = null; return { next };
  }
  function generated(statement) {
    if (!ts.isReturnStatement(statement)) return null;
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
      && number(elements[1]) === active.after && machine.view.owner(statement.getStart(machine.view.source)) === null) return leave();
    return null;
  }
  function complete(kind, value) {
    assert.equal(kind, 'return'); assert.equal(active, null); return { done: true, value };
  }
  function audit(visited) {
    assert.equal(active, null); assert(registrations.length > 0);
    const dead = [...machine.states].filter(([state]) => !visited.has(state));
    assert.equal(dead.length, machine.blue ? 1 : 0);
    for (const [, statements] of dead) {
      assert.equal(statements.length, 1); const statement = statements[0];
      assert.equal(statement.getText().replace(/\s/g, ''), 'return{kind:"return",value:void0};');
      machine.view.noSource(statement.getStart(machine.view.source), statement.end, 'unreachable implicit loop completion');
    }
  }
  return { enter, generated, complete, audit, jump, registrations, phase: () => active?.phase ?? 'outside' };
}
module.exports = { regions };
