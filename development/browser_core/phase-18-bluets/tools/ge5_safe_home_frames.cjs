// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The original private kind fixes brand storage, dispatcher effects and class evaluation order.
const assert = require('node:assert/strict'), ir = require('./ge5_safe_home_ir.cjs'), pairs = require('./ge5_safe_home_pairs.cjs');
function read(ts, p, options) {
  const lowered = options.target === 'ES2015', staticClass = p.cap.members[0].static, view = p.view, originalNames = new Set(ir.walk(ts,p.cap.original,ts.isIdentifier).map(n=>n.text));
  const fresh = id => {assert(ts.isIdentifier(id) && !originalNames.has(id.text), 'independently fresh generated private binding'); return id.text;};
  function flatten(e) {return ts.isBinaryExpression(e) && e.operatorToken.kind === ts.SyntaxKind.CommaToken ? [...flatten(e.left),...flatten(e.right)] : [e];}
  let parts, inner, declarations, coreBinding, frameCall;
  if (!lowered) {
    assert.equal(p.returned.expression,p.core); assert.equal(p.core.members.filter(ts.isClassStaticBlockDeclaration).length,1);
    assert.equal(p.metadataBody.parent,p.core.members.find(ts.isClassStaticBlockDeclaration));
    parts=[]; declarations=[];
  } else if (!p.blue) {
    parts=flatten(p.returned.expression); const first=parts.shift(); assert(ts.isBinaryExpression(first) && first.operatorToken.kind===ts.SyntaxKind.EqualsToken && first.right===p.core && first.left.text===p.receiver); coreBinding=first;
    const last=parts.pop(); assert(ts.isIdentifier(last) && last.text===p.receiver);
    declarations=p.outer.body.statements.filter(ts.isVariableStatement).filter(s=>!(s.declarationList.flags & ts.NodeFlags.BlockScoped));
    assert.equal(declarations.length,1); assert(declarations[0].declarationList.declarations.every(d=>!d.initializer));
    frameCall=parts.pop(); assert(ts.isCallExpression(frameCall) && !frameCall.arguments.length); const fn=ir.unwrap(ts,frameCall.expression);
    assert(ts.isArrowFunction(fn) && !fn.parameters.length && fn.body===p.metadataBody);
  } else {
    const call=ir.unwrap(ts,p.returned.expression); assert(ts.isCallExpression(call) && !call.arguments.length); inner=ir.unwrap(ts,call.expression); assert(ts.isArrowFunction(inner) && !inner.parameters.length && ts.isBlock(inner.body));
    const statements=[...inner.body.statements], last=statements.pop(); assert(ts.isReturnStatement(last) && ts.isIdentifier(last.expression) && last.expression.text===p.receiver);
    const coreStatements=statements.filter(ts.isVariableStatement).filter(s=>s.declarationList.declarations.some(d=>d.initializer===p.core)); assert.equal(coreStatements.length,1); const cs=coreStatements[0]; assert.equal(cs.declarationList.flags & ts.NodeFlags.BlockScoped,ts.NodeFlags.Const); assert.equal(cs.declarationList.declarations.length,1); assert.equal(cs.declarationList.declarations[0].name.text,p.receiver); coreBinding=cs;
    const ci=statements.indexOf(cs), before=statements.slice(0,ci); declarations=before.filter(s=>s.declarationList.declarations.every(d=>!d.initializer)); assert.equal(declarations.length,1);
    for(const s of before.filter(s=>!declarations.includes(s)))for(const token of view.tokens(s))assert(view.covered(token.start),'only already proved private helper definitions precede class construction');
    parts=statements.slice(ci+1).map(s=>{assert(ts.isExpressionStatement(s));return s.expression;}); frameCall=parts.pop();
    assert(ts.isCallExpression(frameCall) && ts.isPropertyAccessExpression(frameCall.expression) && frameCall.expression.name.text==='call' && frameCall.arguments.length===1 && frameCall.arguments[0].text===p.receiver);
    const fn=ir.unwrap(ts,frameCall.expression.expression); assert(ts.isFunctionExpression(fn) && !fn.name && !fn.parameters.length && fn.body===p.metadataBody);
  }
  const dispatchers=[], slots=new Set(), state=[];
  if(lowered) {
    if(!staticClass){const assignment=parts.shift();assert(ts.isBinaryExpression(assignment) && assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken);fresh(assignment.left);const allocation=assignment.right;assert(ts.isNewExpression(allocation) && allocation.expression.getText(view.source)==='WeakSet' && !allocation.arguments.length);state.push(assignment);p.aliases.set(assignment.left.text,'$class.brand');slots.add(assignment.left.text);}
    const expected=p.cap.members.map(m=>({member:m,kind:m.kind==='setter'?'set':'get'}));
    assert.equal(parts.length,expected.length,'complete private dispatcher allocation inventory');
    for(let index=0;index<parts.length;index++) {
      const expr=parts[index], spec=expected[index]; assert(ts.isBinaryExpression(expr) && expr.operatorToken.kind===ts.SyntaxKind.EqualsToken);fresh(expr.left); const fn=expr.right;
      assert(ts.isFunctionExpression(fn) && fn.name?.text===expr.left.text && !fn.asteriskToken && !fn.modifiers?.length); slots.add(expr.left.text); p.aliases.set(expr.left.text,'$private.'+spec.member.index+'.dispatch');
      dispatchers.push({member:spec.member,node:fn,assignment:expr,kind:spec.kind});
    }
    if(!p.blue)slots.add(p.receiver);
    const names=declarations[0].declarationList.declarations.map(d=>fresh(d.name));assert.deepEqual(names.slice().sort(),[...slots].sort(),'every generated state slot has one independently proved initialization');
  } else {
    const members=p.core.members.filter(m=>(ts.isGetAccessorDeclaration(m)||ts.isSetAccessorDeclaration(m))&&ts.isPrivateIdentifier(m.name));
    assert.equal(members.length,p.cap.members.length);
    for(let index=0;index<members.length;index++){const node=members[index],member=p.cap.members[index],kind=member.kind==='setter'?'set':'get';assert.equal(node.name.text,member.key);assert.equal(ts.isSetAccessorDeclaration(node),kind==='set');assert.equal(!!node.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword),member.static);dispatchers.push({member,node,kind});}
  }
  for(const entry of dispatchers) {
    const fn=entry.node, statements=fn.body.statements; assert.equal(statements.length,1);const statement=statements[0];
    assert(ts.isReturnStatement(statement)||p.blue&&entry.kind==='set'&&ts.isExpressionStatement(statement),'only generated setter dispatch may discard its ignored return');const expression=statement.expression;
    const descriptor=p.entries[entry.member.index].descriptor.text, property=entry.member.kind==='method'?'value':entry.kind;
    if(entry.member.kind==='method'){assert(ts.isPropertyAccessExpression(expression)&&expression.expression.text===descriptor&&expression.name.text==='value');assert(!fn.parameters.length);}
    else {assert(ts.isCallExpression(expression)&&ts.isPropertyAccessExpression(expression.expression)&&expression.expression.name.text==='call');const target=expression.expression.expression;assert(ts.isPropertyAccessExpression(target)&&target.expression.text===descriptor&&target.name.text===property);assert.equal(expression.arguments[0].kind,ts.SyntaxKind.ThisKeyword);assert.equal(expression.arguments.length,entry.kind==='set'?2:1);assert.equal(fn.parameters.length,entry.kind==='set'?1:0);if(entry.kind==='set'){assert(ts.isIdentifier(fn.parameters[0].name)&&expression.arguments[1].text===fn.parameters[0].name.text);}}
  }
  return {lowered,staticClass,parts,inner,declarations,coreBinding,frameCall,dispatchers,state};
}
function compare(ts,n,a,options,contract) {
  const nn=read(ts,n,options),aa=read(ts,a,options),points=[];
  for(let index=0;index<nn.dispatchers.length;index++) {
    const ne=nn.dispatchers[index],ae=aa.dispatchers[index];
    if(ne.kind==='set' && ts.isReturnStatement(ne.node.body.statements[0]) && ts.isExpressionStatement(ae.node.body.statements[0])) {
      const ns=ne.node.body.statements[0],as=ae.node.body.statements[0],ret=n.view.tokens(ns)[0];
      if(nn.lowered)n.view.noSource(ret.start,ret.end,'ignored generated setter dispatcher return');else pairs.role(n.view,ret.start,ret.end,n.view.owner(ns.expression.getStart(n.view.source)),'generated setter return has the exact private-key continuation role');
      contract.pair(pairs.tokens(ts,n,ne.node).filter(t=>t.start!==ret.start),pairs.tokens(ts,a,ae.node),'complete generated setter dispatcher with discarded language-ignored return');
      points.push(...pairs.ends(ts,n.view,ns.expression,a.view,as.expression,'complete generated setter effect'));assert.deepEqual(a.view.owner(as.end),n.view.owner(ns.end));
      points.push(...pairs.ends(ts,n.view,ne.node,a.view,ae.node,'complete generated setter structure',{omitNative:[ns],omitActual:[as]}));
    } else {contract.pair(pairs.tokens(ts,n,ne.node),pairs.tokens(ts,a,ae.node),'complete source-bound private dispatcher');points.push(...pairs.ends(ts,n.view,ne.node,a.view,ae.node,'complete private dispatcher endpoints'));}
    if(nn.lowered){const nt=n.view.tokens(ne.assignment).filter(t=>t.end<=ne.node.getStart(n.view.source)),at=a.view.tokens(ae.assignment).filter(t=>t.end<=ae.node.getStart(a.view.source));contract.pair(nt,at,'exact external private dispatcher slot assignment');}
  }
  if(nn.lowered&&!nn.staticClass){contract.pair(pairs.tokens(ts,n,nn.state[0]),pairs.tokens(ts,a,aa.state[0]),'exact WeakSet brand allocation');points.push(...pairs.ends(ts,n.view,nn.state[0],a.view,aa.state[0],'complete WeakSet brand allocation'));}
  return {native:nn,actual:aa,points};
}
module.exports={read,compare};
