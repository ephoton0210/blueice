// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A closed, already-verified original default guard may precede the core machine.
const assert=require('node:assert/strict');
function interval(ts,view,machine){const capability=machine.reviewedPrologue;assert.equal(capability?.kind,'verified-original-default-parameter-v1');assert.equal(machine.blue,false);
  const {statement,parameter,range}=capability;assert(ts.isIfStatement(statement)&&statement.parent===machine.node.body&&!statement.elseStatement);
  assert(machine.node.body.statements.includes(statement)&&statement.end<machine.returned.getStart(view.source));
  assert(ts.isParameter(parameter)&&parameter.initializer&&machine.originals.includes(parameter.getSourceFile().text));assert(ts.isIdentifier(parameter.name));
  assert.equal(machine.node.parameters.length,1);assert.equal(machine.node.parameters[0].name.getText(view.source),parameter.name.text);
  const predicate=statement.expression;assert(ts.isBinaryExpression(predicate)&&predicate.operatorToken.kind===ts.SyntaxKind.EqualsEqualsEqualsToken&&ts.isIdentifier(predicate.left)&&predicate.left.text===parameter.name.text&&ts.isVoidExpression(predicate.right)&&ts.isNumericLiteral(predicate.right.expression)&&predicate.right.expression.text==='0');
  assert(ts.isBlock(statement.thenStatement)&&statement.thenStatement.statements.length===1);const effect=statement.thenStatement.statements[0];assert(ts.isExpressionStatement(effect)&&ts.isBinaryExpression(effect.expression)&&effect.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(effect.expression.left)&&effect.expression.left.text===parameter.name.text);
  assert.equal(effect.expression.right.getText(view.source),parameter.initializer.getText(parameter.getSourceFile()));
  assert.deepEqual(range,[statement.getStart(view.source),statement.end],'the capability is the exact original IfStatement interval');
  for(let offset=range[0];offset<range[1];offset++)assert(view.covered(offset),'every prologue UTF-16 unit was independently verified');
  return range;
}
module.exports={interval};
