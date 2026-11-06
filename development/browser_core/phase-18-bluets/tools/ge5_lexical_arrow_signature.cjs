// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native outer formals move into the Blue callback under a closed field-arrow ABI.
const assert=require('node:assert/strict'),{compareTokens,compareGap}=require('./ge5_role_stream.cjs');
const start=(view,node)=>node.getStart(view.source);
function header(ts,native,n,actual,a){assert.equal(n.fn.parameters.length,1);assert.equal(n.callback.parameters.length,0);assert.equal(a.callback.parameters.length,1);assert.equal(a.entryFunction.parameters.length,1);
  const nt=native.tokens(n.fn),at=actual.tokens(a.entryFunction),nh=native.tokens(n.callback).filter(token=>token.end<=start(native,n.callback.body)+1),ah=actual.tokens(a.callback).filter(token=>token.end<=start(actual,a.callback.body)+1),no=nt.find(token=>token.text==='('),nc=nt.find(token=>token.text===')'),ao=ah.find(token=>token.text==='('),ac=ah.find(token=>token.text===')'),body=ah.at(-1);assert(no&&nc&&ao&&ac&&body.text==='{');
  function uniform(view,from,to,owner,label){for(let offset=from;offset<to;offset++)assert.deepEqual(view.owner(offset),owner,label+' at '+offset);view.cover(from,to);}
  const identity=native.owner(nt[0].start),keyword=at[0];assert.equal(keyword.text,'function');uniform(native,nt[0].start,nt[0].end,identity,'Native field-arrow identity');uniform(actual,keyword.start,start(actual,a.entryFunction.name),identity,'captured field-arrow identity');
  compareTokens(native,native.tokens(n.fn.parameters[0]),actual,actual.tokens(a.callback.parameters[0]),'complete relocated field-arrow formal');compareTokens(native,[nc],actual,[ac],'relocated field-arrow parameter closing');
  if(ts.isFunctionExpression(n.fn))compareTokens(native,[no],actual,[ao],'relocated ES5 field-arrow parameter opening');else uniform(actual,ao.start,ao.end,native.owner(nh[0].start),'generated callback opening before moved formal');
  compareGap(native,no.end,start(native,n.fn.parameters[0]),actual,ao.end,start(actual,a.callback.parameters[0]),'relocated field formal entry gap');compareGap(native,n.fn.parameters[0].end,nc.start,actual,a.callback.parameters[0].end,ac.start,'relocated field formal completion gap');
  const operation=ts.isFunctionExpression(n.fn)?native.tokens(n.fn).find(token=>token.text==='{'):native.token(n.fn.equalsGreaterThanToken),actualOperation=at.find(token=>token.text==='{');assert(operation&&actualOperation);
  uniform(native,operation.start,operation.end,native.owner(operation.start),'Native field-arrow operation');uniform(actual,actualOperation.start,actualOperation.end,native.owner(operation.start),'captured field-arrow operation');
  const phase=native.owner(nh[0].start);uniform(native,nh[0].start,nh.at(-1).end,phase,'complete Native generated field-arrow callback signature');uniform(actual,ah[0].start,ao.start,phase,'complete callback prefix before relocated formal');uniform(actual,body.start,body.end,phase,'field-arrow callback body entry');
  compareGap(native,nc.end,operation.start,actual,ac.end,body.start,'relocated signature/body phase gap');
  // The remainder of the outer formal list is generated transport, not a second
  // copy of the original argument binding.
  actual.noSource(start(actual,a.entryFunction.name),actualOperation.start,'complete generated outer callable formal transport');
  for(let offset=nt[0].end;offset<operation.start;offset++)if(!native.covered(offset)){assert(/\s/.test(native.code[offset]));uniform(native,offset,offset+1,native.owner(offset),'Native outer field signature layout');}
  return Object.freeze({kind:'verified-relocated-callable-signature-v1',sourceKind:'class-field-async-arrow',source:n.origin.arrow,nativeOuter:n.fn,actualOuter:a.entryFunction,nativeCallback:n.callback,actualCallback:a.callback,formalPairs:Object.freeze([{source:n.origin.arrow.parameters[0],native:n.fn.parameters[0],actual:a.callback.parameters[0]}]),ranges:Object.freeze([[start(native,n.callback),start(native,n.callback.body)+1],[start(actual,a.callback),start(actual,a.callback.body)+1]])});
}
module.exports={header};
