// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Registered machine and fully proved header identities cannot be forged.
const assert=require('node:assert/strict'),adapter=require('./ge5_async_generator_machine.cjs');
function run(ts,native,actual,proof){const n=proof.sequence.core.native.machine,a=proof.sequence.core.actual.machine,controls=[];adapter.signature(ts,native,n,actual,a);
  function reject(label,fn,reason){assert.throws(fn,reason,label);controls.push({label,rejected:true});}
  reject('clone cannot forge registered async machine',()=>adapter.paths(ts,{...a,asyncGeneratorAdapter:true}),/registered async-generator machine identity/);
  reject('swapped callback views cannot grant header source',()=>adapter.signature(ts,actual,n,native,a));
  const originalNode=a.node;a.node=a.callback;try{reject('wrong exact registered callback node',()=>adapter.signature(ts,native,n,actual,a),/registered async-generator machine identity/);}finally{a.node=originalNode;}
  const covered=actual.covered,interior=a.node.getStart(actual.source)+1;actual.covered=offset=>offset===interior?false:covered(offset);try{reject('unproved callback-header UTF-16 interior',()=>adapter.signature(ts,native,n,actual,a),/complete preverified async-generator callback header/);}finally{actual.covered=covered;}
  a.slots.add('JSON');try{reject('unproved slot inventory cannot grant a source alias',()=>adapter.paths(ts,a),/exact registered slot inventory/);}finally{a.slots.delete('JSON');}
  const state=a.states.keys().next().value,statements=a.states.get(state);a.states.delete(state);try{reject('registered machine cannot omit a physical state',()=>adapter.paths(ts,a),/exact registered state inventory/);}finally{const entries=[...a.states];a.states.clear();for(const[label,nodes]of [[state,statements],...entries].sort((left,right)=>left[0]-right[0]))a.states.set(label,nodes);}
  const source=ts.createSourceFile('original.ts',a.originals[0],ts.ScriptTarget.Latest,true),wrongSource=source.statements.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text==='observe');
  reject('ordinary observer cannot grant async-generator protocol',()=>adapter.read(ts,actual,a.node,'blue',a.originals,{source:wrongSource,outer:proof.parts.actual.sequence,marker:proof.parts.actual.marker}));
  reject('source JSON cannot alias the generated marker binding',()=>adapter.read(ts,actual,a.node,'blue',a.originals,{source:source.statements[1],outer:proof.parts.actual.sequence,marker:'JSON'}),/both exact marker and queued-driver identities/);
  return controls;
}
module.exports={run};
