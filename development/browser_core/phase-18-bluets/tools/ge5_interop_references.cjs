// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Semantic reference pairs precede source-role comparison; they do not prove maps.
const assert = require('node:assert/strict');
const imports = require('./ge5_interop_imports.cjs');

function compare(ts, capability, view, original, actual) {
  const model = imports.checked(capability, view), {module, item} = model;
  function owned(root) {
    const nodes = new WeakSet();
    function visit(node) { nodes.add(node); ts.forEachChild(node, visit); }
    visit(root); return nodes;
  }
  const originals = owned(module.source), emitted = owned(view.source), pairs = [];
  assert(originals.has(original) && emitted.has(actual), 'exact original and emitted AST identities');
  function enclosingFunction(node) {
    for (let parent = node.parent; parent; parent = parent.parent) {
      if (ts.isFunctionLike(parent)) return parent;
    }
    return null;
  }
  const originalFunction = enclosingFunction(original), actualFunction = enclosingFunction(actual);
  const parameters = new Set();
  if (originalFunction) {
    assert(module.declarations.some(declaration => declaration.node === originalFunction), 'original module function scope');
    assert(ts.isFunctionDeclaration(actualFunction) && actualFunction.name?.text === originalFunction.name.text,
      'matching emitted function scope');
    const names = nodes => nodes.map(parameter => {
      assert(ts.isIdentifier(parameter.name) && !parameter.initializer && !parameter.dotDotDotToken);
      return parameter.name.text;
    });
    assert.deepEqual(names(actualFunction.parameters), names(originalFunction.parameters), 'exact lexical parameter bindings');
    for (const name of names(originalFunction.parameters)) parameters.add(name);
  } else assert(!actualFunction || actualFunction === item.execute || actualFunction === item.factory,
    'module expression retains its execution scope');
  const preserved = item.body.some(ts.isImportDeclaration) || item.body.some(node =>
    node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword));
  const exportedVariables = new Set(module.exports.filter(edge =>
    module.declarations.some(declaration => declaration.kind === 'variable' && declaration.node === edge.node))
    .map(edge => edge.local));
  const nativeSystem = item.format === 'system' && !item.system.stem;
  function identifier(node, name, label) {
    assert(ts.isIdentifier(node) && node.text === name, label);
  }
  function property(node, namespace, member, label) {
    assert(ts.isPropertyAccessExpression(node) && !node.questionDotToken && node.name.text === member, label);
    identifier(node.expression, namespace, label);
  }
  function pair(left, right) {
    assert(originals.has(left) && emitted.has(right), 'owned reference subtree');
    const record = {original: left, actual: right, role: 'expression'};
    if (ts.isIdentifier(left)) {
      const name = left.text, binding = !parameters.has(name) && model.symbols[name];
      if (parameters.has(name)) {
        identifier(right, name, 'lexically shadowed parameter reference'); record.role = 'parameter';
      } else if (binding) {
        if (binding.direct) identifier(right, name, 'direct original import reference');
        else property(right, binding.namespace, binding.imported, 'independently bound imported reference');
        record.role = 'import';
      } else {
        assert(module.declarations.some(declaration => declaration.name === name), 'known original module binding');
        if (exportedVariables.has(name) && !preserved && !nativeSystem) {
          property(right, 'exports', name, 'live exported module variable');
        } else identifier(right, name, 'original local module reference');
        record.role = 'local';
      }
    } else if (ts.isNumericLiteral(left) || ts.isStringLiteral(left)) {
      assert.equal(right.kind, left.kind); assert.equal(right.text, left.text, 'unchanged source literal');
    } else if (ts.isPropertyAccessExpression(left)) {
      assert(ts.isPropertyAccessExpression(right) && !left.questionDotToken && !right.questionDotToken);
      assert.equal(right.name.text, left.name.text, 'original source property member');
      pair(left.expression, right.expression);
    } else if (ts.isCallExpression(left)) {
      assert(ts.isCallExpression(right) && !left.questionDotToken && !right.questionDotToken);
      assert.equal(right.arguments.length, left.arguments.length, 'complete original call arguments');
      let callee = right.expression;
      const binding = ts.isIdentifier(left.expression) && !parameters.has(left.expression.text)
        && model.symbols[left.expression.text];
      if (binding && !binding.direct && item.format !== 'system') {
        assert(ts.isParenthesizedExpression(callee) && ts.isBinaryExpression(callee.expression)
          && callee.expression.operatorToken.kind === ts.SyntaxKind.CommaToken
          && ts.isNumericLiteral(callee.expression.left) && callee.expression.left.text === '0',
        'exact unbound imported-call transport');
        record.transport = callee; callee = callee.expression.right;
      }
      pair(left.expression, callee);
      left.arguments.forEach((argument, index) => pair(argument, right.arguments[index]));
    } else if (ts.isBinaryExpression(left)) {
      if (nativeSystem && left.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(left.left)
        && !parameters.has(left.left.text) && exportedVariables.has(left.left.text)) {
        assert(ts.isCallExpression(right) && right.arguments.length === 2, 'live System assignment publication');
        identifier(right.expression, item.system.publisher, 'owned System publisher');
        assert(ts.isStringLiteral(right.arguments[0]) && right.arguments[0].text === left.left.text,
          'exact System live-binding publication name');
        record.transport = right; right = right.arguments[1];
      }
      assert(ts.isBinaryExpression(right));
      assert.equal(right.operatorToken.kind, left.operatorToken.kind, 'original expression operator');
      pair(left.left, right.left); pair(left.right, right.right);
    } else if (ts.isParenthesizedExpression(left)) {
      assert(ts.isParenthesizedExpression(right)); pair(left.expression, right.expression);
    } else assert.fail('unclassified original interop expression: ' + ts.SyntaxKind[left.kind]);
    pairs.push(Object.freeze(record));
  }
  pair(original, actual);
  return Object.freeze(pairs);
}

module.exports = {compare};
