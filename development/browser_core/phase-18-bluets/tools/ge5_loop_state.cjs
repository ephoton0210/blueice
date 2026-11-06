// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original lexical identity and bounded scalar state drive the frozen loop trace.
const assert = require('node:assert/strict');
function bind(ts, machine) {
  assert.equal(machine.originals.length, 1); assert.equal(machine.view.decoded.metadata.sources.length, 1);
  const source = ts.createSourceFile('original.ts', machine.originals[0], ts.ScriptTarget.Latest, true);
  const declarations = [];
  function visit(node) {
    if (ts.isForStatement(node)) {
      assert(node.initializer && ts.isVariableDeclarationList(node.initializer));
      assert.equal(node.initializer.declarations.length, 1);
      const declaration = node.initializer.declarations[0]; assert(ts.isIdentifier(declaration.name));
      declarations.push(declaration);
    }
    ts.forEachChild(node, visit);
  }
  visit(source); assert.equal(declarations.length, 1);
  const original = declarations[0], position = source.getLineAndCharacterOfPosition(original.name.getStart(source));
  const owner = [0, position.line, position.character], candidates = [];
  for (const [state, statements] of machine.states) for (const statement of statements) {
    if (!ts.isExpressionStatement(statement) || !ts.isBinaryExpression(statement.expression)) continue;
    const assignment = statement.expression;
    if (assignment.operatorToken.kind !== ts.SyntaxKind.EqualsToken || !ts.isIdentifier(assignment.left)) continue;
    if (JSON.stringify(machine.view.owner(assignment.left.getStart(machine.view.source))) === JSON.stringify(owner)) {
      candidates.push({ alias: assignment.left.text, state, statement });
    }
  }
  assert.equal(candidates.length, 1, 'one original loop binding initializer');
  const binding = { ...candidates[0], name: original.name.text };
  assert(machine.slots.has(binding.alias)); machine.userSlots.add(binding.alias);
  machine.loopBinding = binding;
}
function state(machine) {
  let current;
  function constant(value) {
    if (value.kind === 'literal') {
      if (/^\d+$/.test(value.text)) return Number(value.text);
      if (value.text === 'true') return true;
      if (value.text === 'false') return false;
    }
    if (value.kind === 'user') return value.known;
    if (value.kind === 'group') return constant(value.inner);
    if (value.kind === 'binary') {
      const left = constant(value.left), right = constant(value.right);
      assert.equal(typeof left, 'number'); assert.equal(typeof right, 'number');
      if (value.operator === '+') return left + right;
      if (value.operator === '-') return left - right;
      if (value.operator === '<') return left < right;
      if (value.operator === '===') return left === right;
    }
    assert.fail('unreviewed scalar loop expression');
  }
  function store(name, value) {
    assert.equal(name, machine.loopBinding.alias); current = constant(value);
    assert(Number.isSafeInteger(current) && Math.abs(current) <= 1_000_000);
  }
  function snapshot(name) {
    if (name !== machine.loopBinding.alias) return undefined;
    assert(Number.isSafeInteger(current), 'loop binding is initialized before a read'); return current;
  }
  function update(name, operator) {
    assert.equal(name, machine.loopBinding.alias); assert(['++', '--'].includes(operator));
    const before = snapshot(name); current += operator === '++' ? 1 : -1;
    assert(Number.isSafeInteger(current) && Math.abs(current) <= 1_000_000); return { before, after: current };
  }
  function condition(value) {
    const result = constant(value); assert.equal(typeof result, 'boolean'); return result;
  }
  return { store, snapshot, update, condition };
}
module.exports = { bind, state };
