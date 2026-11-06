// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exact transport syntax leaves source identities to the owned source model.
const assert = require('node:assert/strict');
function create(ts, view) {
  function tokens(node, aliases = new Map()) {
    return view.tokens(node).map(token => {
      const scanner = ts.createScanner(ts.ScriptTarget.Latest, true, ts.LanguageVariant.Standard, token.text);
      return scanner.scan() === ts.SyntaxKind.StringLiteral
        ? JSON.stringify(scanner.getTokenValue()) : aliases.get(token.text) || token.text;
    });
  }
  function words(code) {
    const anonymous = /^function\s*\(/.test(code);
    const source = ts.createSourceFile('module-pattern.js', anonymous ? '(' + code + ')' : code,
      ts.ScriptTarget.Latest, true, ts.ScriptKind.JS), result = [];
    assert.equal(source.parseDiagnostics.length, 0, 'complete module pattern parses');
    function visit(node) {
      if (ts.isToken(node)) {
        if (node.kind !== ts.SyntaxKind.EndOfFileToken) result.push(ts.isStringLiteral(node)
          ? JSON.stringify(node.text) : node.getText(source));
      } else for (const child of node.getChildren(source)) visit(child);
    }
    visit(anonymous ? source.statements[0].expression.expression : source);
    return result;
  }
  function exact(node, code, aliases) {
    assert.deepEqual(tokens(node, aliases), words(code), 'complete module transport syntax');
  }
  function plainFunction(node, parameters) {
    assert(ts.isFunctionExpression(node) && !node.name && !node.asteriskToken && !node.modifiers?.length);
    assert.deepEqual(node.parameters.map(parameter => {
      assert(ts.isIdentifier(parameter.name) && !parameter.initializer && !parameter.dotDotDotToken
        && !parameter.questionToken && !parameter.type && !parameter.modifiers?.length);
      return parameter.name.text;
    }), parameters, 'complete module parameter inventory');
    return node;
  }
  return {tokens, exact, plainFunction};
}
module.exports = {create};
