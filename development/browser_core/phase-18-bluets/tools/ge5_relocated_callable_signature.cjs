// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A callback signature may be consumed only after its complete physical roles
// and its source-to-runtime formal pairing have been proved by its wrapper IR.
const assert = require('node:assert/strict');
const start = (view, node) => node.getStart(view.source);
function contains(parent, child) {
  for (let node = child; node; node = node.parent) if (node === parent) return true;
  return false;
}
function runtime(ts, node) {
  return node.parameters.filter(parameter => !(ts.isIdentifier(parameter.name) && parameter.name.text === 'this'));
}
function callable(ts, node) {
  return ts.isFunctionExpression(node) || ts.isFunctionDeclaration(node)
    || ts.isArrowFunction(node) || ts.isMethodDeclaration(node);
}
function verify(ts, native, nm, actual, am, cap) {
  assert.equal(cap?.kind, 'verified-relocated-callable-signature-v1');
  const source = cap.source, sourceFile = source.getSourceFile();
  assert(source.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.AsyncKeyword));
  if (cap.sourceKind === 'variable-async-arrow') {
    assert(ts.isArrowFunction(source) && ts.isVariableDeclaration(source.parent)
      && source.parent.initializer === source
      && ts.isVariableDeclarationList(source.parent.parent)
      && ts.isVariableStatement(source.parent.parent.parent)
      && ts.isSourceFile(source.parent.parent.parent.parent));
  } else if (cap.sourceKind === 'class-field-async-arrow') {
    assert(ts.isArrowFunction(source) && ts.isPropertyDeclaration(source.parent)
      && source.parent.initializer === source
      && (ts.isClassDeclaration(source.parent.parent) || ts.isClassExpression(source.parent.parent)));
  } else if (cap.sourceKind === 'async-class-method') {
    assert(ts.isMethodDeclaration(source) && source.body && !source.asteriskToken
      && (ts.isClassDeclaration(source.parent) || ts.isClassExpression(source.parent)));
  } else {
    assert.equal(cap.sourceKind, 'async-object-method');
    assert(ts.isMethodDeclaration(source) && ts.isObjectLiteralExpression(source.parent));
  }
  assert(nm.originals.includes(sourceFile.text) && am.originals.includes(sourceFile.text));
  assert(cap.nativeCallback === nm.node, 'exact Native callback node');
  assert(cap.actualCallback === am.node, 'exact actual callback node');
  assert(nm.view === native, 'exact Native callback view'); assert(am.view === actual, 'exact actual callback view');
  for (const [view, outer, callback] of [[native, cap.nativeOuter, nm.node], [actual, cap.actualOuter, am.node]]) {
    assert(callable(ts, outer) && outer.body && outer !== callback && contains(outer.body, callback));
    assert(outer.getSourceFile() === view.source, 'outer callable belongs to the exact emitted tree');
    assert(ts.isFunctionExpression(callback) && callback.body && ts.isBlock(callback.body));
    assert(!callback.name && !callback.modifiers?.length && !callback.asteriskToken);
  }
  const sourceParameters = runtime(ts, source), nativeParameters = runtime(ts, cap.nativeOuter);
  assert.equal(nm.node.parameters.length, 0, 'Native callback receives no relocated formals');
  assert.equal(am.node.parameters.length, sourceParameters.length);
  assert.equal(nativeParameters.length, sourceParameters.length);
  assert.equal(cap.formalPairs.length, sourceParameters.length);
  for (let index = 0; index < sourceParameters.length; index++) {
    const original = sourceParameters[index], left = nativeParameters[index], right = am.node.parameters[index];
    assert(cap.formalPairs[index].source === original, 'exact original formal node');
    assert(cap.formalPairs[index].native === left, 'exact Native formal node');
    assert(cap.formalPairs[index].actual === right, 'exact actual relocated formal node');
    for (const parameter of [original, left, right]) {
      assert(ts.isIdentifier(parameter.name) && !parameter.initializer && !parameter.dotDotDotToken,
        'only plain runtime formals use the relocated signature capability');
    }
    assert.equal(left.name.text, original.name.text); assert.equal(right.name.text, original.name.text);
  }
  const ranges = [[start(native, nm.node), start(native, nm.node.body) + 1],
    [start(actual, am.node), start(actual, am.node.body) + 1]];
  assert.deepEqual(cap.ranges, ranges, 'capability covers each exact complete callback header');
  for (const [view, range] of [[native, ranges[0]], [actual, ranges[1]]]) {
    for (let offset = range[0]; offset < range[1]; offset++) {
      assert(view.covered(offset), 'every relocated signature UTF-16 unit was independently verified');
    }
  }
  return { native: ranges[0], actual: ranges[1], trailing: native.owner(ranges[0][1]) };
}
module.exports = { verify };
