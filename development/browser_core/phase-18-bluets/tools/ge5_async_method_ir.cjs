// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Decode complete method/receiver/parameter forwarding before comparing maps.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path');
const envelope = require('./ge5_module_envelope.cjs');
function inspect(ts, view, options, original, blue) {
  const source = ts.createSourceFile('original.ts', original, ts.ScriptTarget.Latest, true);
  const declarations = source.statements.filter(ts.isVariableStatement).flatMap(statement => [...statement.declarationList.declarations]);
  const originalBox = declarations.find(node => node.name.getText(source) === 'box');
  assert(originalBox && ts.isObjectLiteralExpression(originalBox.initializer));
  const originalMethod = originalBox.initializer.properties.find(node => ts.isMethodDeclaration(node));
  assert(originalMethod && originalMethod.name.getText(source) === 'read' && originalMethod.body);
  assert.equal(originalBox.initializer.properties.length, 2);
  assert.equal(originalMethod.modifiers?.length, 1);
  assert.equal(originalMethod.modifiers[0].kind, ts.SyntaxKind.AsyncKeyword);
  const parameters = originalMethod.parameters.filter(parameter => parameter.name.getText(source) !== 'this');
  assert.equal(parameters.length, 1); assert(ts.isIdentifier(parameters[0].name));
  assert(!parameters[0].initializer && !parameters[0].dotDotDotToken);
  const identifiers = new Set();
  function collect(node) { if (ts.isIdentifier(node)) identifiers.add(node.text); ts.forEachChild(node, collect); }
  collect(source);
  const env = envelope.read(ts, view, options, {blue, originals: [source]});
  const text = node => node.getText(view.source), word = node => text(node).replace(/\s/g, '');
  const unwrap = node => { while (ts.isParenthesizedExpression(node)) node = node.expression; return node; };
  const generatedNames = new Set();
  function fresh(node, prefix) {
    assert(ts.isIdentifier(node) && !identifiers.has(node.text) && !generatedNames.has(node.text), 'fresh method transport binding');
    assert.match(node.text, prefix); generatedNames.add(node.text); return node.text;
  }
  const boxStatement = env.sourceNodes.find(node => ts.isVariableStatement(node)
    && node.declarationList.declarations.some(declaration => declaration.name.getText(view.source) === 'box'));
  assert(boxStatement && boxStatement.declarationList.declarations.length === 1);
  const box = boxStatement.declarationList.declarations[0]; assert(ts.isObjectLiteralExpression(box.initializer));
  assert.equal(box.initializer.properties.length, originalBox.initializer.properties.length);
  const [value, member] = box.initializer.properties;
  assert(ts.isPropertyAssignment(value) && text(value.name) === 'value' && ts.isNumericLiteral(value.initializer));
  assert.equal(text(value.initializer), originalBox.initializer.properties[0].initializer.getText(source));
  assert.equal(text(member.name), originalMethod.name.getText(source));
  let method;
  if (options.target === 'ES5') {
    assert(ts.isPropertyAssignment(member) && ts.isFunctionExpression(member.initializer));
    method = member.initializer; assert(!method.name && !method.asteriskToken && !method.modifiers?.length);
  } else { assert(ts.isMethodDeclaration(member)); method = member; }
  assert(method.body && method.parameters.length === parameters.length);
  const retained = !['ES5', 'ES2015', 'ES2016'].includes(options.target);
  const result = env.sourceNodes.at(-1);
  const resultCall = ts.isVariableStatement(result)
    ? result.declarationList.declarations[0].initializer : result.expression.right;
  assert(ts.isCallExpression(resultCall) && ts.isPropertyAccessExpression(resultCall.expression));
  assert.equal(text(resultCall.expression.expression), 'box'); assert.equal(text(resultCall.expression.name), 'read');
  assert.equal(resultCall.arguments.length, 1); assert.equal(word(resultCall.arguments[0]), '2');
  let entry = method, returned = null, apply = null, driver = null, callback = null, guard = null;
  if (retained) {
    assert.equal(method.modifiers?.length, 1); assert.equal(method.modifiers[0].kind, ts.SyntaxKind.AsyncKeyword);
    assert.equal(text(method.parameters[0].name), parameters[0].name.text); callback = method;
  } else {
    assert(!method.modifiers?.length && !method.asteriskToken && method.body.statements.length === 1);
    returned = method.body.statements[0]; assert(ts.isReturnStatement(returned));
    if (blue) {
      const parameter = method.parameters[0]; fresh(parameter.name, /^__blueice_target_async_parameter_\d+$/);
      assert(!parameter.initializer && !parameter.dotDotDotToken && !parameter.type);
      apply = returned.expression;
      assert(ts.isCallExpression(apply) && ts.isPropertyAccessExpression(apply.expression)
        && !apply.questionDotToken && !apply.expression.questionDotToken && apply.expression.name.text === 'apply');
      assert.equal(apply.arguments.length, 2); assert.equal(apply.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
      assert.equal(text(apply.arguments[1]), 'arguments');
      entry = unwrap(apply.expression.expression);
      assert(ts.isFunctionExpression(entry) && !entry.asteriskToken && !entry.modifiers?.length);
      fresh(entry.name, /^__blueice_target_async_callable_\d+$/);
      assert.equal(entry.parameters.length, 1);
      fresh(entry.parameters[0].name, /^__blueice_target_async_argument_\d+$/);
      assert(!entry.parameters[0].initializer && !entry.parameters[0].dotDotDotToken && !entry.parameters[0].type);
      const body = [...entry.body.statements];
      if (options.target !== 'ES5') {
        guard = body.shift();
        env.exact(guard, 'if(new.target)throw new TypeError("Async function is not a constructor");');
      }
      assert.equal(body.length, 1); assert(ts.isReturnStatement(body[0])); returned = body[0];
    } else assert.equal(text(method.parameters[0].name), parameters[0].name.text);
    driver = returned.expression; assert(ts.isCallExpression(driver) && ts.isIdentifier(driver.expression));
    assert.match(driver.expression.text, blue
      ? options.target === 'ES5' ? /^__blueice_target_async_driver_\d+$/ : /^__blueice_target_async_\d+$/
      : /^__awaiter(?:_\d+)?$/);
    assert.equal(driver.arguments.length, blue ? 3 : 4); assert.equal(driver.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    if (blue) assert.equal(text(driver.arguments[1]), 'arguments');
    else for (const node of driver.arguments.slice(1, 3)) assert.equal(word(node), 'void0');
    callback = driver.arguments.at(-1);
    assert(ts.isFunctionExpression(callback) && !callback.name && !callback.modifiers?.length);
    assert.equal(Boolean(callback.asteriskToken), options.target !== 'ES5');
    assert.equal(callback.parameters.length, blue ? 1 : 0);
    if (blue) assert.equal(text(callback.parameters[0].name), parameters[0].name.text);
  }
  const helpers = [], generated = [];
  for (const node of env.sourceNodes) {
    if (node === boxStatement || node === result) continue;
    if (ts.isFunctionDeclaration(node)) { assert(blue && !retained); helpers.push(node); }
    else if (ts.isVariableStatement(node)) { assert(!blue && !retained); helpers.push(node); }
    else if (ts.isExpressionStatement(node)) {
      if (ts.isStringLiteral(node.expression)) env.exact(node, '"use strict";');
      else if (ts.isCallExpression(node.expression)) env.exact(node, 'Object.defineProperty(exports,"__esModule",{value:true});');
      else env.exact(node, 'exports.result=void 0;');
    } else assert.fail('unreviewed method module statement');
    view.noSource(node.getStart(view.source), node.end, 'complete generated method module statement'); generated.push(node);
  }
  assert.equal(helpers.length, retained ? 0 : options.target === 'ES5' ? blue ? 4 : 2 : 1);
  if (!retained && blue) {
    const generators = helpers.filter(node => /^__blueice_target_(?:generator|delegate_iterator|delegate_step)_\d+$/.test(node.name?.text));
    if (options.target === 'ES5') require('./ge5_binding_helpers.cjs').validate(ts, view, generators, identifiers);
    else assert.equal(generators.length, 0);
    const async = helpers.filter(node => !generators.includes(node)); assert.equal(async.length, 1);
    assert.equal(text(driver.expression), async[0].name.text);
    env.exact(async[0], fs.readFileSync(path.resolve(__dirname, '../../../../backend/bluets/src/emitter/targets/async_helpers.v1.js'), 'utf8')
      .replaceAll('__blueice_target_async', async[0].name.text));
  } else if (helpers.length) {
    assert.deepEqual(helpers.map(node => node.declarationList.declarations[0].name.text).sort(),
      (options.target === 'ES5' ? ['__awaiter', '__generator'] : ['__awaiter']).sort());
  }
  return {source, originalBox, originalMethod, parameters, env, boxStatement, box, value, member, method, result, resultCall,
    retained, blue, entry, returned, apply, driver, callback, guard, helpers, generated, identifiers};
}
module.exports = {inspect};
