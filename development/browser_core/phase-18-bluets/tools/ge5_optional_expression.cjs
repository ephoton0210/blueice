// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Normalize the reviewed optional/nullish ABIs and retain every physical role.
const assert=require('node:assert/strict');
const {compareTokens}=require('./ge5_role_stream.cjs');
function read(ts,item,node,blue) {
  const groups=[];
  function unwrap(node,into=groups) {while(ts.isParenthesizedExpression(node)){into.push(node);node=node.expression;}return node;}
  function binary(node,operator) {assert(ts.isBinaryExpression(node)&&node.operatorToken.getText()===operator,'reviewed '+operator+' operation');return node;}
  function identifier(node,wanted) {assert(ts.isIdentifier(node));if(wanted!==undefined)assert.equal(node.text,wanted);return node.text;}
  function zero(node) {assert(ts.isNumericLiteral(node)&&node.text==='0');}
  function missing(node) {assert(ts.isVoidExpression(node));zero(node.expression);}
  function nullGuard(node,operator,right) {const result=binary(node,operator);if(right==='null')assert.equal(result.right.kind,ts.SyntaxKind.NullKeyword);else missing(result.right);return result.left;}
  function slot(node) {const name=identifier(node);assert(item.slots.has(name),'declared continuation/module slot');return name;}
  function optionalNative(node) {
    assert(ts.isConditionalExpression(node));const predicate=binary(node.condition,'||');
    const first=nullGuard(predicate.left,'===','null'),second=nullGuard(predicate.right,'===','undefined');missing(node.whenTrue);
    const member=node.whenFalse;assert(ts.isPropertyAccessExpression(member));
    const name=identifier(first);identifier(second,name);identifier(member.expression,name);
    return {node,receiver:first,receivers:[first,second,member.expression],key:member.name,name:member.name.text,kind:'native'};
  }
  function optionalBlue(node) {
    const optionalGroups=[],root=unwrap(node,optionalGroups);
    if(ts.isCallExpression(root)) {
      assert.equal(item.helpers.get(root.expression.getText()),'optional');assert.equal(root.arguments.length,2);
      const [receiver,key]=root.arguments;identifier(receiver);assert(ts.isStringLiteral(key));
      return {node,root,receiver,key,name:key.text,kind:'helper',groups:optionalGroups};
    }
    const comma=binary(root,','),capture=binary(comma.left,'='),receiverGroups=[];
    const receiverSlot=slot(capture.left),receiver=unwrap(capture.right,receiverGroups);identifier(receiver);
    const conditional=comma.right;assert(ts.isConditionalExpression(conditional));const predicate=binary(conditional.condition,'||');
    identifier(nullGuard(predicate.left,'===','null'),receiverSlot);identifier(nullGuard(predicate.right,'===','undefined'),receiverSlot);missing(conditional.whenTrue);
    const member=conditional.whenFalse;assert(ts.isElementAccessExpression(member));identifier(member.expression,receiverSlot);assert(ts.isStringLiteral(member.argumentExpression));
    return {node,root,receiver,key:member.argumentExpression,name:member.argumentExpression.text,kind:'inline',groups:optionalGroups,receiverGroups,
      references:[predicate.left.left,predicate.right.left,member.expression]};
  }
  const root=unwrap(node);let capture,conditional,optional,fallback,slotName,fallbackGroups=[];
  if(blue) {
    const comma=binary(root,',');capture=binary(comma.left,'=');slotName=slot(capture.left);optional=optionalBlue(capture.right);
    conditional=comma.right;assert(ts.isConditionalExpression(conditional));
    if(ts.isCallExpression(conditional.condition)) {
      assert.equal(item.helpers.get(conditional.condition.expression.getText()),'nullish');assert.equal(conditional.condition.arguments.length,1);identifier(conditional.condition.arguments[0],slotName);
    } else {
      const predicate=binary(conditional.condition,'||');identifier(nullGuard(predicate.left,'===','null'),slotName);identifier(nullGuard(predicate.right,'===','undefined'),slotName);
    }
    identifier(conditional.whenFalse,slotName);fallback=unwrap(conditional.whenTrue,fallbackGroups);
  } else {
    conditional=root;assert(ts.isConditionalExpression(conditional));const predicate=binary(conditional.condition,'&&');
    capture=binary(unwrap(nullGuard(predicate.left,'!==','null')),'=');slotName=slot(capture.left);
    identifier(nullGuard(predicate.right,'!==','undefined'),slotName);identifier(conditional.whenTrue,slotName);
    optional=optionalNative(capture.right);fallback=conditional.whenFalse;
  }
  zero(fallback);
  return {node,root,groups,capture,conditional,optional,fallback,fallbackGroups,slotName};
}
function compare(ts,native,n,actual,a,cover,uniform) {
  const x=read(ts,n.item,n.node,false),y=read(ts,a.item,a.node,true);
  const start=(view,node)=>node.getStart(view.source);
  const entry=native.owner(start(native,x.optional.receiver)),receiverEnd=native.owner(x.optional.receiver.end),memberEnd=native.owner(x.optional.key.end),fallbackEnd=native.owner(x.fallback.end);
  assert(entry&&receiverEnd&&memberEnd&&fallbackEnd);
  assert.equal(y.optional.name,x.optional.name);
  compareTokens(native,[native.token(x.optional.receiver)],actual,[actual.token(y.optional.receiver)],'optional original receiver');
  for(const node of x.optional.receivers.slice(1))compareTokens(native,[native.token(x.optional.receiver)],native,[native.token(node)],'Native repeated receiver');
  compareTokens(native,[native.token(x.fallback)],actual,[actual.token(y.fallback)],'nullish fallback');
  if(y.optional.kind==='helper') {
    uniform(actual,start(actual,y.optional.root),start(actual,y.optional.receiver),entry,'optional helper call prefix');
    uniform(actual,y.optional.receiver.end,start(actual,y.optional.key),receiverEnd,'optional key separator');
    uniform(actual,y.optional.key.end,y.optional.root.end,memberEnd,'optional helper call close');
  } else {
    uniform(actual,start(actual,y.optional.root),start(actual,y.optional.receiver),entry,'optional receiver capture prefix');
    for(const reference of y.optional.references)uniform(actual,start(actual,reference),reference.end,entry,'captured optional receiver');
    const spans=[y.optional.receiver,...y.optional.references,y.optional.key].sort((a,b)=>start(actual,a)-start(actual,b));
    let cursor=y.optional.receiver.end;
    for(const node of spans) {
      if(start(actual,node)<cursor)continue;
      uniform(actual,cursor,start(actual,node),receiverEnd,'optional generated guard');cursor=node.end;
    }
    uniform(actual,y.optional.key.end,y.optional.root.end,memberEnd,'optional element close');
    for(const group of y.optional.groups) {
      uniform(actual,start(actual,group),start(actual,group)+1,entry,'optional grouping open');
      uniform(actual,group.end-1,group.end,memberEnd,'optional grouping close');
    }
  }
  const keyOwners=native.token(x.optional.key).owners;assert(keyOwners.every(owner=>JSON.stringify(owner)===JSON.stringify(keyOwners[0])),'uniform Native property role');
  uniform(actual,start(actual,y.optional.key),y.optional.key.end,keyOwners[0],'derived optional property');
  native.cover(start(native,x.optional.key),x.optional.key.end);
  // Every remaining Native character has one specific region role. Receiver
  // leaves carry their own owners; generated tests borrow the receiver end.
  let cursor=start(native,x.optional.node);
  for(const leaf of [...x.optional.receivers,x.optional.key].sort((a,b)=>start(native,a)-start(native,b))) {
    uniform(native,cursor,start(native,leaf),receiverEnd,'Native optional guard');native.cover(start(native,leaf),leaf.end);cursor=leaf.end;
  }
  uniform(native,cursor,x.optional.node.end,memberEnd,'Native optional close');
  uniform(native,start(native,x.node),start(native,x.optional.node),entry,'Native coalesce capture prefix');
  uniform(native,x.optional.node.end,start(native,x.fallback),memberEnd,'Native coalesce tests and captured branch');
  uniform(native,x.fallback.end,x.node.end,fallbackEnd,'Native coalesce endpoint');
  uniform(actual,start(actual,y.node),start(actual,y.optional.node),entry,'coalesce capture prefix');
  uniform(actual,y.optional.node.end,start(actual,y.fallback),memberEnd,'coalesce tests and fallback prefix');
  const afterFallback=y.fallbackGroups[0]?.end??y.fallback.end;
  uniform(actual,y.fallback.end,afterFallback,fallbackEnd,'generated fallback close');
  const coalesceClose=y.groups.length?y.groups.at(-1).end-1:y.node.end;
  uniform(actual,afterFallback,coalesceClose,memberEnd,'coalesce captured branch');
  for(const group of y.groups)uniform(actual,group.end-1,group.end,fallbackEnd,'coalesce grouping endpoint');
  for(const[view,node]of [[native,x.node],[actual,y.node]])for(let offset=start(view,node);offset<node.end;offset++)assert(view.covered(offset),'unclassified optional expression character at '+offset);
  cover(native,x.node.end,x.node.end,fallbackEnd);cover(actual,y.node.end,y.node.end,fallbackEnd);
  return {native:x,actual:y};
}
module.exports={read,compare};
