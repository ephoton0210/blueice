// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Decode the complete arrow/default wrapper before comparing its physical roles.
const assert = require('node:assert/strict');
function inspect(ts, view, options, original, blue) {
  const source = ts.createSourceFile('original.ts', original, ts.ScriptTarget.Latest, true);
  const binding = source.statements.filter(ts.isVariableStatement).flatMap(node => [...node.declarationList.declarations])
    .find(node => ts.isArrowFunction(node.initializer));
  assert(binding && ts.isIdentifier(binding.name));
  const arrow = binding.initializer;
  assert(arrow.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.AsyncKeyword));
  assert.equal(arrow.parameters.length, 1); assert(arrow.parameters[0].initializer && ts.isBlock(arrow.body));
  const parameter = arrow.parameters[0], names = new Set();
  function collect(node) { if (ts.isIdentifier(node)) names.add(node.text); ts.forEachChild(node, collect); }
  collect(source);
  const text = node => node.getText(view.source), word = node => text(node).replace(/\s/g, '');
  const unwrap = node => { while (ts.isParenthesizedExpression(node)) node = node.expression; return node; };
  function fresh(node, pattern) {
    assert(ts.isIdentifier(node) && !names.has(node.text), 'fresh wrapper identity');
    if (pattern) assert.match(node.text, pattern); return node.text;
  }
  const declarations = view.source.statements.filter(ts.isVariableStatement).flatMap(node => [...node.declarationList.declarations]);
  const declaration = declarations.find(node => node.name.getText() === binding.name.text);
  assert(declaration?.initializer);
  const result = view.source.statements.at(-1);
  const resultCall = ts.isVariableStatement(result) ? result.declarationList.declarations[0].initializer : result.expression.right;
  assert(ts.isCallExpression(resultCall) && text(resultCall.expression) === binding.name.text && resultCall.arguments.length === 0);
  const retained = !['ES5', 'ES2015', 'ES2016'].includes(options.target);
  let fn = unwrap(declaration.initializer), callback, driver = null, factory = null, guard = null, defaultGuard = null;
  let rest = null, entryFunction = null;
  if (retained) {
    assert(ts.isArrowFunction(fn) && fn.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.AsyncKeyword));
    assert.equal(fn.parameters.length, 1); callback = fn;
  } else if (blue) {
    assert(ts.isCallExpression(fn) && fn.arguments.length === 2); factory = unwrap(fn.expression);
    assert(ts.isFunctionExpression(factory) && !factory.name && !factory.asteriskToken && factory.parameters.length === 2);
    const receiver = fresh(factory.parameters[0].name, /^__blueice_target_async_this_\d+$/);
    const arguments_ = fresh(factory.parameters[1].name, /^__blueice_target_async_arguments_\d+$/);
    assert.notEqual(receiver, arguments_);
    assert.equal(fn.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    assert.equal(word(fn.arguments[1]), "typeofarguments==='undefined'?void0:arguments");
    assert.equal(factory.body.statements.length, 1); assert(ts.isReturnStatement(factory.body.statements[0]));
    fn = factory.body.statements[0].expression;
    assert(ts.isFunctionExpression(fn) && !fn.asteriskToken && !fn.modifiers?.length);
    fresh(fn.name, /^__blueice_target_async_callable_\d+$/); entryFunction = fn;
    fn.parameters.forEach(node => { assert(!node.initializer && !node.dotDotDotToken); fresh(node.name, /^__blueice_target_async_argument_\d+$/); });
    const statements = [...fn.body.statements];
    if (options.target !== 'ES5') {
      guard = statements.shift();
      assert.equal(word(guard), 'if(new.target)thrownewTypeError("Asyncfunctionisnotaconstructor");');
    }
    assert.equal(statements.length, 1); assert(ts.isReturnStatement(statements[0]) && ts.isCallExpression(statements[0].expression));
    driver = statements[0].expression;
    assert.match(text(driver.expression), options.target === 'ES5' ? /^__blueice_target_async_driver_\d+$/ : /^__blueice_target_async_\d+$/);
    assert.equal(driver.arguments.length, 3); assert.equal(driver.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    assert.equal(text(driver.arguments[1]), 'arguments'); callback = driver.arguments[2];
    assert(ts.isFunctionExpression(callback) && Boolean(callback.asteriskToken) === (options.target !== 'ES5'));
  } else {
    if (options.target === 'ES5') {
      assert(ts.isFunctionExpression(fn) && !fn.name && !fn.asteriskToken && fn.parameters.length === 0);
      assert.equal(fn.body.statements.length, 3);
      const declaration = fn.body.statements[0]; assert(ts.isVariableStatement(declaration));
      assert.equal(declaration.declarationList.declarations.length, 1);
      const item = declaration.declarationList.declarations[0], name = fresh(item.name);
      assert(ts.isArrayLiteralExpression(item.initializer) && item.initializer.elements.length === 0);
      const loop = fn.body.statements[1]; assert(ts.isForStatement(loop));
      assert(ts.isVariableDeclarationList(loop.initializer) && loop.initializer.declarations.length === 1);
      const index = fresh(loop.initializer.declarations[0].name); assert.equal(text(loop.initializer.declarations[0].initializer), '0');
      assert.equal(word(loop.condition), index + '<arguments.length'); assert.equal(word(loop.incrementor), index + '++');
      assert(ts.isBlock(loop.statement) && loop.statement.statements.length === 1);
      assert.equal(word(loop.statement.statements[0]), name + '[' + index + ']=arguments[' + index + '];');
      assert(ts.isReturnStatement(fn.body.statements[2])); driver = fn.body.statements[2].expression;
      rest = { declaration, loop, name, index };
    } else {
      assert(ts.isArrowFunction(fn) && !fn.modifiers?.length && fn.parameters.length === 1);
      assert(fn.parameters[0].dotDotDotToken && !fn.parameters[0].initializer);
      rest = { name: fresh(fn.parameters[0].name) }; driver = fn.body;
    }
    assert(ts.isCallExpression(driver)); assert.match(text(driver.expression), /^__awaiter(?:_\d+)?$/);
    assert.equal(driver.arguments.length, 4); assert.equal(word(driver.arguments[0]), 'void0'); assert.equal(word(driver.arguments[2]), 'void0');
    if (options.target === 'ES5') assert.equal(word(driver.arguments[1]), '__spreadArray([],__read(' + rest.name + '),false)');
    else assert.equal(word(driver.arguments[1]), '[...' + rest.name + ']');
    callback = driver.arguments[3]; assert(ts.isFunctionExpression(callback));
    assert.equal(Boolean(callback.asteriskToken), options.target !== 'ES5');
  }
  assert.equal(callback.parameters.length, 1);
  assert.equal(text(callback.parameters[0].name), parameter.name.getText(source));
  let defaultValue, awaitCall;
  if (options.target === 'ES5') {
    if (!blue) {
      assert.equal(callback.body.statements.length, 3);
      defaultGuard = callback.body.statements[1]; assert(ts.isIfStatement(defaultGuard) && !defaultGuard.elseStatement);
      assert.equal(word(defaultGuard.expression), text(callback.parameters[0].name) + '===void0');
      assert(ts.isBlock(defaultGuard.thenStatement) && defaultGuard.thenStatement.statements.length === 1);
      const assignment = defaultGuard.thenStatement.statements[0].expression;
      assert(ts.isBinaryExpression(assignment) && assignment.operatorToken.kind === ts.SyntaxKind.EqualsToken);
      assert.equal(text(assignment.left), parameter.name.getText(source)); defaultValue = assignment.right;
    } else {
      const assignments = [];
      function visit(node) {
        if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken && text(node.left) === parameter.name.getText(source)) assignments.push(node);
        ts.forEachChild(node, visit);
      }
      visit(callback.body); assert.equal(assignments.length, 1); defaultValue = unwrap(assignments[0].right);
    }
  } else defaultValue = callback.parameters[0].initializer;
  assert(defaultValue && text(defaultValue) === parameter.initializer.getText(source), 'complete default expression identity');
  const awaited = [];
  function findCalls(node) {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
      && text(node.expression.expression) === 'Promise' && node.expression.name.text === 'resolve') awaited.push(node);
    ts.forEachChild(node, findCalls);
  }
  findCalls(callback.body); assert.equal(awaited.length, 1); awaitCall = awaited[0];
  return { source, arrow, parameter, binding, declaration, fn, entryFunction, callback, driver, factory, rest, guard,
    defaultGuard, defaultValue, awaitCall, resultCall, retained, blue };
}
module.exports = { inspect };
