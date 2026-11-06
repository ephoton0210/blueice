// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Source/dataflow and endpoint controls precede production changes. No module claim.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),references=require('./ge5_static_super_references.cjs');
function coherent(ts,view){const marker='//# sourceMappingURL=data:application/json;base64,',start=view.code.indexOf(marker);if(start<0)return view;const end=view.code.indexOf('\n',start);return stream(ts,view.code.slice(0,start)+marker+Buffer.from(view.raw).toString('base64')+(end<0?'':view.code.slice(end)),view.raw);}
function verify(ts,native,actual,source,options){const n=references.read(ts,native,source,options),a=references.read(ts,actual,source,options);assert.deepEqual(n.expressions.map(e=>e.proof.semantic),n.expressions.map(e=>e.sourceProof.semantic),'Native reference program equals the independent original AST');assert.deepEqual(a.expressions.map(e=>e.proof.semantic),n.expressions.map(e=>e.proof.semantic),'complete reference effects and returned values');assert.equal(a.expressions.length,n.expressions.length);for(let i=0;i<n.expressions.length;i++){
 const np=n.expressions[i],ap=a.expressions[i];assert.deepEqual(actual.owner(ap.node.end),native.owner(np.node.end),'logical source endpoint: complete expression');assert.equal(ap.proof.effects.length,np.proof.effects.length);
 for(let j=0;j<np.proof.effects.length;j++){const ne=np.proof.effects[j],ae=ap.proof.effects[j];if(['sourceCall','invoke','get','set'].includes(ne.kind))assert.deepEqual(actual.owner(ae.logicalNode.end),native.owner(ne.logicalNode.end),'logical source endpoint: '+ne.kind);}
 }return {native:n,actual:a};}
function run(ts,native,source,options){const fresh=()=>stream(ts,native.code,native.raw),check=actual=>verify(ts,fresh(),coherent(ts,actual),source,options),proof=check(fresh()),controls=[],seen=new Set();
 function mapping(label,offset,owner){const key=offset+':'+JSON.stringify(owner);if(seen.has(key))return;seen.add(key);assert.notDeepEqual(native.owner(offset),owner,'effective '+label);const changed=coherent(ts,mutate(ts,fresh(),offset,owner));assert.throws(()=>check(changed),/logical source endpoint/,label);controls.push({label,offset,rejected:true,kind:'coherent-map'});}
 const points=proof.native.expressions.flatMap(e=>[{label:'expression',offset:e.node.end},...e.proof.effects.filter(effect=>['sourceCall','invoke','get','set'].includes(effect.kind)).map(effect=>({label:effect.kind,offset:effect.logicalNode.end}))]);
 for(const {label,offset}of points){if(native.owner(offset)!==null)mapping('erase '+label+' completion',offset,null);mapping('retarget '+label+' completion',offset,JSON.stringify(native.owner(offset))==='[0,0,0]'?[0,0,1]:[0,0,0]);}
 function code(label,node,replacement){const from=node.getStart(native.source);assert(replacement.length<=node.end-from);const changed=native.code.slice(0,from)+replacement.padEnd(node.end-from,' ')+native.code.slice(node.end);assert.notEqual(changed,native.code);const actual=coherent(ts,stream(ts,changed,native.raw));assert.throws(()=>check(actual),undefined,label);controls.push({label,rejected:true,kind:'complete-AST-code'});}
 const call=references.referenceCalls(ts,proof.native.frame)[0];assert(call);code('wrong reference receiver',call.arguments.at(-1),'null');code('wrong reference base',call.arguments[0],'null');code('wrong complete reference operation',call.expression.name,call.expression.name.text==='get'?'set':'get');code('wrong captured property key',call.arguments[1],'"x"');
 const sourceCall=proof.native.expressions.flatMap(e=>e.proof.effects).find(e=>e.kind==='sourceCall');if(sourceCall)code('drop required original operand evaluation',sourceCall.node,'0');
 for(const [label,changed]of [['wrong original heritage',source.text.replace(/class C extends Base/,'class C extends Other')],['wrong original decorator',source.text.replace('@replace','@dec')]]){assert.notEqual(changed,source.text);const original=ts.createSourceFile('wrong.ts',changed,99,true);assert.throws(()=>verify(ts,fresh(),fresh(),original,options),undefined,label);controls.push({label,rejected:true,kind:'original-AST'});}
 return {controls,completeModuleRoles:false};}
