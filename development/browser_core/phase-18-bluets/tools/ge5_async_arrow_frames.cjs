// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Callback grammar, yielded body and promise-driver boundaries are separate roles.
const assert=require('node:assert/strict'),{compareTokens,compareGap}=require('./ge5_role_stream.cjs');
function body(ts,native,n,actual,a){
  const ns=n.callback.body.statements,as=a.callback.body.statements;assert.equal(ns.length,2);assert.equal(as.length,2);assert(ts.isVariableStatement(ns[0])&&ts.isVariableStatement(as[0]));
  const nd=ns[0].declarationList.declarations,ad=as[0].declarationList.declarations;assert.equal(nd.length,1);assert.equal(ad.length,1);
  assert(ts.isYieldExpression(nd[0].initializer)&&ts.isCallExpression(nd[0].initializer.expression));
  const outer=ad[0].initializer;assert(ts.isParenthesizedExpression(outer)&&ts.isYieldExpression(outer.expression));const inner=outer.expression.expression;assert(ts.isParenthesizedExpression(inner)&&ts.isCallExpression(inner.expression));
  assert(ts.isReturnStatement(ns[1])&&ts.isReturnStatement(as[1]));
  const groups=[outer,inner],holes=groups.flatMap(node=>[[node.getStart(actual.source),node.getStart(actual.source)+1],[node.end-1,node.end]]);
  for(const [from,to]of holes)actual.noSource(from,to,'generated retained-yield precedence token');
  const tokens=actual.tokens(a.callback.body).filter(token=>!holes.some(([from,to])=>from<=token.start&&token.end<=to));
  compareTokens(native,native.tokens(n.callback.body),actual,tokens,'complete retained callback body',holes);
}
function audit(ts,native,n,actual,a){
  const failures=[],start=(v,node)=>node.getStart(v.source),attempt=(role,check)=>{try{check();}catch(error){failures.push({role,error:error.message});}};
  function uniform(v,from,to,owner,label){for(let offset=from;offset<to;offset++)assert.deepEqual(v.owner(offset),owner,label+' at '+offset);v.cover(from,to);}
  const nh=native.tokens(n.callback).filter(token=>token.end<=start(native,n.callback.body)+1),ah=actual.tokens(a.callback).filter(token=>token.end<=start(actual,a.callback.body)+1);assert.equal(ah.length,nh.length,'complete callback header token inventory');
  for(let index=0;index<nh.length;index++)attempt('callback header '+index+' '+nh[index].text,()=>compareTokens(native,[nh[index]],actual,[ah[index]],'same callback grammar token'));
  attempt('callback signature ordered gaps',()=>compareTokens(native,nh,actual,ah,'complete callback signature'));
  const prefix=native.owner(start(native,n.driver));
  attempt('Native complete driver prefix',()=>uniform(native,start(native,n.driver),start(native,n.callback),prefix,'Native driver argument forwarding'));
  attempt('complete Blue driver prefix',()=>uniform(actual,start(actual,a.driver),start(actual,a.callback),prefix,'Blue driver argument forwarding'));
  attempt('callback logical ending',()=>assert.deepEqual(actual.owner(a.callback.end),native.owner(n.callback.end)));
  attempt('driver closing syntax',()=>uniform(actual,a.callback.end,a.driver.end,native.owner(n.callback.end),'Blue promise driver close'));
  attempt('Native driver closing syntax',()=>uniform(native,n.callback.end,n.driver.end,native.owner(n.callback.end),'Native promise driver close'));
  attempt('driver logical ending',()=>assert.deepEqual(actual.owner(a.driver.end),native.owner(n.driver.end)));
  const ni=n.declaration.initializer,ai=a.declaration.initializer;
  attempt('arrow replacement completion',()=>assert.deepEqual(actual.owner(ai.end),native.owner(ni.end)));
  if(n.rest.declaration){attempt('Native complete rest argument transport',()=>native.noSource(start(native,n.rest.declaration),n.rest.loop.end,'Native generated rest-copy algorithm'));}
  if(a.guard)attempt('generated callable constructor guard',()=>actual.noSource(start(actual,a.guard),a.guard.end,'generated non-constructor check'));
  if(n.callback.asteriskToken){
    attempt('complete retained callback body',()=>body(ts,native,n,actual,a));
    const initializer=a.callback.body.statements[0].declarationList.declarations[0].initializer,inner=initializer.expression.expression;
    const holes=[initializer,inner].flatMap(node=>[[start(actual,node),start(actual,node)+1],[node.end-1,node.end]]),nt=native.tokens(n.callback.body),at=actual.tokens(a.callback.body).filter(token=>!holes.some(([from,to])=>from<=token.start&&token.end<=to));
    assert.equal(nt.length,at.length);
    for(let index=0;index<nt.length;index++){
      attempt('callback body token '+index+' '+nt[index].text,()=>compareTokens(native,[nt[index]],actual,[at[index]],'complete retained callback body token'));
      if(index)attempt('callback body gap before '+index+' '+nt[index].text,()=>compareGap(native,nt[index-1].end,nt[index].start,actual,at[index-1].end,at[index].start,'complete callback body ordered gap',holes));
    }
  }
  return failures;
}
function complete(ts,native,n,actual,a){
  const failures=audit(ts,native,n,actual,a);assert.equal(failures.length,0,JSON.stringify(failures));
  const start=(v,node)=>node.getStart(v.source),uniform=(v,from,to,owner,label)=>{for(let offset=from;offset<to;offset++)assert.deepEqual(v.owner(offset),owner,label+' at '+offset);v.cover(from,to);};
  const nn=native.tokens(n.fn),aa=actual.tokens(a.entryFunction),identity=native.owner(start(native,n.fn)),keyword=aa[0];assert.equal(keyword.text,'function');
  uniform(actual,keyword.start,start(actual,a.entryFunction.name),identity,'lowered arrow identity and its boundary');
  const open=actual.tokens(a.entryFunction).find(token=>token.text==='{');assert(open);
  if(n.rest.declaration){
    uniform(native,start(native,n.fn),start(native,n.fn.body)+1,identity,'Native ES5 callable signature');
    uniform(actual,open.start,open.end,native.owner(start(native,n.fn.body)),'lowered ES5 callable operation');
    compareGap(native,start(native,n.fn.body)+1,start(native,n.rest.declaration),actual,open.end,start(actual,a.entryFunction.body.statements[0]),'complete generated callable body entry gap');
    const close=nn.at(-1);assert.equal(close.text,'}');uniform(native,close.start,close.end,native.owner(close.start),'Native callable completion syntax');
  }else{
    const operation=n.fn.equalsGreaterThanToken;assert(operation);
    uniform(native,start(native,n.fn),start(native,operation),identity,'Native rest-arrow identity');
    uniform(native,start(native,operation),operation.end,native.owner(start(native,operation)),'Native arrow operation');
    uniform(actual,open.start,open.end,native.owner(start(native,operation)),'lowered arrow operation');
    uniform(native,operation.end,start(native,n.driver),native.owner(start(native,n.driver)),'Native arrow/driver phase boundary');
  }
  const ending=native.owner(n.callback.end),returned=a.entryFunction.body.statements.at(-1);assert(ts.isReturnStatement(returned));
  uniform(actual,a.driver.end,returned.end,ending,'complete generated driver statement ending');
  const closing=actual.tokens(a.entryFunction).at(-1);assert.equal(closing.text,'}');
  assert(/^\s*$/.test(actual.code.slice(returned.end,closing.start)),'only completed-callable layout precedes its generated closure');
  compareGap(native,n.fn.end,n.fn.end,actual,returned.end,closing.start,'callable completion/closure ordered gap');
  for(const[view,from,to]of [[native,start(native,n.declaration.initializer),n.declaration.initializer.end],[actual,start(actual,a.declaration.initializer),a.declaration.initializer.end]]){
    for(let offset=from;offset<to;offset++)if(!view.covered(offset)){assert.equal(view.owner(offset),null,'unclassified callable wrapper source grant at '+offset);view.cover(offset,offset+1);}
  }
}
module.exports={body,audit,complete};
