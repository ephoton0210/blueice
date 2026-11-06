// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The complete original derived class owns public methods, constructor effects and its core boundary.
const assert = require('node:assert/strict'), ir = require('./ge5_safe_home_ir.cjs'), pairs = require('./ge5_safe_home_pairs.cjs');
function compare(ts,n,a,options,contract,frames,metadata) {
  const points=[];
  const sourcePublic=n.cap.cls.members.filter(m=>!n.cap.members.some(p=>p.node===m));
  for(const original of sourcePublic) {
    const find=p=>{const found=p.core.members.filter(m=>ts.isMethodDeclaration(m)&&m.name.getText(p.view.source)===original.name.getText(n.cap.original));assert.equal(found.length,1);return found[0];},nn=find(n),aa=find(a);
    assert.equal(!!nn.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword),!!original.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword));
    assert.equal(!!aa.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword),!!original.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword));
    if(options.target==='ES2015'&&original.modifiers?.some(m=>m.kind===ts.SyntaxKind.AsyncKeyword)) points.push(...require('./ge5_safe_home_async.cjs').compare(ts,n,nn,a,aa,original,options,contract).points);
    else {contract.pair(pairs.tokens(ts,n,nn),pairs.tokens(ts,a,aa),'complete original public safe-home method');points.push(...pairs.ends(ts,n.view,nn,a.view,aa,'complete original public method endpoints'));}
  }
  const constructors=[];
  for(const p of[n,a]) {
    const list=p.core.members.filter(ts.isConstructorDeclaration);assert.equal(list.length,Number(!n.cap.members[0].static),'complete implicit constructor inventory');
    if(!list.length)continue;const node=list[0];assert(!node.parameters.length);const statements=node.body.statements;assert.equal(statements.length,options.target==='ES2015'?3:2);
    const first=statements[0];assert(ts.isExpressionStatement(first)&&ts.isCallExpression(first.expression)&&first.expression.expression.kind===ts.SyntaxKind.SuperKeyword&&first.expression.arguments.length===1&&ts.isSpreadElement(first.expression.arguments[0])&&first.expression.arguments[0].expression.getText(p.view.source)==='arguments');
    if(options.target==='ES2015'){const branded=statements[1];assert(ts.isExpressionStatement(branded)&&ts.isCallExpression(branded.expression)&&ts.isPropertyAccessExpression(branded.expression.expression)&&branded.expression.expression.name.text==='add'&&p.aliases.get(branded.expression.expression.expression.text)==='$class.brand'&&branded.expression.arguments.length===1&&branded.expression.arguments[0].kind===ts.SyntaxKind.ThisKeyword);}
    const last=statements.at(-1);assert(ts.isExpressionStatement(last)&&ts.isCallExpression(last.expression)&&p.aliases.get(last.expression.expression.getText(p.view.source))==='$helper.runInitializers'&&last.expression.arguments.length===2&&last.expression.arguments[0].kind===ts.SyntaxKind.ThisKeyword&&p.aliases.get(last.expression.arguments[1].text)==='$class.extra');constructors.push({profile:p,node});
  }
  if(constructors.length){const[nn,aa]=constructors;contract.pair(pairs.tokens(ts,n,nn.node),pairs.tokens(ts,a,aa.node),'complete derived constructor brand and extra queue execution');points.push(...pairs.ends(ts,n.view,nn.node,a.view,aa.node,'complete implicit constructor endpoints'));}
  for(const p of[n,a]) {
    const expected=sourcePublic.length+constructors.length/2+(options.target==='ES2015'?0:p.cap.members.length+1);assert.equal(p.core.members.length,expected,'complete emitted class member inventory');
    for(const block of p.core.members.filter(ts.isClassStaticBlockDeclaration)) {
      assert.equal(block.body,p.metadataBody);for(const token of p.view.tokens(block).filter(t=>t.end<=block.body.getStart(p.view.source)+1||t.start===block.end-1))p.view.noSource(token.start,token.end,'generated exact private metadata static phase delimiters');
      assert.deepEqual(p.view.owner(block.end),null,'generated metadata phase complete endpoint');
    }
  }
  const coreTokens=p=>pairs.tokens(ts,p,p.core).filter(t=>!p.view.covered(t.start));contract.pair(coreTokens(n),coreTokens(a),'complete original derived class core header and completion');
  points.push(...pairs.ends(ts,n.view,n.core,a.view,a.core,'complete private-derived class core AST',{omitNative:[...n.core.members],omitActual:[...a.core.members]}));
  // Every outer lexical declaration has an exact source-bound counterpart. The
  // separately parsed state slots and inner class transport are generated.
  const lets=p=>p.outer.body.statements.filter(ts.isVariableStatement).filter(s=>s.declarationList.flags&ts.NodeFlags.Let);
  const nl=lets(n),al=lets(a);assert.equal(nl.length,al.length);
  const wanted=new Set([n.classSuper.name.text,n.entries[0].call.arguments[5].text,...n.entries.flatMap(e=>[e.descriptor.text,e.call.arguments[2].text])]);
  assert.equal(wanted.size,nl.length);for(let index=0;index<nl.length;index++){const ns=nl[index],as=al[index];assert.equal(ns.declarationList.declarations.length,1);assert.equal(as.declarationList.declarations.length,1);const nd=ns.declarationList.declarations[0],ad=as.declarationList.declarations[0];assert(wanted.delete(nd.name.text));assert.equal(n.aliases.get(nd.name.text),a.aliases.get(ad.name.text));if(nd===n.classSuper){assert.equal(ad,a.classSuper);assert.equal(nd.initializer.text,n.cap.base.text);assert.equal(ad.initializer.text,n.cap.base.text);}else if(n.aliases.get(nd.name.text)==='$class.extra'){assert(ts.isArrayLiteralExpression(nd.initializer)&&!nd.initializer.elements.length&&ts.isArrayLiteralExpression(ad.initializer)&&!ad.initializer.elements.length);}else assert(!nd.initializer&&!ad.initializer);contract.pair(pairs.tokens(ts,n,ns),pairs.tokens(ts,a,as),'complete private class lexical slot declaration');points.push(...pairs.ends(ts,n.view,ns,a.view,as,'complete private class lexical declaration endpoints'));}
  assert.equal(wanted.size,0);
  const header=p=>p.view.tokens(p.binding.parent.parent).filter(t=>t.end<=p.outer.body.getStart(p.view.source)+1);
  contract.pair(header(n),header(a),'complete original public class binding and outer evaluation frame header');
  const suffix=p=>p.view.tokens(p.binding.parent.parent).filter(t=>t.start>=p.outer.body.end-1);contract.pair(suffix(n),suffix(a),'complete original class binding evaluation completion');
  assert.deepEqual(a.view.owner(a.binding.end),n.view.owner(n.binding.end),'complete source class public binding endpoint');
  // Parsing above fixes every transport operation and evaluation schedule; no
  // remaining transport token may acquire an original source position.
  for(const p of[n,a])for(const token of p.view.tokens(p.binding.parent.parent))if(!p.view.covered(token.start))p.view.noSource(token.start,token.end,'complete fixed private class construction transport');
  for(const[p,node]of[[n,n.binding.parent.parent],[a,a.binding.parent.parent]]) {
    // region() closes gaps using independently proved neighbouring token roles.
    p.view.roleEnds.set(node.end,p.view.owner(node.end));
  }
  for(const p of[n,a])for(const comment of p.view.comments())if(p.binding.getStart(p.view.source)<=comment.start&&comment.end<=p.binding.end&&comment.text==='/* bluets-class-helper-v1 */')p.view.noSource(comment.start,comment.end,'exact owned private helper generation marker');
  contract.region(n.outerCall,a.outerCall);
  return {source:n.cap.cls,native:{node:n.binding.parent.parent,native:true},actual:{node:a.binding.parent.parent,native:true},points,frames,metadata};
}
module.exports={compare};
