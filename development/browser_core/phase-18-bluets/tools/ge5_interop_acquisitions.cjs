// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each source edge acquires one exact module and its selected interop wrapper.
const assert = require('node:assert/strict');
const imports = require('./ge5_interop_imports.cjs'), helpers = require('./ge5_interop_helpers.cjs');

function read(ts, binding, helper, view, options, blue) {
  const model = imports.checked(binding, view), hp = helpers.checked(helper, view), {item, module} = model;
  assert.equal(hp.item, item); assert.equal(hp.module, module);
  const edges = [], consumed = new Set();
  function identifier(node, name, label) { assert(ts.isIdentifier(node) && node.text === name, label); }
  function requireCall(node, edge) {
    assert(ts.isCallExpression(node) && !node.questionDotToken && !node.typeArguments?.length && node.arguments.length === 1);
    identifier(node.expression, 'require', 'exact module acquisition intrinsic');
    assert(ts.isStringLiteral(node.arguments[0]) && node.arguments[0].text === edge.specifier, 'exact original acquired module');
  }
  function wrap(edge) {
    if (!options.esModuleInterop || edge.star) return null;
    const member = edge.bindings[0]?.imported;
    return member === 'default' ? '__importDefault' : member === '*' ? '__importStar' : null;
  }
  for (const acquisition of model.acquisitions) {
    const {edge, namespace, node} = acquisition, member = wrap(edge);
    if (node && (ts.isImportDeclaration(node) || ts.isExportDeclaration(node))) {
      consumed.add(node); edges.push(Object.freeze({edge, namespace, node, transport: 'preserved'})); continue;
    }
    if (item.format === 'system') {
      if (node) consumed.add(node);
      edges.push(Object.freeze({edge, namespace, node, transport: 'setter'})); continue;
    }
    if (!blue && String(item.module).toLowerCase() === 'amd') {
      assert(ts.isParameter(node));
      if (!edge.star && member) {
        const assignments = item.body.filter(statement => ts.isExpressionStatement(statement)
          && ts.isBinaryExpression(statement.expression) && ts.isIdentifier(statement.expression.left)
          && statement.expression.left.text === namespace);
        assert.equal(assignments.length, 1, 'one exact AMD interop wrapper assignment');
        const statement = assignments[0], expression = statement.expression;
        assert.equal(expression.operatorToken.kind, ts.SyntaxKind.EqualsToken);
        assert(ts.isCallExpression(expression.right) && !expression.right.questionDotToken && expression.right.arguments.length === 1);
        helpers.reference(ts, helper, view, expression.right.expression, member);
        identifier(expression.right.arguments[0], namespace, 'AMD wrapper retains its own acquired module');
        consumed.add(statement);
        edges.push(Object.freeze({edge, namespace, node: statement, parameter: node, transport: 'amd-wrapper'}));
      } else if (edge.star) {
        const calls = item.body.filter(statement => ts.isExpressionStatement(statement) && ts.isCallExpression(statement.expression)
          && statement.expression.arguments.length === 2 && ts.isIdentifier(statement.expression.arguments[0])
          && statement.expression.arguments[0].text === namespace);
        assert.equal(calls.length, 1, 'one exact AMD original star publication');
        const statement = calls[0]; helpers.reference(ts, helper, view, statement.expression.expression, '__exportStar');
        identifier(statement.expression.arguments[1], 'exports', 'exact original star export destination'); consumed.add(statement);
        edges.push(Object.freeze({edge, namespace, node: statement, parameter: node, transport: 'helper-star'}));
      } else edges.push(Object.freeze({edge, namespace, node, transport: 'amd-parameter'}));
      continue;
    }
    assert(node, 'complete acquired source edge');
    if (ts.isExpressionStatement(node)) {
      assert(edge.star && ts.isCallExpression(node.expression) && node.expression.arguments.length === 2);
      helpers.reference(ts, helper, view, node.expression.expression, '__exportStar');
      requireCall(node.expression.arguments[0], edge);
      identifier(node.expression.arguments[1], 'exports', 'exact original star export destination');
      consumed.add(node); edges.push(Object.freeze({edge, namespace, node, transport: 'helper-star'})); continue;
    }
    assert(ts.isVariableStatement(node) && node.declarationList.declarations.length === 1);
    const declaration = node.declarationList.declarations[0];
    identifier(declaration.name, namespace, 'exact bound acquisition namespace');
    assert.equal(node.declarationList.flags & (ts.NodeFlags.Let | ts.NodeFlags.Const),
      options.target === 'ES5' ? 0 : ts.NodeFlags.Const, 'complete acquired binding declaration kind');
    let value = declaration.initializer;
    if (member) {
      assert(ts.isCallExpression(value) && !value.questionDotToken && value.arguments.length === 1);
      helpers.reference(ts, helper, view, value.expression, member); value = value.arguments[0];
    }
    requireCall(value, edge); consumed.add(node);
    edges.push(Object.freeze({edge, namespace, node, transport: edge.star ? 'inline-star' : 'require'}));
  }
  return Object.freeze({edges: Object.freeze(edges), consumed: Object.freeze([...consumed]), completeModuleValidated: false});
}

module.exports = {read};
