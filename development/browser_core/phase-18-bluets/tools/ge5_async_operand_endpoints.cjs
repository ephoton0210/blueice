// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exact source await operands retain their completion through generated captures.
const assert=require('node:assert/strict');
function compare(ts,native,n,actual,a,options){const source=n.originalMethod,original=source.getSourceFile(),operands=[];function sourceNodes(node){if(ts.isAwaitExpression(node)){assert(ts.isCallExpression(node.expression),'closed original awaited call operand');operands.push(node.expression);}ts.forEachChild(node,sourceNodes);}sourceNodes(source.body);assert.equal(operands.length,1,'complete original method await inventory');const results=[];
 for(const operand of operands){const callee=operand.expression.getText(original),argumentsText=operand.arguments.map(node=>node.getText(original));function emitted(view,item){const found=[];function visit(node){if(ts.isCallExpression(node)&&node.expression.getText(view.source)===callee&&node.arguments.length===argumentsText.length&&node.arguments.every((argument,index)=>argument.getText(view.source)===argumentsText[index]))found.push(node);ts.forEachChild(node,visit);}visit(item.callback);assert.equal(found.length,1,'exact independently proved original await call');return found[0];}const nc=emitted(native,n),ac=emitted(actual,a),ending=native.owner(nc.end);assert.deepEqual(actual.owner(ac.end),ending,'complete original async await operand endpoint');
  if(options.target==='ES5'){const assignment=ac.parent,statement=assignment.parent;assert(ts.isBinaryExpression(assignment)&&assignment.right===ac&&assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isExpressionStatement(statement),'exact independently proved async captured call');for(let offset=ac.end;offset<statement.end;offset++){assert.deepEqual(actual.owner(offset),ending,'complete async captured operand completion suffix');actual.cover(offset,offset+1);}}
  results.push({source:operand,native:nc,actual:ac});
 }
 return results;
}
module.exports={compare};
