// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The original AST fixes the entire switch, fallthrough, delegate and finalizer.
const assert = require('node:assert/strict');
const registry = new WeakMap();
function read(ts, original) {
  const file = ts.createSourceFile('original.ts', original, ts.ScriptTarget.Latest, true);
  assert.equal(file.parseDiagnostics.length, 0); assert.equal(file.statements.length, 3);
  const [fn, iterator, result] = file.statements;
  assert(ts.isFunctionDeclaration(fn) && fn.name && fn.asteriskToken && fn.body);
  assert.deepEqual(fn.modifiers.map(m => m.kind), [ts.SyntaxKind.ExportKeyword]);
  assert.equal(fn.parameters.length, 1); assert(ts.isIdentifier(fn.parameters[0].name));
  const parameter = fn.parameters[0].name.text;
  assert.equal(fn.body.statements.length, 1);
  const region = fn.body.statements[0];
  assert(ts.isTryStatement(region) && !region.catchClause && region.finallyBlock);
  assert.equal(region.tryBlock.statements.length, 2);
  const [selection, completion] = region.tryBlock.statements;
  assert(ts.isSwitchStatement(selection) && ts.isIdentifier(selection.expression));
  assert.equal(selection.expression.text, parameter);
  assert.equal(selection.caseBlock.clauses.length, 3);
  const [first, second, fallback] = selection.caseBlock.clauses;
  assert(ts.isCaseClause(first) && first.expression.getText(file) === '1');
  assert(ts.isCaseClause(second) && second.expression.getText(file) === '2');
  assert(ts.isDefaultClause(fallback));
  function yielding(clause, count, delegated) {
    assert.equal(clause.statements.length, count);
    const statement = clause.statements[0]; assert(ts.isExpressionStatement(statement));
    const expression = statement.expression; assert(ts.isYieldExpression(expression));
    assert.equal(Boolean(expression.asteriskToken), delegated); assert(expression.expression);
    return expression;
  }
  const one = yielding(first, 1, false), delegated = yielding(second, 2, true), zero = yielding(fallback, 1, false);
  assert.equal(one.expression.getText(file), '10'); assert.equal(zero.expression.getText(file), '0');
  assert(ts.isBreakStatement(second.statements[1]) && !second.statements[1].label);
  const array = delegated.expression;
  assert(ts.isArrayLiteralExpression(array) && array.elements.every(ts.isNumericLiteral));
  assert.deepEqual(array.elements.map(e => e.text), ['20', '30']);
  assert(ts.isReturnStatement(completion) && completion.expression.getText(file) === '42');
  assert.equal(region.finallyBlock.statements.length, 1);
  const finalizer = region.finallyBlock.statements[0];
  assert(ts.isExpressionStatement(finalizer) && ts.isBinaryExpression(finalizer.expression));
  assert.equal(finalizer.expression.operatorToken.kind, ts.SyntaxKind.PlusEqualsToken);
  assert.equal(finalizer.expression.left.getText(file), parameter);
  assert.equal(finalizer.expression.right.getText(file), '1');
  function declaration(statement) {
    assert(ts.isVariableStatement(statement) && statement.declarationList.declarations.length === 1);
    const node = statement.declarationList.declarations[0]; assert(ts.isIdentifier(node.name) && node.initializer); return node;
  }
  const binding = declaration(iterator), exported = declaration(result);
  assert(!iterator.modifiers?.length); assert(result.modifiers.some(m => m.kind === ts.SyntaxKind.ExportKeyword));
  assert(ts.isCallExpression(binding.initializer) && binding.initializer.expression.getText(file) === fn.name.text);
  assert.equal(binding.initializer.arguments.length, 1); assert.equal(binding.initializer.arguments[0].getText(file), '1');
  function sum(node) {
    if (ts.isBinaryExpression(node)) {assert.equal(node.operatorToken.kind, ts.SyntaxKind.PlusToken); return sum(node.left) + sum(node.right);}
    assert(ts.isPropertyAccessExpression(node) && node.name.text === 'value' && ts.isCallExpression(node.expression));
    assert.equal(node.expression.arguments.length, 0);
    assert.equal(node.expression.expression.getText(file), binding.name.text + '.next'); return 1;
  }
  assert.equal(sum(exported.initializer), 4);
  const identifiers = new Set();
  function names(node) {if (ts.isIdentifier(node)) identifiers.add(node.text); ts.forEachChild(node, names);}
  names(file);
  const origin = Object.freeze({file, fn, parameter, region, selection, first, second, fallback, one, delegated, zero, array,
    completion, finalizer, iterator, binding, result, exported, identifiers});
  const identities = [];
  function identity(node) {
    const children = []; ts.forEachChild(node, child => {children.push(child);});
    identities.push({node, parent: node.parent, kind: node.kind, pos: node.pos, end: node.end,
      text: node.text, escapedText: node.escapedText, children});
    for (const child of children) identity(child);
  }
  identity(file); registry.set(origin, {ts, text: file.text, identities}); return origin;
}
function checked(origin) {
  const record = registry.get(origin);
  assert(record && origin.fn.parent === origin.file, 'exact independently read original generator source');
  assert.equal(origin.file.text, record.text, 'the original source text is unchanged');
  const identifiers = new Set();
  for (const item of record.identities) {
    const node = item.node, children = []; record.ts.forEachChild(node, child => {children.push(child);});
    assert.equal(node.parent, item.parent); assert.equal(node.kind, item.kind);
    assert.equal(node.pos, item.pos); assert.equal(node.end, item.end);
    assert.equal(node.text, item.text); assert.equal(node.escapedText, item.escapedText);
    if (record.ts.isIdentifier(node)) identifiers.add(node.text);
    assert.equal(children.length, item.children.length);
    for (let index = 0; index < children.length; index++) assert.equal(children[index], item.children[index], 'the complete original AST ancestry and child inventory is unchanged');
  }
  assert.deepEqual([...origin.identifiers].sort(), [...identifiers].sort(), 'the exact original AST identifier inventory is unchanged');
  return origin;
}
module.exports = {read, checked};
