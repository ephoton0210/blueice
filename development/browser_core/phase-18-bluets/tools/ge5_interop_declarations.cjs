// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every original declaration precedes independent accounting of module scaffolds.
const assert = require('node:assert/strict');
const imports = require('./ge5_interop_imports.cjs');

function read(ts, capability, view) {
  const model = imports.checked(capability, view), {module, item} = model;
  const declarations = [], consumed = new Set();
  const preserved = item.body.some(ts.isImportDeclaration) || item.body.some(node =>
    node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword));
  const nativeSystem = item.format === 'system' && !item.system.stem;
  function variable(node, original) {
    if (preserved) {
      if (!ts.isVariableStatement(node)) return null;
      const matches = node.declarationList.declarations.filter(declaration =>
        ts.isIdentifier(declaration.name) && declaration.name.text === original.name && declaration.initializer);
      assert(matches.length <= 1); if (!matches.length) return null;
      assert.equal(node.declarationList.declarations.length, 1, 'one complete original variable declaration');
      return {statement: node, node: matches[0], expression: matches[0].initializer};
    }
    if (!ts.isExpressionStatement(node)) return null;
    let expression = node.expression, publication = null;
    if (nativeSystem) {
      if (!ts.isCallExpression(expression) || !ts.isIdentifier(expression.expression)
        || expression.expression.text !== item.system.publisher || expression.arguments.length !== 2
        || !ts.isStringLiteral(expression.arguments[0]) || expression.arguments[0].text !== original.name) return null;
      publication = expression; expression = expression.arguments[1];
    }
    if (!ts.isBinaryExpression(expression) || expression.operatorToken.kind !== ts.SyntaxKind.EqualsToken) return null;
    const left = expression.left;
    if (nativeSystem ? !ts.isIdentifier(left) || left.text !== original.name
      : !ts.isPropertyAccessExpression(left) || left.questionDotToken || left.name.text !== original.name
        || !ts.isIdentifier(left.expression) || left.expression.text !== 'exports') return null;
    if (ts.isVoidExpression(expression.right)) return null; // Predeclarations have a separate generated role.
    return {statement: node, node: expression, expression: expression.right, publication};
  }
  for (const original of module.declarations) {
    if (original.kind === 'variable') {
      assert(module.exports.some(edge => edge.local === original.name), 'reviewed exported original variable');
      const matches = item.body.map(node => variable(node, original)).filter(Boolean);
      assert.equal(matches.length, 1, 'one complete original variable initializer: ' + original.name);
      const observed = matches[0]; assert(!consumed.has(observed.statement)); consumed.add(observed.statement);
      declarations.push(Object.freeze({original, ...observed}));
    } else {
      const matches = item.sourceNodes.filter(node => ts.isFunctionDeclaration(node) && node.name?.text === original.name);
      assert.equal(matches.length, 1, 'one complete original function declaration: ' + original.name);
      const node = matches[0];
      assert(node.body && !node.asteriskToken && !node.typeParameters?.length && !node.type,
        'complete ordinary original function');
      assert(!node.modifiers?.some(modifier => ![ts.SyntaxKind.ExportKeyword, ts.SyntaxKind.DefaultKeyword].includes(modifier.kind)),
        'original callable kind');
      assert.deepEqual(node.parameters.map(parameter => {
        assert(ts.isIdentifier(parameter.name) && !parameter.initializer && !parameter.dotDotDotToken
          && !parameter.questionToken && !parameter.type && !parameter.modifiers?.length);
        return parameter.name.text;
      }), original.parameters, 'complete original parameter inventory');
      assert.equal(node.body.statements.length, original.node.body.statements.length, 'complete original function body inventory');
      node.body.statements.forEach((statement, index) => {
        assert.equal(statement.kind, original.node.body.statements[index].kind, 'original function statement kind');
        assert(ts.isReturnStatement(statement) || ts.isExpressionStatement(statement), 'reviewed original function statement');
        assert(statement.expression, 'complete source expression');
      });
      consumed.add(node); declarations.push(Object.freeze({original, node, statement: node, body: node.body}));
    }
  }
  return Object.freeze({declarations: Object.freeze(declarations),
    remaining: Object.freeze(item.sourceNodes.filter(node => !consumed.has(node))), completeModuleValidated: false});
}

module.exports = {read};
