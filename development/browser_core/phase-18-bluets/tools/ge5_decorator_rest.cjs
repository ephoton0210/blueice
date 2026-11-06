// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Match complete zero-position rest transport using the original signature and Native AST.
const assert = require('node:assert/strict');
const {compareTokens} = require('./ge5_role_stream.cjs');
function readPrologue(ts, view, statements, parameter, originalRoot) {
  assert(parameter.dotDotDotToken && ts.isIdentifier(parameter.name), 'one original rest binding');
  const name = parameter.name.text, sourceNames = new Set();
  function identifiers(node) { if (ts.isIdentifier(node)) sourceNames.add(node.text); ts.forEachChild(node, identifiers); }
  identifiers(originalRoot.getSourceFile());
  assert(statements.length >= 2, 'the complete rest array and loop are retained');
  const [array, loop] = statements;
  assert(ts.isVariableStatement(array) && array.declarationList.declarations.length === 1);
  const declaration = array.declarationList.declarations[0];
  assert(ts.isIdentifier(declaration.name) && declaration.name.text === name);
  assert(ts.isArrayLiteralExpression(declaration.initializer) && declaration.initializer.elements.length === 0);
  assert(ts.isForStatement(loop) && ts.isVariableDeclarationList(loop.initializer));
  assert.equal(loop.initializer.declarations.length, 1);
  const index = loop.initializer.declarations[0];
  assert(ts.isIdentifier(index.name) && !sourceNames.has(index.name.text), 'fresh rest transport index');
  assert(ts.isNumericLiteral(index.initializer) && index.initializer.text === '0', 'this complete ABI begins at the first source parameter');
  assert(ts.isBinaryExpression(loop.condition) && loop.condition.operatorToken.kind === ts.SyntaxKind.LessThanToken);
  assert(ts.isIdentifier(loop.condition.left) && loop.condition.left.text === index.name.text);
  const limit = loop.condition.right;
  assert(ts.isPropertyAccessExpression(limit) && ts.isIdentifier(limit.expression)
    && limit.expression.text === 'arguments' && limit.name.text === 'length');
  assert(ts.isPostfixUnaryExpression(loop.incrementor) && loop.incrementor.operator === ts.SyntaxKind.PlusPlusToken
    && ts.isIdentifier(loop.incrementor.operand) && loop.incrementor.operand.text === index.name.text);
  assert(ts.isBlock(loop.statement) && loop.statement.statements.length === 1);
  const copy = loop.statement.statements[0]; assert(ts.isExpressionStatement(copy));
  const assignment = copy.expression;
  assert(ts.isBinaryExpression(assignment) && assignment.operatorToken.kind === ts.SyntaxKind.EqualsToken);
  for (const [access, binding] of [[assignment.left, name], [assignment.right, 'arguments']]) {
    assert(ts.isElementAccessExpression(access) && ts.isIdentifier(access.expression) && access.expression.text === binding);
    assert(ts.isIdentifier(access.argumentExpression) && access.argumentExpression.text === index.name.text,
      'rest copy retains the complete source/destination index identity');
  }
  function tokens(node) {
    const values = node ? view.tokens(node) : [...view.tokens(array), ...view.tokens(loop)];
    return values.map(token => {
      if (token.text !== index.name.text) return token;
      const owner = token.owners[0];
      assert(token.owners.every(value => JSON.stringify(value) === JSON.stringify(owner)), 'entire fresh-index UTF-16 role');
      return {...token, text: '$rest.index', owners: [owner]};
    });
  }
  return {array, loop, index, copy, tokens};
}
function comparePrologue(ts, native, nativeStatements, actual, actualStatements, parameter, originalRoot) {
  const n = readPrologue(ts, native, nativeStatements, parameter, originalRoot);
  const a = readPrologue(ts, actual, actualStatements, parameter, originalRoot);
  compareTokens(native, n.tokens(), actual, a.tokens(), 'complete original rest prologue');
  return {native: n, actual: a};
}
function compareFunction(ts, native, nf, actual, af, original, options) {
  assert(ts.isFunctionDeclaration(nf) && ts.isFunctionDeclaration(af));
  assert.equal(nf.name?.text, original.name.text); assert.equal(af.name?.text, original.name.text);
  if (options.target !== 'ES5') {
    compareTokens(native, native.tokens(nf), actual, actual.tokens(af), 'complete original decorator function');
    return {native: nf, actual: af, original, rest: null};
  }
  assert.equal(original.parameters.length, 1, 'reviewed complete one-rest signature');
  assert(nf.body && af.body && nf.body.statements.length >= 3 && af.body.statements.length >= 3);
  const n = readPrologue(ts, native, nf.body.statements, original.parameters[0], original);
  const a = readPrologue(ts, actual, af.body.statements, original.parameters[0], original);
  compareTokens(native, n.tokens(nf), actual, a.tokens(af), 'complete rest signature/prologue/body');
  return {native: nf, actual: af, original, rest: {native: n, actual: a}};
}
function projectCallableRest(ts, view, emittedFn, originalCallable) {
  assert(ts.isFunctionDeclaration(emittedFn) || ts.isFunctionExpression(emittedFn), 'ordinary emitted rest callable');
  const parameters = originalCallable.parameters.filter(parameter => !ts.isIdentifier(parameter.name) || parameter.name.text !== 'this');
  assert.equal(parameters.length, 1, 'one effective source parameter after receiver erasure');
  assert(parameters[0].dotDotDotToken && ts.isIdentifier(parameters[0].name));
  assert.equal(emittedFn.parameters.length, 0, 'the complete effective rest signature was lowered');
  assert(emittedFn.body && ts.isBlock(emittedFn.body));
  const prologue = readPrologue(ts, view, emittedFn.body.statements, parameters[0], originalCallable);
  return {prologue, tokens: node => prologue.tokens(node || emittedFn)};
}
module.exports = {compareFunction, readPrologue, comparePrologue, projectCallableRest};
