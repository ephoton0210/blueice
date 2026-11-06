// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The capability binds original AST ownership and already-verified UTF-16 bytes.
const assert=require('node:assert/strict'),prologue=require('./ge5_async_arrow_prologue.cjs'),machine=require('./ge5_continuation_machine.cjs'),{inspect}=require('./ge5_async_arrow_roles.cjs'),{stream}=require('./ge5_role_stream.cjs');
function run(ts,factory,options,original){const first=factory(),n=first.nativeMachine,controls=[];
  const compare=descriptor=>prologue.interval(ts,descriptor.view,descriptor);
  assert.deepEqual(compare(n),n.reviewedPrologue.range);
  for(const[label,change]of[
    ['missing closed prologue capability',()=>undefined],
    ['prologue interval extends past its original IfStatement',cap=>({...cap,range:[cap.range[0],cap.range[1]+1]})],
    ['prologue capability is a different original statement',cap=>({...cap,statement:n.node.body.statements[0]})],
    ['prologue capability has no original parameter',cap=>({...cap,parameter:cap.statement.thenStatement})],
  ]){assert.throws(()=>compare({...n,reviewedPrologue:change(n.reviewedPrologue)}),undefined,label);controls.push({label,rejected:true});}
  const view=stream(ts,n.view.code,n.view.raw),item=inspect(ts,view,options,original,false),bare=machine.read(ts,view,item.callback,'native',[original],{bodyStatements:item.callback.body.statements.filter(node=>node!==item.defaultGuard)});
  bare.reviewedPrologue={kind:'verified-original-default-parameter-v1',statement:item.defaultGuard,parameter:item.parameter,range:[item.defaultGuard.getStart(view.source),item.defaultGuard.end]};
  assert.throws(()=>compare(bare),/independently verified/,'an original AST interval alone cannot authorize unverified source bytes');controls.push({label:'prologue bytes were not independently verified',rejected:true});
  return {positive:'closed original-AST and independently verified UTF-16 prologue capability',controls};
}
module.exports={run};
