// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Compare original leaves plus every transport region in array/call spread.
const assert=require('node:assert/strict'),{create}=require('./ge5_module_contract.cjs');
function compare(ts,native,actual,options,originals) {
  const c=create(ts,native,actual,options,originals,'spread'),{n,a,pair,uniform,cover,boundary,binding,prefix,suffix,finish}=c;
  assert.equal(n.user.length,4);assert.equal(options.downlevelIteration,false);
  const start=(view,node)=>node.getStart(view.source);
  function unwrap(node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
  function helper(item,node,kind) {
    const wanted=['array','append'].includes(kind)?'__spreadArray':null;
    if(item.helpers.get(node.getText())===kind)return true;
    return wanted&&(ts.isIdentifier(node)&&node.text===wanted&&!item.spread.namespace||ts.isPropertyAccessExpression(node)&&node.name.text===wanted&&node.expression.getText()===item.spread.namespace);
  }
  function append(item,node,blue) {
    const root=unwrap(node);assert(ts.isCallExpression(root)&&helper(item,root.expression,blue?'array':'append'),'reviewed append helper');
    const args=root.arguments;assert.equal(args.length,blue&&item.helpers.get(root.expression.getText())==='array'?4:3);
    assert(ts.isArrayLiteralExpression(args[0])&&args[0].elements.length===0);assert.equal(args[2].kind,ts.SyntaxKind.TrueKeyword);
    if(args.length===4)assert.equal(args[3].kind,ts.SyntaxKind.FalseKeyword);
    const leaf=unwrap(args[1]);assert(ts.isIdentifier(leaf)&&leaf.text==='values');return {root,leaf};
  }
  for(let index=0;index<2;index++) {
    pair(native.tokens(n.user[index]),actual.tokens(a.user[index]),'complete original spread statement '+index);
    assert.deepEqual(actual.owner(a.user[index].end),native.owner(n.user[index].end),'original spread statement endpoint');
    boundary(native,n.user[index],native.owner(n.user[index].end));boundary(actual,a.user[index],native.owner(n.user[index].end));
  }
  const nc=binding(n,2,'copied'),ac=binding(a,2,'copied');prefix(nc,ac);
  const nb=binding(n,3,'result',true),ab=binding(a,3,'result',true);prefix(nb,ab);
  const lowered=options.target==='ES5';
  let copied,call;
  if(!lowered) {
    pair(native.tokens(nc.expression),actual.tokens(ac.expression),'retained array spread');
    pair(native.tokens(nb.expression),actual.tokens(ab.expression),'retained call spread');
  } else {
    const x=append(n,nc.expression,false),y=append(a,ac.expression,true);copied={native:x,actual:y};
    const entry=native.owner(start(native,x.root)),end=native.owner(x.leaf.end);assert(entry&&end);
    pair([native.token(x.leaf)],[actual.token(y.leaf)],'array spread operand');
    uniform(native,start(native,x.root),start(native,x.leaf),entry,'Native array helper prefix');
    uniform(native,x.leaf.end,x.root.end,end,'Native array helper close');
    uniform(actual,start(actual,y.root),start(actual,y.leaf),entry,'array helper prefix');
    uniform(actual,y.leaf.end,y.root.end,end,'array helper close');
    const nx=nb.expression,ay=ab.expression;assert(ts.isBinaryExpression(nx)&&ts.isBinaryExpression(ay));assert.equal(nx.operatorToken.getText(),'+');assert.equal(ay.operatorToken.getText(),'+');
    const nativeCall=nx.left,actualCall=ay.left;assert(ts.isCallExpression(nativeCall)&&ts.isCallExpression(actualCall));
    assert(ts.isPropertyAccessExpression(nativeCall.expression)&&nativeCall.expression.name.text==='apply');const callee=nativeCall.expression.expression;assert(ts.isIdentifier(callee)&&callee.text==='total');
    assert.equal(nativeCall.arguments.length,2);assert.equal(nativeCall.arguments[0].getText(),'void 0');const argument=nativeCall.arguments[1];assert(ts.isIdentifier(argument)&&argument.text==='values');
    let functionTransport=null;
    if(!helper(a,actualCall.expression,'call')) {
      functionTransport=actualCall.expression;const fn=unwrap(functionTransport);assert(ts.isFunctionExpression(fn));
      assert.deepEqual(fn.parameters.map(p=>p.name.getText()),['callee','receiver','values']);assert(!fn.name&&!fn.asteriskToken&&!fn.modifiers?.length);
      assert.equal(fn.getText().replace(/\s/g,''),'function(callee,receiver,values){returnFunction.prototype.apply.call(callee,receiver,values);}','complete inline invocation ABI');
      assert.equal(fn.body.statements.length,1);const returned=fn.body.statements[0];assert(ts.isReturnStatement(returned));
      assert.equal(returned.expression.getText().replace(/\s/g,''),'Function.prototype.apply.call(callee,receiver,values)');
      actual.noSource(start(actual,functionTransport),functionTransport.end,'generated inline invocation helper');
    }
    assert.equal(actualCall.arguments.length,3);const observedCallee=unwrap(actualCall.arguments[0]);assert(ts.isIdentifier(observedCallee)&&observedCallee.text==='total');
    assert.equal(actualCall.arguments[1].getText(),'void 0');const observedArgument=a.helpers.get(actualCall.expression.getText())==='call'?append(a,actualCall.arguments[2],true).leaf:unwrap(actualCall.arguments[2]);
    assert(ts.isIdentifier(observedArgument)&&observedArgument.text==='values');
    pair([native.token(callee)],[actual.token(observedCallee)],'spread call original callee');pair([native.token(argument)],[actual.token(observedArgument)],'spread call original argument');
    const callEntry=native.owner(start(native,callee)),calleeEnd=native.owner(callee.end),argumentEnd=native.owner(argument.end);assert(callEntry&&calleeEnd&&argumentEnd);
    uniform(native,callee.end,start(native,argument),calleeEnd,'Native apply receiver/argument transport');uniform(native,argument.end,nativeCall.end,argumentEnd,'Native call closing');
    uniform(actual,functionTransport?functionTransport.end:start(actual,actualCall),start(actual,observedCallee),callEntry,'call helper/callee prefix');
    uniform(actual,observedCallee.end,start(actual,observedArgument),calleeEnd,'call receiver/argument transport');uniform(actual,observedArgument.end,actualCall.end,argumentEnd,'call argument/closing transport');
    pair([native.token(nx.operatorToken),...native.tokens(nx.right)],[actual.token(ay.operatorToken),...actual.tokens(ay.right)],'addition after spread call');
    call={native:nativeCall,actual:actualCall,callee:observedCallee,argument:observedArgument,functionTransport};
    for(const[view,node]of [[native,x.root],[actual,y.root],[native,nativeCall],[actual,actualCall]])for(let offset=start(view,node);offset<node.end;offset++)assert(view.covered(offset),'unclassified spread expression character '+offset);
    cover(native,x.root.end,x.root.end,native.owner(x.root.end));cover(actual,y.root.end,y.root.end,native.owner(x.root.end));
    cover(native,nativeCall.end,nativeCall.end,native.owner(nativeCall.end));cover(actual,actualCall.end,actualCall.end,native.owner(nativeCall.end));
  }
  suffix(nc,ac);suffix(nb,ab);finish();return {native:n,actual:a,binding:ab,copied,call,bindings:{native:nb,actual:ab},copiedBindings:{native:nc,actual:ac},values:{native:binding(n,1,'values'),actual:binding(a,1,'values')}};
}
module.exports={compare};
