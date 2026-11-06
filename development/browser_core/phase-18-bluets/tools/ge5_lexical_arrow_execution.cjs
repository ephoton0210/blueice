// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Calls, getters and abrupt completion challenge the lexical field receiver.
const assert=require('node:assert/strict'),vm=require('node:vm');
function resultCall(ts,view){const calls=[];function visit(node){if(ts.isCallExpression(node)&&ts.isPropertyAccessExpression(node.expression)&&node.expression.name.text==='read')calls.push(node);ts.forEachChild(node,visit);}visit(view.source);assert.equal(calls.length,1);return calls[0];}
async function run(ts,native,actual,options,proof){
  async function execute(code,expected){const sandbox={},exports={},context=vm.createContext(sandbox);let result;
    if(options.module==='ESNext'){const module=new vm.SourceTextModule(code,{context,identifier:'memory:///main.js'});await module.link(name=>assert.fail('unexpected dependency '+name));await module.evaluate();assert.deepEqual(Object.keys(module.namespace),['Box','result']);result=module.namespace.result;}
    else{sandbox.exports=exports;sandbox.module={exports};vm.runInContext(code,context,{timeout:1000});assert.deepEqual(Object.keys(exports).sort(),['Box','result']);result=exports.result;}
    assert(result&&typeof result.then==='function','async field always returns a promise');
    let value;try{value=await result;}catch(error){assert(Object.hasOwn(expected,'throws'),'unexpected rejection '+error);assert.equal(error,expected.throws);return;}
    assert(!Object.hasOwn(expected,'throws'),'a rejected promise was required');if(expected.nan)assert(Number.isNaN(value));else assert.equal(JSON.stringify(value),JSON.stringify(expected.value));
  }
  const variants=[
    ['original',null,{value:42}],['provided','new Box().read(7)',{value:47}],['zero','new Box().read(0)',{value:40}],['null','new Box().read(null)',{value:40}],['undefined','new Box().read(void 0)',{nan:true}],['string','new Box().read("4")',{value:'404'}],
    ['borrowed-call-and-constructor-arguments','(function(){var box=new Box(900);return box.read.call({value:900},7);})()',{value:47}],
    ['detached-call-and-shadow-receiver','(function(){var box=new Box(800),read=box.read;return read.apply({value:900},[3]);})()',{value:43}],
    ['getter-once-before-await','(function(){var box=new Box(),count=0;Object.defineProperty(box,"value",{configurable:true,get:function(){count++;return count===1?40:900;}});var promise=box.read(2);Object.defineProperty(box,"value",{value:900});return promise.then(function(value){return [value,count];});})()',{value:[42,1]}],
    ['borrowed-receiver-getter-never-read','(function(){var box=new Box(),other={};Object.defineProperty(other,"value",{get:function(){throw "wrong receiver";}});return box.read.call(other,2);})()',{value:42}],
    ['receiver-getter-abrupt-rejects','(function(){var box=new Box();Object.defineProperty(box,"value",{get:function(){throw "receiver getter";}});return box.read(2);})()',{throws:'receiver getter'}],
    ['rejection-original','new Box().read(2)',{throws:2},true],['rejection-borrowed','new Box().read.call({value:900},7)',{throws:7},true],
  ];
  for(const[mode,replacement,expected,reject]of variants)for(const[view,item]of[[native,proof.items.native],[actual,proof.items.actual]]){let code=view.code;const edits=[];if(replacement!==null){const call=resultCall(ts,view);edits.push([call.getStart(view.source),call.end,replacement]);}if(reject){const name=item.awaitCall.expression.name;edits.push([name.getStart(view.source),name.end,'reject']);}for(const[from,to,text]of edits.sort((a,b)=>b[0]-a[0]))code=code.slice(0,from)+text+code.slice(to);await execute(code,expected);}
  return {executions:2,additionalFixtureExecutions:(variants.length-1)*2,variants:variants.map(([mode,,expected])=>({mode,executions:2,...expected}))};
}
module.exports={run,resultCall};
