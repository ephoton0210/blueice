// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Compare helper semantics through independently bound function ASTs.
const assert = require('node:assert/strict');
const helpers = new Map([
  ['__decorate', 'decorate'], ['__bluetsDecorate', 'decorate'],
  ['__param', 'param'], ['__bluetsParam', 'param'],
  ['__metadata', 'metadata'], ['__bluetsMetadata', 'metadata'],
  ['__esDecorate', 'esDecorate'], ['__bluetsEsDecorate', 'esDecorate'],
  ['__runInitializers', 'runInitializers'], ['__bluetsRunInitializers', 'runInitializers'],
]);

function semantic(ts, fn) {
  let next = 0;
  function functionScope(node, parent) {
    const scope = new Map(parent), local = new Map();
    function declare(name) {
      assert(ts.isIdentifier(name), 'plain generated helper binding');
      if (!local.has(name.text)) local.set(name.text, '$' + next++);
      scope.set(name.text, local.get(name.text));
    }
    for (const parameter of node.parameters) declare(parameter.name);
    if (node.name) declare(node.name);
    function hoist(child) {
      if (ts.isFunctionDeclaration(child)) { if (child.name) declare(child.name); return; }
      if (ts.isFunctionExpression(child) || ts.isArrowFunction(child)) return;
      if (ts.isVariableDeclaration(child)) declare(child.name);
      ts.forEachChild(child, hoist);
    }
    hoist(node.body);
    return [ts.SyntaxKind[node.kind], !!node.asteriskToken, !!node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.AsyncKeyword), node.parameters.map(p => visit(p, scope)), visit(node.body, scope)];
  }
  function visit(node, scope) {
    if (ts.isFunctionExpression(node) || ts.isFunctionDeclaration(node) || ts.isArrowFunction(node)) return functionScope(node, scope);
    if (ts.isParenthesizedExpression(node)) return visit(node.expression, scope);
    if (ts.isIdentifier(node)) {
      const property = ts.isPropertyAccessExpression(node.parent) && node.parent.name === node
        || ts.isPropertyAssignment(node.parent) && node.parent.name === node;
      return ['Identifier', property ? node.text : scope.get(node.text) || node.text];
    }
    if (ts.isStringLiteral(node) || ts.isNumericLiteral(node)) return [ts.SyntaxKind[node.kind], node.text];
    const children = []; ts.forEachChild(node, child => { children.push(visit(child, scope)); });
    if (ts.isPrefixUnaryExpression(node) || ts.isPostfixUnaryExpression(node)) return [ts.SyntaxKind[node.kind], node.operator, children];
    if (ts.isVariableDeclarationList(node)) return [ts.SyntaxKind[node.kind], node.flags & ts.NodeFlags.BlockScoped, children];
    return [ts.SyntaxKind[node.kind], children];
  }
  return functionScope(fn, new Map());
}

function read(ts, view, originals, blue) {
  const originalIdentifiers = new Set();
  for (const source of originals) {
    const ast = ts.createSourceFile('original.ts', source, ts.ScriptTarget.Latest, true);
    function sourceIdentifiers(node) { if (ts.isIdentifier(node)) originalIdentifiers.add(node.text); ts.forEachChild(node, sourceIdentifiers); }
    sourceIdentifiers(ast);
  }
  const bindings = new Map(), definitions = [], calls = [];
  function unwrap(node) { while (ts.isParenthesizedExpression(node)) node = node.expression; return node; }
  function definition(name, initializer, node) {
    if (!helpers.has(name)) return false;
    assert(!originalIdentifiers.has(name), 'generated helper must be fresh');
    const kind = helpers.get(name); assert(!bindings.has(name), 'unique helper binding');
    let fn = unwrap(initializer);
    if (!blue) {
      assert(ts.isBinaryExpression(fn) && fn.operatorToken.kind === ts.SyntaxKind.BarBarToken);
      const fallback = unwrap(fn.left), rhs = unwrap(fn.right);
      assert(ts.isBinaryExpression(fallback) && fallback.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken);
      assert.equal(fallback.left.kind, ts.SyntaxKind.ThisKeyword);
      assert(ts.isPropertyAccessExpression(fallback.right));
      assert.equal(fallback.right.expression.kind, ts.SyntaxKind.ThisKeyword);
      assert.equal(fallback.right.name.text, name);
      fn = rhs;
    }
    assert(ts.isFunctionExpression(fn), 'complete generated helper function');
    bindings.set(name, {kind, fn, shape: semantic(ts, fn)}); definitions.push(node);
    view.noSource(node.getStart(view.source), node.end, 'decorator helper definition');
    return true;
  }
  function visit(node) {
    if (ts.isVariableStatement(node) && node.declarationList.declarations.length === 1) {
      const item = node.declarationList.declarations[0];
      if (ts.isIdentifier(item.name) && item.initializer && definition(item.name.text, item.initializer, node)) return;
    }
    if (blue && ts.isExpressionStatement(node) && ts.isBinaryExpression(node.expression)) {
      const item = node.expression;
      if (item.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(item.left)
        && ts.isFunctionExpression(item.right) && definition(item.left.text, item.right, node)) return;
    }
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && helpers.has(node.expression.text)) calls.push(node);
    ts.forEachChild(node, visit);
  }
  visit(view.source);
  return {bindings, definitions, calls};
}

function compare(ts, native, actual, originals) {
  const n = read(ts, native, originals, false), a = read(ts, actual, originals, true);
  const expected = [...n.bindings.values()].map(item => item.kind).sort();
  assert.deepEqual([...a.bindings.values()].map(item => item.kind).sort(), expected, 'complete decorator helper inventory');
  for (const helper of n.bindings.values()) {
    const observed = [...a.bindings.values()].find(item => item.kind === helper.kind);
    assert.deepEqual(observed.shape, helper.shape, helper.kind + ': independently bound complete helper AST');
  }
  return {native: n, actual: a};
}
module.exports = {read, compare, semantic, helpers};
