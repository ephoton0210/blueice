// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A field store is compared through its complete source-bound assignment or descriptor ABI.
const assert=require('node:assert/strict'),{compareGap}=require('./ge5_role_stream.cjs');
function unwrap(ts,node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function literal(ts,node){if(ts.isIdentifier(node)||ts.isStringLiteral(node)||ts.isNumericLiteral(node))return node.text;assert.fail('literal field key required');}
function access(ts,node){if(ts.isPropertyAccessExpression(node))return {receiver:node.expression,key:node.name.text};if(ts.isElementAccessExpression(node))return {receiver:node.expression,key:literal(ts,node.argumentExpression)};return null;}
function readStore(ts,view,node){
  const expression=ts.isExpressionStatement(node)?node.expression:node;
  if(ts.isPropertyDeclaration(expression)){assert(expression.initializer);return {node,expression,key:literal(ts,expression.name),value:expression.initializer,retained:true,static:!!expression.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword)};}
  if(ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken){const property=access(ts,expression.left);if(property)return {node,expression,...property,value:expression.right,assignment:expression};}
  if(ts.isCallExpression(expression)&&expression.expression.getText(view.source)==='Object.defineProperty'){
    assert.equal(expression.arguments.length,3);const [receiver,key,descriptor]=expression.arguments;assert(ts.isObjectLiteralExpression(descriptor));
    assert.deepEqual(descriptor.properties.map(p=>p.name?.getText(view.source)),['enumerable','configurable','writable','value'],'complete field descriptor property inventory');
    assert(descriptor.properties.every(ts.isPropertyAssignment));for(const property of descriptor.properties.slice(0,3))assert.equal(property.initializer.kind,ts.SyntaxKind.TrueKeyword);
    return {node,expression,receiver,key:literal(ts,key),value:descriptor.properties[3].initializer,descriptor};
  }
  return null;
}
function compareStore(ts,native,nn,actual,an,field,{nativeAliases=new Map(),actualAliases=new Map(),initializerProof=null}={}){
  const n=readStore(ts,native,nn),a=readStore(ts,actual,an);assert(n&&a,'complete paired field stores');
  const key=literal(ts,field.name);assert.equal(n.key,key);assert.equal(a.key,key);assert.equal(!!a.descriptor,!!n.descriptor);assert.equal(!!a.retained,!!n.retained,'same target field representation');
  const pairs=[];
  function mark(view,start,end,owner){for(let at=start;at<end;at++)assert.deepEqual(view.owner(at),owner,'complete source-bound field transport UTF-16');view.cover(start,end);(view.roleStarts??=new Map()).set(start,owner);(view.roleEnds??=new Map()).set(end,owner);}
  function wrapper(view,store){
    const value=unwrap(ts,store.value);if(!ts.isCallExpression(value)||!ts.isPropertyAccessExpression(value.expression)||value.expression.name.text!=='call')return store.value;
    const fn=unwrap(ts,value.expression.expression);assert(ts.isFunctionExpression(fn)&&!fn.name&&!fn.asteriskToken&&!fn.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.AsyncKeyword)&&!fn.parameters.length&&fn.body.statements.length===1&&ts.isReturnStatement(fn.body.statements[0]),'complete static receiver-binding wrapper');
    assert.equal(value.arguments.length,1);assert(store.receiver);assert.equal(value.arguments[0].getText(view.source),store.receiver.getText(view.source),'the static initializer retains its class receiver');return fn.body.statements[0].expression;
  }
  const nv=wrapper(native,n),av=wrapper(actual,a);
  function units(view,node,aliases){return view.tokens(node).map(token=>{let text=aliases.get(token.text);if(!text)return token;const owner=token.owners[0];assert(token.owners.every(x=>JSON.stringify(x)===JSON.stringify(owner)),'complete generated field binding UTF-16');return {...token,text,owners:[owner]};});}
  function pair(wanted,observed,label){assert.equal(observed.length,wanted.length,label+' token inventory');for(let index=0;index<wanted.length;index++){const x=wanted[index],y=observed[index];assert.equal(y.text,x.text,label+' ordered token '+index);assert.deepEqual(y.owners,x.owners,label+' complete UTF-16 token '+index+' '+x.text);native.cover(x.start,x.end);actual.cover(y.start,y.end);(native.roleStarts??=new Map()).set(x.start,native.owner(x.start));(actual.roleStarts??=new Map()).set(y.start,native.owner(x.start));(native.roleEnds??=new Map()).set(x.end,native.owner(x.end));(actual.roleEnds??=new Map()).set(y.end,native.owner(x.end));(actual.nativeStarts??=new Map()).set(y.start,x.start);(actual.nativeEnds??=new Map()).set(y.end,x.end);if(index)compareGap(native,wanted[index-1].end,x.start,actual,observed[index-1].end,y.start,label+' ordered physical gap');pairs.push([x,y]);}}
  if(initializerProof){assert(require('./ge5_static_super_initializers.cjs').owns(initializerProof,ts,native,nv,actual,av,field),'exact complete static initializer capability');for(const[view,node]of[[native,nv],[actual,av]])for(let at=node.getStart(view.source);at<node.end;at++)assert(view.covered(at),'initializer capability requires complete prior UTF-16 coverage');assert.deepEqual(actual.owner(av.end),native.owner(nv.end),'delegated static initializer exact endpoint');}
  else{const ni=units(native,nv,nativeAliases),ai=units(actual,av,actualAliases);pair(ni,ai,'complete field initializer '+key);}
  // The receiver-binding call completes the field; its returned initializer
  // completes an earlier payload role, before the generated wrapper suffix.
  assert.deepEqual(actual.owner(a.value.end),native.owner(n.value.end),'complete field value endpoint '+key);
  const prefixOwner=native.owner(nv.getStart(native.source)),suffixOwner=native.owner(native.tokens(nv).at(-1).start);
  for(const [view,store,value]of[[native,n,nv],[actual,a,av]])if(value!==store.value){assert.deepEqual(view.owner(value.end),suffixOwner,'complete wrapped initializer payload endpoint '+key);mark(view,store.value.getStart(view.source),value.getStart(view.source),prefixOwner);mark(view,value.end,store.value.end,suffixOwner);}
  const nt=units(native,n.expression,nativeAliases),at=units(actual,a.expression,actualAliases);
  const np=nt.filter(t=>t.end<=n.value.getStart(native.source)),ap=at.filter(t=>t.end<=a.value.getStart(actual.source));
  if(n.retained)pair(np,ap,'complete retained field prefix '+key);
  else if(n.descriptor){const keys=(view,store,tokens)=>tokens.map(token=>{const node=store.expression.arguments[1];if(token.start!==node.getStart(view.source)||token.end!==node.end)return token;const owner=token.owners[0];assert(token.owners.every(x=>JSON.stringify(x)===JSON.stringify(owner)),'complete field key reference UTF-16');return {...token,text:'$field.key.'+key,owners:[owner]};});pair(keys(native,n,np),keys(actual,a,ap),'complete field descriptor prefix '+key);}
  else{
    assert(n.assignment&&a.assignment);const nameOwner=native.owner(n.assignment.left.getStart(native.source));
    for(const [view,store]of[[native,n],[actual,a]])mark(view,store.assignment.left.getStart(view.source),store.assignment.left.end,nameOwner);
    pair([native.token(n.assignment.operatorToken)],[actual.token(a.assignment.operatorToken)],'complete field assignment operator '+key);
    compareGap(native,n.assignment.left.end,n.assignment.operatorToken.getStart(native.source),actual,a.assignment.left.end,a.assignment.operatorToken.getStart(actual.source),'field key to assignment gap');
  }
  if(np.length&&ap.length)compareGap(native,np.at(-1).end,n.value.getStart(native.source),actual,ap.at(-1).end,a.value.getStart(actual.source),'complete field prefix to initializer gap '+key);
  const ns=nt.filter(t=>t.start>=n.value.end),as=at.filter(t=>t.start>=a.value.end);pair(ns,as,'complete field store completion '+key);
  if(ns.length&&as.length)compareGap(native,n.value.end,ns[0].start,actual,a.value.end,as[0].start,'complete field initializer to completion gap '+key);
  assert.deepEqual(actual.owner(a.expression.end),native.owner(n.expression.end),'complete field expression endpoint '+key);
  if(ts.isExpressionStatement(nn)&&ts.isExpressionStatement(an)){const nc=native.tokens(nn).filter(t=>t.start>=n.expression.end),ac=actual.tokens(an).filter(t=>t.start>=a.expression.end);if(nc.length&&ac.length)compareGap(native,n.expression.end,nc[0].start,actual,a.expression.end,ac[0].start,'complete field expression to statement gap '+key);pair(native.tokens(nn).filter(t=>t.start>=n.expression.end),actual.tokens(an).filter(t=>t.start>=a.expression.end),'complete field statement terminator '+key);assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete field statement endpoint '+key);}
  (actual.nativeStarts??=new Map()).set(an.getStart(actual.source),nn.getStart(native.source));(actual.nativeEnds??=new Map()).set(an.end,nn.end);
  return {field,native:n,actual:a,pairs};
}
module.exports={unwrap,literal,access,readStore,compareStore};
