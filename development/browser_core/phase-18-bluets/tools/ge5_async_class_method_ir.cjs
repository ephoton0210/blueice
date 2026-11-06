// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete class identity and dynamic method/awaiter/argument forwarding.
const assert = require('node:assert/strict');
const classes = require('./ge5_class_roles.cjs');
function read(ts, view, options, source, blue, env) {
  const originalClass = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'Box'); assert(originalClass);
  const originalMethod = originalClass.members.find(node => ts.isMethodDeclaration(node) && node.name.getText(source) === 'read');
  assert(originalMethod?.body && originalMethod.modifiers?.length === 1 && originalMethod.modifiers[0].kind === ts.SyntaxKind.AsyncKeyword);
  assert.equal(originalMethod.typeParameters?.length ?? 0, 0);
  const parameters = originalMethod.parameters.filter(parameter => parameter.name.getText(source) !== 'this');
  assert.equal(parameters.length, 1); assert(ts.isIdentifier(parameters[0].name) && !parameters[0].initializer && !parameters[0].dotDotDotToken);
  const originalFields = originalClass.members.filter(ts.isPropertyDeclaration); assert.equal(originalFields.length, 1); assert.equal(originalFields[0].name.getText(source), 'value');
  const identifiers = new Set(); function names(node) {if (ts.isIdentifier(node)) identifiers.add(node.text); ts.forEachChild(node, names);} names(source);
  const nodes = env.sourceNodes.filter(node => ts.isClassDeclaration(node) && node.name?.text === 'Box'
    || ts.isVariableStatement(node) && node.declarationList.declarations.some(node => node.name.getText(view.source) === 'Box' && node.initializer));
  assert.equal(nodes.length, 1); const classNode = nodes[0], klass = classes.readClass(ts, view, classNode, originalClass);
  const method = klass.native ? klass.node.members.find(node => ts.isMethodDeclaration(node) && node.name.getText(view.source) === 'read')
    : klass.methods.find(node => (ts.isStringLiteral(node.key) ? node.key.text : node.key.getText(view.source)) === 'read')?.fn;
  assert(method?.body && method.parameters.length === 1);
  const value = require('./ge5_lexical_arrow_read.cjs').field(ts, view, 'value'); assert.equal(value.getText(view.source), originalFields[0].initializer.getText(source));
  const text = node => node.getText(view.source), word = node => text(node).replace(/\s/g, '');
  function fresh(node, pattern) {assert(ts.isIdentifier(node) && !identifiers.has(node.text), 'fresh class-method transport binding'); assert.match(node.text, pattern);}
  const retained = !['ES5', 'ES2015', 'ES2016'].includes(options.target);
  let entry = method, returned = null, apply = null, driver = null, callback = method, guard = null;
  if (retained) {
    assert.equal(method.modifiers?.length, 1); assert.equal(method.modifiers[0].kind, ts.SyntaxKind.AsyncKeyword);
    assert.equal(text(method.parameters[0].name), parameters[0].name.text);
  } else {
    assert(!method.modifiers?.length && !method.asteriskToken && method.body.statements.length === 1);
    returned = method.body.statements[0]; assert(ts.isReturnStatement(returned));
    if (blue) {
      fresh(method.parameters[0].name, /^__blueice_target_async_parameter_\d+$/);
      assert(!method.parameters[0].initializer && !method.parameters[0].dotDotDotToken);
      apply = returned.expression;
      assert(ts.isCallExpression(apply) && ts.isPropertyAccessExpression(apply.expression) && apply.expression.name.text === 'apply'
        && !apply.questionDotToken && !apply.expression.questionDotToken);
      assert.equal(apply.arguments.length, 2); assert.equal(apply.arguments[0].kind, ts.SyntaxKind.ThisKeyword); assert.equal(text(apply.arguments[1]), 'arguments');
      entry = apply.expression.expression; while (ts.isParenthesizedExpression(entry)) entry = entry.expression;
      assert(ts.isFunctionExpression(entry) && !entry.asteriskToken && !entry.modifiers?.length);
      fresh(entry.name, /^__blueice_target_async_callable_\d+$/); assert.equal(entry.parameters.length, 1);
      fresh(entry.parameters[0].name, /^__blueice_target_async_argument_\d+$/);
      assert(!entry.parameters[0].initializer && !entry.parameters[0].dotDotDotToken);
      const statements = [...entry.body.statements];
      if (options.target !== 'ES5') {guard = statements.shift(); env.exact(guard, 'if(new.target)throw new TypeError("Async function is not a constructor");');}
      assert.equal(statements.length, 1); returned = statements[0]; assert(ts.isReturnStatement(returned));
    } else assert.equal(text(method.parameters[0].name), parameters[0].name.text);
    driver = returned.expression; assert(ts.isCallExpression(driver) && ts.isIdentifier(driver.expression));
    assert.match(driver.expression.text, blue ? options.target === 'ES5' ? /^__blueice_target_async_driver_\d+$/ : /^__blueice_target_async_\d+$/ : /^__awaiter(?:_\d+)?$/);
    assert.equal(driver.arguments.length, blue ? 3 : 4); assert.equal(driver.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    if (blue) assert.equal(text(driver.arguments[1]), 'arguments'); else for (const node of driver.arguments.slice(1, 3)) assert.equal(word(node), 'void0');
    callback = driver.arguments.at(-1); assert(ts.isFunctionExpression(callback) && !callback.name && !callback.modifiers?.length);
    assert.equal(Boolean(callback.asteriskToken), options.target !== 'ES5'); assert.equal(callback.parameters.length, blue ? 1 : 0);
    if (blue) assert.equal(text(callback.parameters[0].name), parameters[0].name.text);
  }
  const results = env.sourceNodes.filter(node => ts.isVariableStatement(node)
    && node.declarationList.declarations.some(node => node.name.getText(view.source) === 'result')
    || ts.isExpressionStatement(node) && ts.isBinaryExpression(node.expression) && node.expression.left.getText(view.source) === 'exports.result' && ts.isCallExpression(node.expression.right));
  assert.equal(results.length, 1); const result = results[0], resultCall = ts.isVariableStatement(result) ? result.declarationList.declarations[0].initializer : result.expression.right;
  assert(ts.isCallExpression(resultCall) && ts.isPropertyAccessExpression(resultCall.expression) && resultCall.expression.name.text === 'read');
  const constructed = resultCall.expression.expression; assert(ts.isNewExpression(constructed) && constructed.expression.getText(view.source) === 'Box' && constructed.arguments.length === 0);
  assert.equal(resultCall.arguments.length, 1); assert.equal(text(resultCall.arguments[0]), '2');
  const calls = []; function find(node) {if (ts.isCallExpression(node) && node.expression.getText(view.source) === 'Promise.resolve') calls.push(node); ts.forEachChild(node, find);} find(callback.body); assert.equal(calls.length, 1);
  return {source, originalClass, originalMethod, parameters, identifiers, classNode, klass, method, value, result, resultCall,
    retained, blue, entry, returned, apply, driver, callback, guard, awaitCall: calls[0], env};
}
module.exports = {read};
