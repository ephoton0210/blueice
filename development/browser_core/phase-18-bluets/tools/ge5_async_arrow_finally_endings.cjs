// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Logical endings are independent roles, including erased parameter annotations.
const assert = require('node:assert/strict');
function compare(ts, native, n, actual, a, origin, options, callables, roles) {
  assert(n.view === native && a.view === actual);
  assert(n.origin === origin && a.origin === origin);
  const pairs = [], pair = (label, left, right) => {
    assert(left && right, label);
    assert(left.getSourceFile() === native.source && right.getSourceFile() === actual.source,
      'each endpoint belongs to its exact independently parsed emitted tree');
    pairs.push({label, native: left, actual: right});
  };
  assert.equal(n.compute.name.text, origin.compute.name.text);
  assert.equal(a.compute.name.text, origin.compute.name.text);
  pair('named callable name', n.compute.name, a.compute.name);
  pair('named callable body', n.compute.body, a.compute.body);
  pair('named callable declaration', n.compute, a.compute);
  pair('source arrow binding name', ts.isVariableDeclaration(n.binding) ? n.binding.name : n.binding,
    ts.isVariableDeclaration(a.binding) ? a.binding.name : a.binding);
  pair('source arrow initializer', n.expression, a.expression);
  pair('source arrow statement', n.statement, a.statement);
  for (const kind of ['compute', 'arrow']) {
    const left = n[kind], right = a.retained ? a[kind] : a[kind + 'Frame'].callback;
    assert.equal(left.parameters.length, origin[kind].parameters.length);
    assert.equal(right.parameters.length, origin[kind].parameters.length);
    for (let index = 0; index < left.parameters.length; index++) {
      assert.equal(left.parameters[index].name.text, origin[kind].parameters[index].name.text);
      assert.equal(right.parameters[index].name.text, origin[kind].parameters[index].name.text);
      pair(kind + ' source parameter name ' + index, left.parameters[index].name, right.parameters[index].name);
      pair(kind + ' source parameter declaration ' + index, left.parameters[index], right.parameters[index]);
    }
    if (!n.retained) {
      pair(kind + ' generated callback body', n[kind + 'Frame'].callback.body, a[kind + 'Frame'].callback.body);
      pair(kind + ' generated callback', n[kind + 'Frame'].callback, a[kind + 'Frame'].callback);
    }
  }
  if (n.retained || options.target === 'ES2015') {
    function children(node) {
      const result = []; ts.forEachChild(node, child => { result.push(child); }); return result;
    }
    function walk(source, left, right, label) {
      pair(label, left, right);
      if (ts.isAwaitExpression(source) && !n.retained) {
        assert(ts.isYieldExpression(left) && ts.isParenthesizedExpression(right));
        const yielded = right.expression;
        assert(ts.isYieldExpression(yielded) && ts.isParenthesizedExpression(yielded.expression));
        walk(source.expression, left.expression, yielded.expression.expression, label + '/operand');
        return;
      }
      assert.equal(left.kind, source.kind, label + ' Native source AST kind');
      assert.equal(right.kind, source.kind, label + ' actual source AST kind');
      const original = children(source), lhs = children(left), rhs = children(right);
      assert.equal(lhs.length, original.length); assert.equal(rhs.length, original.length);
      for (let index = 0; index < original.length; index++) {
        walk(original[index], lhs[index], rhs[index], label + '/' + index);
      }
    }
    for (const kind of ['compute', 'arrow']) {
      walk(origin[kind].body, n.retained ? n[kind].body : n[kind + 'Frame'].callback.body,
        a.retained ? a[kind].body : a[kind + 'Frame'].callback.body, kind + ' original body AST');
    }
  } else {
    assert.equal(options.target, 'ES5');
    for (const kind of ['compute', 'arrow']) {
      const core = callables[kind].core; let index = 0;
      for (const trace of core.nativePaths) for (const value of trace.values) {
        assert(value.node && value.peer?.node, 'every source evaluation has its proved peer');
        pair(kind + ' source expression DAG ' + index++ + ' ' + value.kind, value.node, value.peer.node);
      }
    }
  }
  for (const [label, role] of roles) {
    pair('module statement ' + label, role.native, role.actual);
    if (ts.isVariableStatement(role.native) && ts.isVariableStatement(role.actual)) {
      const left = role.native.declarationList.declarations, right = role.actual.declarationList.declarations;
      assert.equal(left.length, right.length);
      for (let index = 0; index < left.length; index++) {
        pair('module binding ' + label + '/' + index, left[index].name, right[index].name);
        pair('module declaration ' + label + '/' + index, left[index], right[index]);
      }
    }
  }
  for (const item of pairs) {
    assert.deepEqual(actual.owner(item.actual.end), native.owner(item.native.end), item.label + ' exact logical ending');
  }
  // A generated outer arity has no source identity. Its preceding whitespace
  // may carry only the exact original function-name completion already proved.
  if (!n.retained) {
    const opening = actual.tokens(a.compute).find(token => token.text === '(');
    assert(opening && opening.start >= a.compute.name.end);
    const owner = native.owner(n.compute.name.end);
    for (let offset = a.compute.name.end; offset < opening.start; offset++) {
      assert(/\s/.test(actual.code[offset]), 'name completion carrier contains only whitespace');
      assert.deepEqual(actual.owner(offset), owner, 'exact named callable completion carrier');
    }
    actual.cover(a.compute.name.end, opening.start);
  }
  return pairs;
}
module.exports = {compare};
