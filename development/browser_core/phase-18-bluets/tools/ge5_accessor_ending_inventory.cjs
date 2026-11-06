// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Enumerate the original accessor callables instead of assuming a fixed end count.
const assert = require('node:assert/strict');
function read(ts, proof) {
  const components = proof.delegated?.components;
  const chain = components?.constructionChain || proof.constructionChain;
  assert(chain?.source && chain.native && chain.actual, 'complete paired original accessor chain');
  const originals = [...chain.source.members];
  assert.equal(originals.length, 2, 'complete original accessor pair');
  assert(ts.isGetAccessorDeclaration(originals[0]) && ts.isSetAccessorDeclaration(originals[1]));
  assert.equal(originals[0].name.text, originals[1].name.text, 'one original accessor key');
  function methods(item) {
    if (!item.native) return item.methods.map(method => ({...method,
      keyNode: method.statement.expression.arguments[1]}));
    return item.node.members.filter(node => ts.isGetAccessorDeclaration(node) || ts.isSetAccessorDeclaration(node))
      .map(fn => {
        const source = originals.filter(member => member.kind === fn.kind && member.name.text === fn.name.text
          && !!member.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.StaticKeyword)
          === !!fn.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.StaticKeyword));
        assert.equal(source.length, 1, 'one exact original accessor callable');
        return {source: source[0], fn, keyNode: fn.name, statement: fn};
      });
  }
  const native = methods(chain.native), actual = methods(chain.actual), points = [...chain.pairs];
  assert.equal(native.length, 2); assert.equal(actual.length, 2);
  function visit(left, right, label) {
    assert.equal(right.kind, left.kind, 'complete accessor callable AST kind');
    if (ts.isIdentifier(left) || ts.isStringLiteral(left) || ts.isNumericLiteral(left)) {
      assert.equal(right.text, left.text, 'complete accessor callable leaf');
    }
    points.push({label, native: left, actual: right});
    const lhs = [], rhs = [];
    ts.forEachChild(left, child => { lhs.push(child); });
    ts.forEachChild(right, child => { rhs.push(child); });
    assert.equal(rhs.length, lhs.length, 'complete accessor callable child inventory');
    for (let index = 0; index < lhs.length; index++) visit(lhs[index], rhs[index], label + '/' + index);
  }
  for (const original of originals) {
    const pick = list => { const found = list.filter(method => method.source === original);
      assert.equal(found.length, 1, 'one emitted callable per original accessor'); return found[0]; };
    const n = pick(native), a = pick(actual), label = ts.SyntaxKind[original.kind];
    points.push({label: label + ' key', native: n.keyNode, actual: a.keyNode});
    visit(n.fn, a.fn, label);
  }
  return {points, methods: actual};
}
module.exports = {read};
