// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The field callable, lexical factory and promise driver form one closed wrapper.
const assert=require('node:assert/strict'),{compareGap,compareTokens}=require('./ge5_role_stream.cjs'),reader=require('./ge5_lexical_arrow_read.cjs'),receiver=require('./ge5_lexical_arrow_receiver.cjs'),signature=require('./ge5_lexical_arrow_signature.cjs'),body=require('./ge5_lexical_arrow_body.cjs'),machine=require('./ge5_continuation_machine.cjs'),linear=require('./ge5_linear_roles.cjs');
function compare(ts,native,n,actual,a,original,options){const start=(view,node)=>node.getStart(view.source),uniform=(view,from,to,owner,label)=>{for(let offset=from;offset<to;offset++)assert.deepEqual(view.owner(offset),owner,label+' at '+offset);view.cover(from,to);};
  if(n.retained){compareTokens(native,native.tokens(n.expression),actual,actual.tokens(a.expression),'complete retained lexical field arrow');assert.deepEqual(actual.owner(a.expression.end),native.owner(n.expression.end));return {native:n,actual:a};}
  const nr=receiver.proof(ts,n),ar=receiver.proof(ts,a),sig=signature.header(ts,native,n,actual,a);let core=null;
  if(options.target==='ES5'){const nm=machine.read(ts,native,n.callback,'native',[original],{lexicalReceiver:nr}),am=machine.read(ts,actual,a.callback,'blue',[original],{lexicalReceiver:ar});nm.reviewedSignature=sig;core=linear.compareFunction(ts,native,nm,actual,am);}
  else body.compare(ts,native,n,actual,a);
  const phase=native.owner(start(native,n.driver));assert(phase);uniform(native,start(native,n.driver),start(native,n.callback),phase,'complete Native lexical driver prefix');uniform(actual,start(actual,a.driver),start(actual,a.callback),phase,'complete lexical driver forwarding prefix');
  if(ts.isFunctionExpression(n.fn)){const from=start(native,n.fn.body)+1;uniform(native,from,start(native,n.driver),phase,'Native synthesized return/driver prefix');}
  else {const operation=n.fn.equalsGreaterThanToken;uniform(native,operation.end,start(native,n.driver),phase,'Native arrow/driver boundary');}
  const ending=native.owner(n.callback.end);uniform(native,n.callback.end,n.driver.end,ending,'Native driver closing syntax');uniform(actual,a.callback.end,a.driver.end,ending,'lexical driver closing syntax');assert.deepEqual(actual.owner(a.driver.end),native.owner(n.driver.end),'complete lexical driver logical end');
  const returned=a.entryFunction.body.statements.at(-1);assert(ts.isReturnStatement(returned)&&returned.expression===a.driver);uniform(actual,a.driver.end,returned.end,ending,'lexical driver statement ending');
  const close=actual.tokens(a.entryFunction).at(-1);assert.equal(close.text,'}');assert(/^\s*$/.test(actual.code.slice(returned.end,close.start)));compareGap(native,n.fn.end,n.fn.end,actual,returned.end,close.start,'lexical callable completion/closure gap');
  if(ts.isFunctionExpression(n.fn))for(const token of native.tokens(n.fn).filter(token=>token.start>=n.driver.end)){assert([';','}'].includes(token.text),'only Native synthesized wrapper completion remains');uniform(native,token.start,token.end,native.owner(token.start),'Native lexical callable completion syntax');}
  assert.deepEqual(actual.owner(a.expression.end),native.owner(n.expression.end),'complete lexical field initializer endpoint');
  const generated=new Set([a.capture.receiver.text,a.capture.arguments.text,a.entryFunction.name.text,...a.entryFunction.parameters.map(p=>p.name.text)]);assert.equal(generated.size,4);assert(a.capture.invocation.arguments[0].kind===ts.SyntaxKind.ThisKeyword);assert.equal(a.capture.invocation.arguments[1].getText(actual.source).replace(/\s/g,''),"typeofarguments==='undefined'?void0:arguments");
  if(a.entryFunction.body.statements.length===2){const guard=a.entryFunction.body.statements[0];assert.equal(guard.getText(actual.source).replace(/\s/g,''),'if(new.target)thrownewTypeError("Asyncfunctionisnotaconstructor");');actual.noSource(start(actual,guard),guard.end,'generated lexical callable constructor guard');}else assert.equal(options.target,'ES5');
  for(const[view,item]of[[native,n],[actual,a]])for(let offset=start(view,item.expression);offset<item.expression.end;offset++)if(!view.covered(offset)){assert.equal(view.owner(offset),null,'unclassified lexical field wrapper source grant at '+offset);view.cover(offset,offset+1);}
  return {native:n,actual:a,receiver:{native:nr,actual:ar},signature:sig,core,capture:nr.capture};
}
module.exports={compare};
