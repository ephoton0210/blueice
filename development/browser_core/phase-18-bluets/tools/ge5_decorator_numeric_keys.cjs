// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Only a decorated original numeric method authorizes a numeric helper key.
const assert = require('node:assert/strict');
function read(ts, view, originals, calls) {
  const classes = originals.flatMap(text => ts.createSourceFile('original.ts', text, 99, true)
    .statements.filter(node => ts.isClassDeclaration(node) && node.name
      && node.members.some(member => ts.isMethodDeclaration(member) && ts.isNumericLiteral(member.name)
        && ts.getDecorators(member)?.length)));
  if (!classes.length) return [];
  const references = [];
  for (const {node, kind} of calls) {
    if (kind !== 'decorate' || node.arguments.length !== 4 || !ts.isNumericLiteral(node.arguments[2])) continue;
    const receiver = node.arguments[1], key = node.arguments[2];
    const isStatic = ts.isIdentifier(receiver);
    assert(isStatic || (ts.isPropertyAccessExpression(receiver) && receiver.name.text === 'prototype'
      && ts.isIdentifier(receiver.expression)), 'numeric method decorator original receiver');
    const name = isStatic ? receiver.text : receiver.expression.text;
    const owners = classes.filter(klass => klass.name.text === name);
    assert.equal(owners.length, 1, 'numeric method decorator unique original class');
    const methods = owners[0].members.filter(member => ts.isMethodDeclaration(member)
      && ts.isNumericLiteral(member.name) && member.name.text === key.text
      && !!member.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.StaticKeyword) === isStatic
      && ts.getDecorators(member)?.length);
    assert.equal(methods.length, 1, 'numeric helper key belongs to one decorated original method');
    assert.equal(node.arguments[3].kind, ts.SyntaxKind.NullKeyword, 'numeric method descriptor acquisition');
    references.push({start: key.getStart(view.source), end: key.end,
      text: '$original.numeric.method.' + name + '.' + Number(isStatic) + '.' + key.text,
      numericMethodKey: true});
  }
  return references;
}
module.exports = {read};
