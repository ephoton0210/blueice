// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The exact original derived method owns each helper, receiver and super lookup.
const assert = require('node:assert/strict'), classes = require('./ge5_class_roles.cjs');
function descendants(ts, node, predicate) {
  const result = []; function visit(node) {if (predicate(node)) result.push(node); ts.forEachChild(node, visit);} visit(node); return result;
}
function read(ts, view, options, source, blue, env) {
  const text = node => node.getText(view.source), word = node => text(node).replace(/\s/g, '');
  const originalClass = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'Derived'); assert(originalClass);
  assert.equal(originalClass.heritageClauses.length, 1); assert.equal(originalClass.heritageClauses[0].types.length, 1);
  assert.equal(originalClass.heritageClauses[0].types[0].expression.getText(source), 'Base');
  const originalMethod = originalClass.members.find(node => ts.isMethodDeclaration(node) && node.name.getText(source) === 'readAsync');
  assert(originalMethod?.body && !originalMethod.asteriskToken && originalMethod.parameters.length === 0);
  assert.equal(originalMethod.modifiers?.length, 1); assert.equal(originalMethod.modifiers[0].kind, ts.SyntaxKind.AsyncKeyword);
  const parameters = [...originalMethod.parameters], identifiers = new Set();
  for (const node of descendants(ts, source, ts.isIdentifier)) identifiers.add(node.text);
  const nodes = env.sourceNodes.filter(node => ts.isClassDeclaration(node) && node.name?.text === 'Derived'
    || ts.isVariableStatement(node) && node.declarationList.declarations.some(node => node.name.getText(view.source) === 'Derived' && node.initializer));
  assert.equal(nodes.length, 1); const classNode = nodes[0], klass = classes.readClass(ts, view, classNode, originalClass);
  const method = klass.native ? klass.node.members.find(node => ts.isMethodDeclaration(node) && node.name.getText(view.source) === 'readAsync')
    : klass.methods.find(node => (ts.isStringLiteral(node.key) ? node.key.text : node.key.getText(view.source)) === 'readAsync')?.fn;
  assert(method?.body && method.parameters.length === 0 && !method.asteriskToken);
  const retained = !['ES5', 'ES2015', 'ES2016'].includes(options.target);
  let entry = method, returned = null, apply = null, driver = null, callback = method, guard = null, bridge = null;
  function fresh(node, pattern) {assert(ts.isIdentifier(node) && !identifiers.has(node.text), 'fresh async-super transport binding'); assert.match(node.text, pattern);}
  if (retained) {
    assert.equal(method.modifiers?.length, 1); assert.equal(method.modifiers[0].kind, ts.SyntaxKind.AsyncKeyword);
  } else {
    assert(!method.modifiers?.length); const statements = [...method.body.statements];
    if (options.target !== 'ES5') {
      bridge = statements.shift(); assert(ts.isVariableStatement(bridge) && bridge.declarationList.declarations.length === 1);
      const declaration = bridge.declarationList.declarations[0]; fresh(declaration.name, blue ? /^__blueice_target_async_super_\d+$/ : /^_super(?:_\d+)?$/);
      if (blue) {
        const arrow = declaration.initializer; assert(ts.isArrowFunction(arrow) && !arrow.modifiers?.length && arrow.parameters.length === 1);
        const parameter = arrow.parameters[0]; assert(ts.isIdentifier(parameter.name) && !parameter.initializer && !parameter.dotDotDotToken && !identifiers.has(parameter.name.text));
        assert(ts.isElementAccessExpression(arrow.body) && arrow.body.expression.kind === ts.SyntaxKind.SuperKeyword && text(arrow.body.argumentExpression) === parameter.name.text);
        env.exact(bridge, `var ${declaration.name.text}=(${parameter.name.text})=>super[${parameter.name.text}];`);
      } else env.exact(bridge, `const ${declaration.name.text}=Object.create(null,{read:{get:()=>super.read}});`);
    }
    assert.equal(statements.length, 1); returned = statements[0]; assert(ts.isReturnStatement(returned));
    if (blue) {
      apply = returned.expression; assert(ts.isCallExpression(apply) && ts.isPropertyAccessExpression(apply.expression) && apply.expression.name.text === 'apply');
      assert.equal(apply.arguments.length, 2); assert.equal(apply.arguments[0].kind, ts.SyntaxKind.ThisKeyword); assert.equal(text(apply.arguments[1]), 'arguments');
      entry = apply.expression.expression; while (ts.isParenthesizedExpression(entry)) entry = entry.expression;
      assert(ts.isFunctionExpression(entry) && !entry.asteriskToken && !entry.modifiers?.length && entry.parameters.length === 0);
      fresh(entry.name, /^__blueice_target_async_callable_\d+$/);
      const statements = [...entry.body.statements];
      if (options.target !== 'ES5') {guard = statements.shift(); env.exact(guard, 'if(new.target)throw new TypeError("Async function is not a constructor");');}
      assert.equal(statements.length, 1); returned = statements[0]; assert(ts.isReturnStatement(returned));
    }
    driver = returned.expression; assert(ts.isCallExpression(driver) && ts.isIdentifier(driver.expression));
    assert.match(driver.expression.text, blue ? options.target === 'ES5' ? /^__blueice_target_async_driver_\d+$/ : /^__blueice_target_async_\d+$/ : /^__awaiter(?:_\d+)?$/);
    assert.equal(driver.arguments.length, blue ? 3 : 4); assert.equal(driver.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    if (blue) assert.equal(text(driver.arguments[1]), 'arguments'); else for (const arg of driver.arguments.slice(1, 3)) assert.equal(word(arg), 'void0');
    callback = driver.arguments.at(-1); assert(ts.isFunctionExpression(callback) && !callback.name && !callback.modifiers?.length && callback.parameters.length === 0);
    assert.equal(Boolean(callback.asteriskToken), options.target !== 'ES5');
  }
  const sourceStatements = originalMethod.body.statements; assert.equal(sourceStatements.length, 2);
  assert(ts.isVariableStatement(sourceStatements[0]) && sourceStatements[0].declarationList.declarations.length === 1);
  const originalExtra = sourceStatements[0].declarationList.declarations[0]; assert(ts.isIdentifier(originalExtra.name) && originalExtra.name.text === 'extra' && ts.isAwaitExpression(originalExtra.initializer));
  const originalReturn = sourceStatements[1]; assert(ts.isReturnStatement(originalReturn) && ts.isBinaryExpression(originalReturn.expression));
  assert.equal(originalReturn.expression.operatorToken.kind, ts.SyntaxKind.PlusToken); assert.equal(originalReturn.expression.right.getText(source), 'extra');
  const originalSuperCall = originalReturn.expression.left; assert(ts.isCallExpression(originalSuperCall) && originalSuperCall.arguments.length === 0);
  assert(ts.isPropertyAccessExpression(originalSuperCall.expression) && originalSuperCall.expression.expression.kind === ts.SyntaxKind.SuperKeyword && originalSuperCall.expression.name.text === 'read');
  const awaited = descendants(ts, callback.body, node => ts.isCallExpression(node) && node.expression.getText(view.source) === 'Promise.resolve');
  assert.equal(awaited.length, 1); const awaitCall = awaited[0]; assert.equal(awaitCall.arguments.length, 1); assert.equal(text(awaitCall.arguments[0]), '2');
  const superCalls = descendants(ts, callback.body, node => ts.isCallExpression(node) && (retained
    ? ts.isPropertyAccessExpression(node.expression) && node.expression.expression.kind === ts.SyntaxKind.SuperKeyword
    : ts.isPropertyAccessExpression(node.expression) && node.expression.name.text === 'call'));
  assert.equal(superCalls.length, 1); const superCall = superCalls[0], superMember = retained ? superCall.expression : superCall.expression.expression;
  if (retained) assert.equal(text(superCall.expression), 'super.read');
  else {
    assert.equal(superCall.arguments.length, 1); assert.equal(superCall.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    if (options.target === 'ES5') {
      if (blue) {
        assert(ts.isCallExpression(superMember) && text(superMember.expression) === 'Reflect.get' && superMember.arguments.length === 3);
        assert.equal(text(superMember.arguments[0]), klass.base + '.prototype'); assert(ts.isStringLiteral(superMember.arguments[1]) && superMember.arguments[1].text === 'read');
        assert.equal(superMember.arguments[2].kind, ts.SyntaxKind.ThisKeyword);
      } else assert.equal(text(superMember), klass.base + '.prototype.read');
    } else {
      const binding = bridge.declarationList.declarations[0].name.text;
      if (blue) {assert(ts.isCallExpression(superMember) && ts.isIdentifier(superMember.expression) && superMember.expression.text === binding); assert.equal(superMember.arguments.length, 1); assert(ts.isStringLiteral(superMember.arguments[0]) && superMember.arguments[0].text === 'read');}
      else assert.equal(text(superMember), binding + '.read');
      const references = descendants(ts, method.body, node => ts.isIdentifier(node) && node.text === binding); assert.equal(references.length, 2, 'one exact super bridge declaration and lookup');
    }
  }
  const results = env.sourceNodes.filter(node => ts.isVariableStatement(node) && node.declarationList.declarations.some(node => node.name.getText(view.source) === 'result')
    || ts.isExpressionStatement(node) && ts.isBinaryExpression(node.expression) && node.expression.left.getText(view.source) === 'exports.result' && ts.isCallExpression(node.expression.right));
  assert.equal(results.length, 1); const result = results[0], resultCall = ts.isVariableStatement(result) ? result.declarationList.declarations[0].initializer : result.expression.right;
  assert(ts.isCallExpression(resultCall) && ts.isPropertyAccessExpression(resultCall.expression) && resultCall.expression.name.text === 'readAsync' && resultCall.arguments.length === 0);
  assert(ts.isNewExpression(resultCall.expression.expression) && text(resultCall.expression.expression.expression) === 'Derived');
  return {source, originalClass, originalMethod, parameters, identifiers, classNode, klass, method, retained, blue, env,
    entry, returned, apply, driver, callback, guard, bridge, awaitCall, superCall, superMember, originalSuperCall, originalExtra, result, resultCall};
}
module.exports = {read, descendants};
