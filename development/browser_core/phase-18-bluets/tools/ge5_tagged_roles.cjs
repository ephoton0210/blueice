// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original tag, complete site-cache data flow, raw spans and substitution roles.
const assert=require('node:assert/strict'),{create}=require('./ge5_module_contract.cjs');
function lowered(ts,item,node) {
  function unwrap(node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
  assert(ts.isCallExpression(node)&&node.arguments.length===2);assert(ts.isIdentifier(node.expression)&&node.expression.text==='tag');
  const cache=unwrap(node.arguments[0]);assert(ts.isBinaryExpression(cache)&&cache.operatorToken.getText()==='||');
  assert(ts.isIdentifier(cache.left)&&item.slots.has(cache.left.text)&&!item.names.has(cache.left.text));const site=cache.left.text;
  const capture=unwrap(cache.right);assert(ts.isBinaryExpression(capture)&&capture.operatorToken.getText()==='=');assert(ts.isIdentifier(capture.left)&&capture.left.text===site);
  const construct=capture.right;assert(ts.isCallExpression(construct)&&construct.arguments.length===2);
  const callee=construct.expression,member='__makeTemplateObject';
  assert(item.helpers.get(callee.getText())==='template'||ts.isIdentifier(callee)&&callee.text===member&&!item.tagged.namespace||ts.isPropertyAccessExpression(callee)&&callee.name.text===member&&callee.expression.getText()===item.tagged.namespace,'reviewed site constructor');
  const arrays=construct.arguments;for(const array of arrays)assert(ts.isArrayLiteralExpression(array)&&array.elements.length===2&&array.elements.every(ts.isStringLiteral));
  const operand=unwrap(node.arguments[1]);assert(ts.isNumericLiteral(operand));
  return {node,cache,capture,construct,arrays,operand,site};
}
function compare(ts,native,actual,options,originals) {
  const c=create(ts,native,actual,options,originals,'tagged'),{n,a,pair,uniform,boundary,binding,prefix,suffix,finish}=c;
  assert.equal(n.user.length,2);pair(native.tokens(n.user[0]),actual.tokens(a.user[0]),'complete original tag function');
  assert.deepEqual(actual.owner(a.user[0].end),native.owner(n.user[0].end),'original tag function endpoint');
  boundary(native,n.user[0],native.owner(n.user[0].end));boundary(actual,a.user[0],native.owner(n.user[0].end));
  const nb=binding(n,1,'result',true),ab=binding(a,1,'result',true);prefix(nb,ab);
  const start=(view,node)=>node.getStart(view.source);let x,y;
  if(options.target==='ES5') {
    x=lowered(ts,n,nb.expression);y=lowered(ts,a,ab.expression);
    assert.deepEqual(y.arrays.map(array=>array.elements.map(value=>value.text)),x.arrays.map(array=>array.elements.map(value=>value.text)),'complete cooked/raw values');
    pair([native.token(x.node.expression)],[actual.token(y.node.expression)],'original tag callee');
    const [nh,nt]=x.arrays[1].elements,[ah,at]=y.arrays[1].elements;
    pair([native.token(nh)],[actual.token(ah)],'raw template head');pair([native.token(nt)],[actual.token(at)],'raw template tail');
    pair([native.token(x.operand)],[actual.token(y.operand)],'original tagged substitution');
    const entry=native.owner(x.node.expression.end),headEnd=native.owner(nh.end),tailEnd=native.owner(nt.end),operandEnd=native.owner(x.operand.end);assert(entry&&headEnd&&tailEnd&&operandEnd);
    for(const[view,item,head,tail]of [[native,x,nh,nt],[actual,y,ah,at]]) {
      uniform(view,item.node.expression.end,start(view,head),entry,'site cache/cooked/prefix transport');
      uniform(view,head.end,start(view,tail),headEnd,'raw span separator');
      uniform(view,tail.end,start(view,item.operand),tailEnd,'site construction and substitution prefix');
      uniform(view,item.operand.end,item.node.end,operandEnd,'tagged substitution/call close');
      for(let offset=start(view,item.node);offset<item.node.end;offset++)assert(view.covered(offset),'unclassified tagged expression character '+offset);
    }
  } else {
    assert(ts.isTaggedTemplateExpression(nb.expression)&&ts.isTaggedTemplateExpression(ab.expression));
    pair(native.tokens(nb.expression),actual.tokens(ab.expression),'complete retained tagged template');
  }
  for(const[item,profile]of [[n,x],[a,y]]) {
    const allowed=new Set(profile?[profile.site]:[]);if(item.format==='system'&&!item.system.stem)allowed.add('result');
    if(item.format==='system'&&item.tagged.namespace)allowed.add(item.tagged.namespace);
    assert.deepEqual([...item.slots].sort(),[...allowed].sort(),'complete module/site slot inventory');
  }
  suffix(nb,ab);finish();return {native:n,actual:a,binding:ab,bindings:{native:nb,actual:ab},site:{native:x,actual:y}};
}
module.exports={compare,lowered};
