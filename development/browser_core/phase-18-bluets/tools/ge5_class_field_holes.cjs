// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A field initializer is delegated only as an AST-identified, completely proved expression.
const assert=require('node:assert/strict');
const proofs=new WeakSet();
function create(ts,native,actual,n,a,original,pairs=[],receiverCapture=null) {
  const sourceFile=original.getSourceFile(),fields=[...original.members].filter(ts.isPropertyDeclaration),seen=new Set(),items=[];
  const property=node=>ts.isIdentifier(node)||ts.isStringLiteral(node)||ts.isNumericLiteral(node)?node.text:null;
  function staticField(field){return !!field.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.StaticKeyword);}
  function owned(view,item,node,field) {
    assert(node&&typeof node.getStart==='function'&&node.getSourceFile()===view.source,'delegated initializer belongs to the exact emitted syntax tree');
    const name=property(field.name);assert.notEqual(name,null,'closed literal field identity');
    const expression=node,parent=expression.parent;
    if(ts.isPropertyDeclaration(parent)) {
      assert(item.native&&parent.parent===item.node&&parent.initializer===expression,'direct retained class initializer');
      assert.equal(property(parent.name),name,'retained field identity');assert.equal(staticField(parent),staticField(field),'retained field placement');return;
    }
    assert(!staticField(field),'lowered static field requires its own class phase contract');
    let statement,receiver;
    if(ts.isBinaryExpression(parent)&&parent.operatorToken.kind===ts.SyntaxKind.EqualsToken&&parent.right===expression) {
      const target=parent.left;assert(ts.isPropertyAccessExpression(target)||ts.isElementAccessExpression(target),'class field assignment target');
      assert.equal(ts.isPropertyAccessExpression(target)?target.name.text:property(target.argumentExpression),name,'lowered field assignment identity');
      receiver=target.expression;statement=parent.parent;
    }else {
      assert(ts.isPropertyAssignment(parent)&&parent.initializer===expression&&property(parent.name)==='value','field descriptor value expression');
      const descriptor=parent.parent;assert(ts.isObjectLiteralExpression(descriptor));const call=descriptor.parent;
      assert(ts.isCallExpression(call)&&call.expression.getText(view.source)==='Object.defineProperty'&&call.arguments.length===3&&call.arguments[2]===descriptor,'closed field-definition call');
      assert.equal(property(call.arguments[1]),name,'lowered descriptor field identity');receiver=call.arguments[0];statement=call.parent;
    }
    assert(receiver.kind===ts.SyntaxKind.ThisKeyword,'instance field initializer retains its constructor receiver');
    assert(ts.isExpressionStatement(statement),'whole initializer belongs to one direct constructor statement');
    const constructor=item.native?item.node.members.find(ts.isConstructorDeclaration):item.constructor;
    assert(constructor&&statement.parent===constructor.body,'initializer belongs to the exact class constructor');
  }
  for(const pair of pairs) {
    assert(pair&&pair.source&&pair.native&&pair.actual,'typed original/native/actual expression tuple');
    const field=pair.source.parent;assert(ts.isPropertyDeclaration(field)&&field.parent===original&&field.initializer===pair.source&&field.getSourceFile()===sourceFile,'source is this original class field initializer');
    assert(!seen.has(field),'one delegated expression per original field');seen.add(field);
    owned(native,n,pair.native,field);owned(actual,a,pair.actual,field);
    for(const [view,node]of [[native,pair.native],[actual,pair.actual]]) {
      for(let offset=node.getStart(view.source);offset<node.end;offset++)assert(view.covered(offset),'delegated initializer must already prove every UTF-16 character');
    }
    assert.deepEqual(actual.owner(pair.actual.end),native.owner(pair.native.end),'delegated initializer exact logical endpoint');
    items.push({...pair,index:fields.indexOf(field)});
  }
  let capture=null;
  if(receiverCapture) {
    const proof=receiverCapture,item=items.find(item=>item.source===proof.source);assert(item,'constructor receiver phase belongs to one exact proved field initializer');
    assert(ts.isArrowFunction(proof.source),'constructor receiver phase belongs to a source arrow');
    let lexicalThis=false;function receivers(node){if(node!==proof.source&&(ts.isFunctionExpression(node)||ts.isFunctionDeclaration(node)||ts.isClassLike(node)))return;if(node.kind===ts.SyntaxKind.ThisKeyword)lexicalThis=true;ts.forEachChild(node,receivers);}receivers(proof.source);assert(lexicalThis,'the original arrow reads its lexical class receiver');
    assert(!original.members.some(ts.isConstructorDeclaration),'closed implicit-constructor receiver phase');
    const constructor=n.native?n.node.members.find(ts.isConstructorDeclaration):n.constructor,statement=proof.native;
    assert(constructor&&ts.isVariableStatement(statement)&&statement.getSourceFile()===native.source&&statement.parent===constructor.body&&constructor.body.statements[0]===statement,'receiver capture is the exact native implicit constructor entry');
    assert.equal(statement.declarationList.declarations.length,1);const declaration=statement.declarationList.declarations[0];assert(ts.isIdentifier(declaration.name)&&declaration.initializer?.kind===ts.SyntaxKind.ThisKeyword,'receiver capture is one this binding');
    const names=new Set();function identifiers(node){if(ts.isIdentifier(node))names.add(node.text);ts.forEachChild(node,identifiers);}identifiers(sourceFile);assert(!names.has(declaration.name.text),'constructor receiver binding is fresh');
    const uses=[];function references(node){if(ts.isIdentifier(node)&&node.text===declaration.name.text)uses.push(node);ts.forEachChild(node,references);}references(constructor);assert.equal(uses.length,2,'one declaration and one proved receiver use');assert(uses.includes(declaration.name)&&uses.includes(proof.nativeUse));
    const use=proof.nativeUse;assert(use&&use.getSourceFile()===native.source&&item.native.getStart(native.source)<=use.getStart(native.source)&&use.end<=item.native.end,'receiver use belongs to the exact delegated initializer');
    assert(ts.isCallExpression(use.parent)&&use.parent.arguments[0]===use,'receiver is the exact async invocation receiver operand');
    for(let offset=statement.getStart(native.source);offset<statement.end;offset++)assert(native.covered(offset),'receiver phase must already prove every UTF-16 character');
    (native.roleStarts??=new Map()).set(statement.getStart(native.source),native.owner(statement.getStart(native.source)));(native.roleEnds??=new Map()).set(statement.end,native.owner(statement.end));capture=statement;
  }
  function project(view,tokens) {
    const side=view===native?'native':'actual',output=[];
    for(const token of tokens) {
      if(view===native&&capture&&capture.getStart(native.source)<=token.start&&token.end<=capture.end)continue;
      const item=items.find(item=>item[side].getStart(view.source)<=token.start&&token.end<=item[side].end);
      if(!item){output.push(token);continue;}
      if(output.at(-1)?.fieldHole===item)continue;
      const node=item[side],start=node.getStart(view.source);
      assert(tokens.some(token=>token.start===start)&&tokens.some(token=>token.end===node.end),'delegated expression is projected as a complete syntax subtree');
      output.push({text:'$class.field.initializer.'+item.index,start,end:node.end,fieldHole:item});
    }
    return output;
  }
  function paired(nt,at) {
    assert(nt.fieldHole&&at.fieldHole&&nt.fieldHole===at.fieldHole,'the same original field owns both complete expression proofs');
    for(const [view,token]of [[native,nt],[actual,at]]) {
      (view.roleStarts??=new Map()).set(token.start,native.owner(nt.start));(view.roleEnds??=new Map()).set(token.end,native.owner(nt.end));
    }
    (actual.nativeStarts??=new Map()).set(at.start,nt.start);(actual.nativeEnds??=new Map()).set(at.end,nt.end);
  }
  const proof={items,capture,project,paired,native,actual};proofs.add(proof);return proof;
}
function coversComment(proof,view,comment) {
  assert(proofs.has(proof),'exact typed class initializer proof');
  assert(view===proof.native||view===proof.actual,'comment belongs to the proven initializer view');
  assert(view.comments().includes(comment),'exact scanned emitted comment');
  const side=view===proof.native?'native':'actual';
  if(!proof.items.some(item=>item[side].getStart(view.source)<=comment.start&&comment.end<=item[side].end))return false;
  for(let offset=comment.start;offset<comment.end;offset++)assert(view.covered(offset),'delegated initializer comment already proves every UTF-16 unit');
  return true;
}
module.exports={create,coversComment};
