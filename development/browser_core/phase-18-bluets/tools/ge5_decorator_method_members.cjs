// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original literal keys and callable bodies own the lowered prototype assignments.
const assert = require('node:assert/strict'), capabilities = new WeakSet();
function source(ts, klass) {
  assert(ts.isClassDeclaration(klass) && klass.name && !klass.heritageClauses?.length);
  assert(!ts.getDecorators(klass)?.length, 'member-only original method class');
  assert(klass.members.length && klass.members.every(ts.isMethodDeclaration));
  const keys = new Set();
  for (const method of klass.members) {
    assert(ts.isStringLiteral(method.name) || ts.isNumericLiteral(method.name), 'literal source method key');
    assert(!method.asteriskToken && !method.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword));
    assert(!method.parameters.length && !method.typeParameters?.length && method.body);
    assert(ts.getDecorators(method)?.length, 'original method is independently decorated');
    const key = (!!method.modifiers?.some(m => m.kind === ts.SyntaxKind.StaticKeyword)) + ':' + method.name.text;
    assert(!keys.has(key), 'unique original literal method'); keys.add(key);
  }
  const cap = Object.freeze({klass, methods: Object.freeze([...klass.members])});
  capabilities.add(cap); return cap;
}

function body(ts, original, emitted) {
  assert(emitted && emitted.kind === original.kind, 'original method body AST kind');
  if (ts.isIdentifier(original) || ts.isNumericLiteral(original) || ts.isStringLiteral(original)) {
    assert.equal(emitted.text, original.text, 'original method body leaf value');
  }
  const left = [], right = [];
  ts.forEachChild(original, node => { left.push(node); });
  ts.forEachChild(emitted, node => { right.push(node); });
  assert.equal(right.length, left.length, 'original method body complete child inventory');
  for (let index = 0; index < left.length; index++) body(ts, left[index], right[index]);
}

function read(ts, view, item, cap) {
  assert(capabilities.has(cap), 'unforgeable original method capability');
  assert(item.view === view && item.node.getSourceFile() === view.source);
  const methods = [], used = new Set(), name = cap.klass.name.text;
  function add(key, isStatic, fn, statement, keyNode) {
    const candidates = cap.methods.filter(method => method.name.text === key
      && !!method.modifiers?.some(m => m.kind === ts.SyntaxKind.StaticKeyword) === isStatic);
    assert.equal(candidates.length, 1, 'one exact original literal method key/static identity');
    const original = candidates[0]; assert(!used.has(original), 'one emitted function per original literal method');
    assert(!fn.parameters.length && !fn.asteriskToken
      && !fn.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword), 'original literal method callable signature');
    if (!item.native) assert(ts.isFunctionExpression(fn) && !fn.name, 'anonymous lowered original method value');
    body(ts, original.body, fn.body); used.add(original);
    methods.push({source: original, fn, statement, keyNode});
  }
  if (item.native) {
    for (const method of item.node.members.filter(ts.isMethodDeclaration)) {
      add(method.name.text, !!method.modifiers?.some(m => m.kind === ts.SyntaxKind.StaticKeyword),
        method, method, method.name);
    }
  } else {
    for (const statement of item.factory.body.statements) {
      if (statement === item.constructor || statement === item.returned) continue;
      if (ts.isExpressionStatement(statement) && ts.isCallExpression(statement.expression)
        && ts.isPropertyAccessExpression(statement.expression.expression)
        && statement.expression.expression.expression.getText() === 'Object'
        && statement.expression.expression.name.text === 'defineProperty') {
        const args = statement.expression.arguments;
        assert.equal(args.length, 3, 'original method complete descriptor arguments');
        const receiver = args[0], key = args[1], descriptor = args[2];
        const isStatic = ts.isIdentifier(receiver) && receiver.text === name;
        assert(isStatic || (ts.isPropertyAccessExpression(receiver) && receiver.name.text === 'prototype'
          && ts.isIdentifier(receiver.expression) && receiver.expression.text === name), 'original method descriptor receiver');
        assert(ts.isStringLiteral(key) || ts.isNumericLiteral(key), 'original method descriptor literal key');
        assert(ts.isObjectLiteralExpression(descriptor), 'original method descriptor object');
        const labels = descriptor.properties.map(property => property.name?.getText());
        assert(['value,writable,enumerable,configurable', 'enumerable,configurable,writable,value']
          .includes(labels.join(',')), 'original method complete descriptor inventory');
        assert(descriptor.properties.every(ts.isPropertyAssignment));
        const properties = new Map(descriptor.properties.map(property => [property.name.getText(), property.initializer]));
        assert.deepEqual(['writable', 'enumerable', 'configurable'].map(label => properties.get(label).kind),
          [ts.SyntaxKind.TrueKeyword, ts.SyntaxKind.FalseKeyword, ts.SyntaxKind.TrueKeyword], 'original method descriptor flags');
        add(key.text, isStatic, properties.get('value'), statement, key);
        continue;
      }
      if (!ts.isExpressionStatement(statement) || !ts.isBinaryExpression(statement.expression)
        || statement.expression.operatorToken.kind !== ts.SyntaxKind.EqualsToken) continue;
      const assignment = statement.expression, left = assignment.left;
      if (!ts.isPropertyAccessExpression(left) && !ts.isElementAccessExpression(left)) continue;
      const receiver = left.expression;
      let isStatic;
      if (ts.isIdentifier(receiver) && receiver.text === name) isStatic = true;
      else if (ts.isPropertyAccessExpression(receiver) && receiver.name.text === 'prototype'
        && ts.isIdentifier(receiver.expression) && receiver.expression.text === name) isStatic = false;
      else continue;
      const key = ts.isPropertyAccessExpression(left) ? left.name : left.argumentExpression;
      assert(key && (ts.isIdentifier(key) || ts.isStringLiteral(key) || ts.isNumericLiteral(key)),
        'closed emitted original literal method reference');
      add(key.text, isStatic, assignment.right, statement, key);
    }
  }
  assert.equal(used.size, cap.methods.length, 'complete original literal method inventory');
  return {...item, methods};
}

function compare(ts, native, n, actual, a, cap) {
  assert(capabilities.has(cap), 'unforgeable original method capability');
  const pairs = [];
  function point(label, left, right) {
    assert(left.getSourceFile() === native.source && right.getSourceFile() === actual.source);
    assert.deepEqual(actual.owner(right.end), native.owner(left.end), 'original literal method exact ending: ' + label);
    pairs.push({label, native: left, actual: right});
  }
  function subtree(left, right, label) {
    assert.equal(right.kind, left.kind, label + ' paired body AST kind'); point(label, left, right);
    const lhs = [], rhs = [];
    ts.forEachChild(left, node => { lhs.push(node); }); ts.forEachChild(right, node => { rhs.push(node); });
    assert.equal(rhs.length, lhs.length);
    for (let index = 0; index < lhs.length; index++) subtree(lhs[index], rhs[index], label + '/' + index);
  }
  for (const original of cap.methods) {
    const find = item => { const matches = item.methods.filter(method => method.source === original);
      assert.equal(matches.length, 1); return matches[0]; };
    const left = find(n), right = find(a), label = original.name.text;
    point(label + ' name', left.keyNode, right.keyNode);
    point(label + ' definition', left.statement, right.statement);
    point(label + ' callable', left.fn, right.fn);
    subtree(left.fn.body, right.fn.body, label + ' body');
  }
  return {source: cap, native: n, actual: a, pairs};
}
module.exports = {source, read, compare};
