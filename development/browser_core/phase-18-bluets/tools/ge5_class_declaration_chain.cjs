// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A source declaration retains its complete construction chain across emitted wrappers.
const assert=require('node:assert/strict');
function binding(ts,node) {
  if(ts.isVariableStatement(node)){assert.equal(node.declarationList.declarations.length,1);const declaration=node.declarationList.declarations[0];return {declaration,name:declaration.name,initializer:declaration.initializer};}
  if(ts.isVariableDeclaration(node))return {declaration:node,name:node.name,initializer:node.initializer};
  const expression=ts.isExpressionStatement(node)?node.expression:node;
  if(ts.isBinaryExpression(expression)){assert.equal(expression.operatorToken.kind,ts.SyntaxKind.EqualsToken);assert(ts.isIdentifier(expression.left));return {declaration:expression,name:expression.left,initializer:expression.right};}
  assert(ts.isClassDeclaration(node)||ts.isClassExpression(node),'closed emitted declaration construction root');return {name:node.name};
}
function groups(ts,node){const result=[];while(ts.isParenthesizedExpression(node)){result.push(node);node=node.expression;}return result;}
function compare(ts,native,nn,n,actual,an,a) {
  const pairs=[];
  function point(label,nativeNode,actualNode) {
    assert(nativeNode&&actualNode,label+' paired construction nodes');assert.equal(nativeNode.getSourceFile(),native.source);assert.equal(actualNode.getSourceFile(),actual.source);
    assert.deepEqual(actual.owner(actualNode.end),native.owner(nativeNode.end),'exact original class construction logical endpoint: '+label);
    pairs.push({label,native:nativeNode,actual:actualNode});
  }
  const nb=binding(ts,nn),ab=binding(ts,an);point('outer statement',nn,an);point('binding name',nb.name,ab.name);
  assert.equal(!!nb.declaration,!!ab.declaration,'both declaration bindings preserve their wrapper');
  if(nb.declaration) {
    point('binding declaration',nb.declaration,ab.declaration);point('initializer',nb.initializer,ab.initializer);
    const ng=groups(ts,nb.initializer),ag=groups(ts,ab.initializer);
    for(let index=0;index<Math.max(ng.length,ag.length);index++)point('initializer transparent grouping '+index,ng[index]??nb.initializer,ag[index]??ab.initializer);
  }
  assert.equal(n.native,a.native,'same class syntax representation');
  const methods=item=>item.native?item.node.members.filter(node=>ts.isMethodDeclaration(node)||ts.isGetAccessorDeclaration(node)||ts.isSetAccessorDeclaration(node)):item.methods;
  if(n.native) {
    point('inner class',n.node,a.node);
    const nc=n.node.members.find(ts.isConstructorDeclaration),ac=a.node.members.find(ts.isConstructorDeclaration);assert.equal(!!nc,!!ac,'same constructor definition phase');
    if(nc){point('constructor definition',nc,ac);point('constructor body',nc.body,ac.body);}
  }else {
    point('class factory call',n.call,a.call);point('factory function',n.factory,a.factory);point('factory body',n.factory.body,a.factory.body);
    point('constructor definition',n.constructor,a.constructor);point('constructor body',n.constructor.body,a.constructor.body);point('returned constructor',n.returned,a.returned);
    const ng=groups(ts,n.call.expression),ag=groups(ts,a.call.expression);
    for(let index=0;index<Math.max(ng.length,ag.length);index++)point('callee transparent grouping '+index,ng[index]??n.factory,ag[index]??a.factory);
  }
  const nm=methods(n),am=methods(a);assert.equal(nm.length,am.length,'complete declaration method endpoint inventory');
  for(let index=0;index<nm.length;index++) {
    const nf=n.native?nm[index]:nm[index].fn,af=a.native?am[index]:am[index].fn;
    point('method definition '+index,n.native?nf:nm[index].statement,a.native?af:am[index].statement);
    if(!n.native)point('method value '+index,nf,af);
    point('method body '+index,nf.body,af.body);
  }
  return pairs;
}
module.exports={compare};
