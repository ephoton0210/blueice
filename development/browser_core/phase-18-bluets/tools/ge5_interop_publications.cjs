// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Only original exported functions and variables justify publication scaffolds.
const assert = require('node:assert/strict');

function read(ts, model, blue, consumed) {
  const {module, item} = model, view = item.view, system = item.format === 'system';
  const preserved = item.body.some(ts.isImportDeclaration) || item.body.some(node =>
    node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword));
  function generated(node, label) { view.noSource(node.getStart(view.source), node.end, label); consumed.add(node); }
  function propertyName(node) {
    if (ts.isPropertyAccessExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'exports') return node.name.text;
    if (ts.isElementAccessExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'exports'
      && ts.isStringLiteral(node.argumentExpression)) return node.argumentExpression.text;
    return null;
  }
  const publications = [];
  if (!preserved) for (const edge of module.exports.filter(edge => ts.isFunctionDeclaration(edge.node))) {
    const matches = item.sourceNodes.filter(node => {
      if (consumed.has(node) || !ts.isExpressionStatement(node)) return false;
      const expression = node.expression;
      if (system && !blue) return ts.isCallExpression(expression) && expression.expression.getText(view.source) === item.system.publisher
        && expression.arguments.length === 2 && ts.isStringLiteral(expression.arguments[0]) && expression.arguments[0].text === edge.exported;
      return ts.isBinaryExpression(expression) && propertyName(expression.left) === edge.exported;
    });
    assert.equal(matches.length, 1, 'one complete publication per original exported function');
    const node = matches[0];
    item.exact(node, system ? blue ? `exports[${JSON.stringify(edge.exported)}]=${edge.local};`
      : `${item.system.publisher}(${JSON.stringify(edge.exported)},${edge.local});` : `exports.${edge.exported}=${edge.local};`);
    if (system) generated(node, 'complete generated System function publication');
    else {
      const position = module.source.getLineAndCharacterOfPosition(edge.node.getStart(module.source));
      const owner = [0, position.line, position.character];
      for (let offset = node.getStart(view.source); offset < node.end; offset++) {
        assert.deepEqual(view.owner(offset), owner, 'complete original function publication phase');
      }
      const end = module.source.getLineAndCharacterOfPosition(edge.node.end), tail = [0, end.line, end.character];
      assert.deepEqual(view.owner(node.end), tail, 'exact original function publication end');
      view.cover(node.getStart(view.source), node.end); view.roleStarts.set(node.getStart(view.source), owner);
      view.roleEnds.set(node.end, tail); consumed.add(node);
    }
    publications.push({edge, node});
  }
  const strict = [], markers = [], predeclarations = [];
  for (const node of item.sourceNodes) {
    if (consumed.has(node) || !ts.isExpressionStatement(node)) continue;
    const expression = node.expression;
    if (ts.isStringLiteral(expression)) {
      item.exact(node, `'use strict';`); strict.push(node); generated(node, 'complete module strict directive');
    } else if (!system && ts.isCallExpression(expression) && expression.expression.getText(view.source) === 'Object.defineProperty') {
      item.exact(node, 'Object.defineProperty(exports,"__esModule",{value:true});');
      markers.push(node); generated(node, 'complete generated module identity marker');
    } else if (!system && ts.isBinaryExpression(expression)) {
      const names = []; let value = expression;
      while (ts.isBinaryExpression(value) && value.operatorToken.kind === ts.SyntaxKind.EqualsToken && propertyName(value.left)) {
        names.push(propertyName(value.left)); value = value.right;
      }
      if (ts.isVoidExpression(value)) {
        assert(ts.isNumericLiteral(value.expression) && value.expression.text === '0');
        predeclarations.push(...names); generated(node, 'complete generated module variable predeclarations');
      }
    }
  }
  assert.equal(strict.length, preserved || system && blue ? 0 : 1, 'complete module directive inventory');
  assert.equal(markers.length, preserved || system ? 0 : 1, 'complete module identity marker inventory');
  const variables = module.exports.filter(edge => ts.isVariableDeclaration(edge.node)).map(edge => edge.exported);
  assert.deepEqual(predeclarations.slice().sort(), preserved || system ? [] : variables.slice().sort(),
    'complete original exported variable predeclaration inventory');
  return {publications, consumed};
}

module.exports = {read};
