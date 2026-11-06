// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Source declaration syntax surrounds independently paired complete expressions.
const assert = require('node:assert/strict');
const declarations = require('./ge5_interop_declarations.cjs'), expressions = require('./ge5_interop_expression_maps.cjs');

function compare(ts, native, nb, actual, ab, accounting) {
  const n = declarations.read(ts, nb, native), a = declarations.read(ts, ab, actual), endpoints = [], expressionPairs = [];
  function point(label, nn, an) {
    assert.deepEqual(actual.owner(an.end), native.owner(nn.end), 'exact interop declaration logical endpoint: ' + label);
    endpoints.push(Object.freeze({label, native: nn, actual: an}));
  }
  function uniform(view, start, end, owner, label) {
    for (let offset = start; offset < end; offset++) assert.deepEqual(view.owner(offset), owner, label);
    view.cover(start, end); view.roleStarts.set(start, owner); view.roleEnds.set(end, view.owner(end));
  }
  for (let index = 0; index < n.declarations.length; index++) {
    const x = n.declarations[index], y = a.declarations[index]; assert.equal(x.original, y.original);
    function expression(original, nn, an) {
      expressionPairs.push(...expressions.compare(ts, native, nb, actual, ab, original, nn, an, accounting).pairs);
    }
    if (x.expression) {
      const name = node => ts.isVariableDeclaration(node) ? node.name
        : ts.isIdentifier(node.left) ? node.left : node.left.name;
      point('variable binding name', name(x.node), name(y.node));
      expression(x.original.node.initializer, x.expression, y.expression);
      if (x.publication) {
        const left = x.node.left, right = y.node.left;
        assert(ts.isIdentifier(left) && (y.publication ? ts.isIdentifier(right) : ts.isPropertyAccessExpression(right)));
        const rightName = y.publication ? right : right.name;
        const owner = native.owner(x.publication.getStart(native.source));
        uniform(native, x.publication.getStart(native.source), left.getStart(native.source), owner,
          'complete Native System variable publication prefix');
        uniform(actual, (y.publication || right).getStart(actual.source), rightName.getStart(actual.source), owner,
          'complete Blue System variable publication prefix');
        accounting.pair([native.token(left), native.token(x.node.operatorToken)],
          [actual.token(rightName), actual.token(y.node.operatorToken)], 'complete original System variable binding');
      } else {
        accounting.pair(native.tokens(x.statement).filter(token => token.end <= x.expression.getStart(native.source)),
          actual.tokens(y.statement).filter(token => token.end <= y.expression.getStart(actual.source)),
          'complete original variable declaration prefix');
      }
      const owner = native.owner(x.expression.end);
      uniform(native, x.expression.end, x.statement.end, owner, 'complete Native variable declaration suffix');
      uniform(actual, y.expression.end, y.statement.end, owner, 'complete variable declaration suffix');
      point('variable expression', x.node, y.node); point('variable statement', x.statement, y.statement);
    } else {
      assert.equal(x.node.modifiers?.length || 0, y.node.modifiers?.length || 0);
      (x.node.modifiers || []).forEach((modifier, index) => point('function modifier', modifier, y.node.modifiers[index]));
      for (let statement = 0; statement < x.body.statements.length; statement++) {
        const nn = x.body.statements[statement], an = y.body.statements[statement];
        expression(x.original.node.body.statements[statement].expression, nn.expression, an.expression);
        point('function statement', nn, an);
      }
      accounting.pair(native.tokens(x.node).filter(token => !native.covered(token.start)),
        actual.tokens(y.node).filter(token => !actual.covered(token.start)), 'complete original function syntax');
      point('function body', x.body, y.body); point('function declaration', x.node, y.node);
      x.node.parameters.forEach((parameter, index) => point('function parameter', parameter, y.node.parameters[index]));
      x.node.parameters.forEach((parameter, index) => point('function parameter name', parameter.name, y.node.parameters[index].name));
    }
  }
  return Object.freeze({native: n, actual: a, endpoints: Object.freeze(endpoints),
    expressionPairs: Object.freeze(expressionPairs), completeModuleValidated: false});
}

module.exports = {compare};