module.exports={run,verify,coherent};

// Each candidate control rebuilds both views and re-derives every AST identity.
function actualProof(ts,native,actual,source,options){
 const n=references.read(ts,native,source,options),a=references.read(ts,actual,source,options,{blue:true});
 assert.deepEqual(n.expressions.map(e=>e.proof.semantic),n.expressions.map(e=>e.sourceProof.semantic),'independent original AST/native reference semantics');
 assert.deepEqual(a.expressions.map(e=>e.proof.semantic),n.expressions.map(e=>e.proof.semantic),'complete candidate reference semantics');
 const points=[];
 function end(nn,an,label){assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'logical source endpoint: '+label);points.push({label,offset:an.end});}
 for(let i=0;i<n.expressions.length;i++){
  const ne=n.expressions[i],ae=a.expressions[i];
  if(ne.kind==='field-array'){
   end(ne.body,ae.body,'array body');
   if(ne.temporary){assert.notEqual(ae.body.end,ae.transport.end,'distinct array body and generated value completion');end(ne.transport,ae.transport,'temporary envelope');}
  }else end(ne.node,ae.node,'complete program');
  const kinds=new Set(['sourceCall','invoke','get','set']),nf=ne.proof.physicalEffects.filter(e=>kinds.has(e.kind)),af=ae.proof.physicalEffects.filter(e=>kinds.has(e.kind));assert.equal(af.length,nf.length);
  for(let j=0;j<nf.length;j++){assert.equal(af[j].kind,nf[j].kind);end(nf[j].logicalNode,af[j].logicalNode,nf[j].kind);}
 }
 return {native:n,actual:a,points};
}
function runActual(ts,native,actual,source,options){
 const freshN=()=>stream(ts,native.code,native.raw),freshA=()=>stream(ts,actual.code,actual.raw),check=view=>actualProof(ts,freshN(),coherent(ts,view),source,options),proof=check(freshA()),controls=[],seen=new Set();
 for(const point of proof.points){
  for(const owner of [null,JSON.stringify(actual.owner(point.offset))==='[0,0,0]'?[0,0,1]:[0,0,0]]){
   if(JSON.stringify(owner)===JSON.stringify(actual.owner(point.offset)))continue;
   const key=point.offset+':'+JSON.stringify(owner);if(seen.has(key))continue;seen.add(key);
   const changed=coherent(ts,mutate(ts,freshA(),point.offset,owner));
   assert.notDeepEqual(changed.owner(point.offset),actual.owner(point.offset),'effective candidate completion mutation');
   assert.throws(()=>check(changed),/logical source endpoint/,'candidate '+point.label+' completion');
   controls.push({label:'candidate '+point.label+' completion',offset:point.offset,rejected:true,kind:'coherent-map'});
  }
 }
 function code(label,node,replacement){const from=node.getStart(actual.source);assert(replacement.length<=node.end-from);const text=actual.code.slice(0,from)+replacement.padEnd(node.end-from,' ')+actual.code.slice(node.end);assert.notEqual(text,actual.code,'effective candidate AST mutation');assert.throws(()=>check(coherent(ts,stream(ts,text,actual.raw))),undefined,label);controls.push({label,rejected:true,kind:'candidate-complete-AST'});}
 const call=references.referenceCalls(ts,proof.actual.frame)[0];assert(call);code('candidate wrong receiver',call.arguments.at(-1),'null');code('candidate wrong base',call.arguments[0],'null');code('candidate wrong reference operation',call.expression.name,call.expression.name.text==='get'?'set':'get');code('candidate wrong property key',call.arguments[1],'"x"');
 const operand=proof.actual.expressions.flatMap(e=>e.proof.effects).find(e=>e.kind==='sourceCall');if(operand)code('candidate drops original operand evaluation',operand.node,'0');
 for(const [label,text]of [['candidate wrong original heritage',source.text.replace(/class C extends Base/,'class C extends Other')],['candidate wrong original decorator',source.text.replace('@replace','@dec')]]){assert.notEqual(text,source.text);assert.throws(()=>actualProof(ts,freshN(),freshA(),ts.createSourceFile('wrong.ts',text,99,true),options),undefined,label);controls.push({label,rejected:true,kind:'candidate-original-AST'});}
 return {controls,completeModuleRoles:false};
}
module.exports.actualProof=actualProof;
module.exports.runActual=runActual;
