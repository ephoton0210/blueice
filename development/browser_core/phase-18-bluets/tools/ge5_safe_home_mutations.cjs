// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every effective mutation reenters the complete contract using fresh independent streams.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate,encode}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs'),{compare}=require('./ge5_safe_home_module.cjs');
function run(ts,native,actual,options,original,proof) {
  const controls=[],attacks=new Set(),claimed=new Set(native.decoded.segments.filter(s=>s.length>2).map(s=>JSON.stringify(s.slice(2))));
  const col=Array.from({length:original.split('\n')[0].length+1},(_,i)=>i).find(i=>!claimed.has(JSON.stringify([0,0,i])));assert.notEqual(col,undefined);const wrong=[0,0,col];
  function check(changed){return compare(ts,stream(ts,native.code,native.raw),stream(ts,changed.code,changed.raw),options,original);}
  function reject(label,changed){assert(changed.code!==actual.code||changed.raw!==actual.raw,'effective complete counterfactual');changed=coherent(ts,changed);assert.equal(changed.source.parseDiagnostics.length,0,'valid JavaScript negative');assert.throws(()=>check(changed),undefined,label);controls.push({label,rejected:true,freshWholeModuleContract:true});}
  function mapping(label,offset,owner){const key=JSON.stringify([offset,owner]);if(attacks.has(key))return;attacks.add(key);assert.notDeepEqual(actual.owner(offset),owner);reject(label,mutate(ts,actual,offset,owner));}
  function code(label,node,replacement){const from=node.getStart(actual.source),to=node.end;assert.equal(replacement.length,to-from);assert.notEqual(actual.code.slice(from,to),replacement);reject(label,stream(ts,actual.code.slice(0,from)+replacement+actual.code.slice(to),actual.raw));}
  for(const point of proof.endings.unique){mapping('exact complete logical end '+point.kind,point.actual,wrong);if(actual.owner(point.actual))mapping('erase complete logical end '+point.kind,point.actual,null);}
  assert(proof.endings.unique.length&&proof.endings.completeOriginalLogicalSubtrees);
  const roles=new Set(),gaps=new Set(),tokens=actual.tokens(actual.source);
  for(const token of tokens){const offset=token.start+Number(token.end-token.start>1),owner=actual.owner(offset),key=JSON.stringify(owner);if(owner&&!roles.has(key)){roles.add(key);mapping('retarget complete UTF-16 role '+key,offset,wrong);mapping('erase complete UTF-16 role '+key,offset,null);}}
  for(let i=1;i<tokens.length;i++)if(tokens[i-1].end<tokens[i].start){const offset=tokens[i-1].end,key=JSON.stringify(actual.owner(offset));if(!gaps.has(key)){gaps.add(key);mapping('retarget ordered physical gap '+key,offset,wrong);}}
  const p=proof.private.actual;
  for(const [index,entry]of p.entries.entries()) {
    code('change exact private callable name '+index,entry.definition.arguments[2].properties[0].initializer,'"'+('X'.repeat(entry.definition.arguments[2].properties[0].initializer.text.length))+'"');
    const context=entry.context;code('change original private kind '+index,context.kind,JSON.stringify('X'.repeat(context.kind.text.length)));
    const value=actual.tokens(entry.fn.body).find(t=>/^[0-9]$/.test(t.text));if(value)reject('change original private callable effect '+index,stream(ts,actual.code.slice(0,value.start)+'9'+actual.code.slice(value.end),actual.raw));
    mapping('synthetic overwritten callable binding gains source '+index,entry.fn.name.getStart(actual.source)+1,wrong);
  }
  for(const [name,node]of [['private helper',proof.helpers.private.actual.values().next().value?.statement],['metadata allocation',p.metadata.parent.parent]])if(node){const t=actual.tokens(node).find(t=>actual.owner(t.start)===null);assert(t);mapping('generated '+name+' gains source authority',t.start,wrong);}
  const queue=proof.classes.find(c=>c.source===proof.cap.cls).frames.actual;
  if(queue.state.length){const allocation=queue.state[0].right;code('brand allocation constructor is independently constrained',allocation.expression,'WeakMap');}
  const footer=actual.comments().find(c=>c.text.startsWith('//# sourceMappingURL='));assert(footer);mapping('generated map publication acquires source authority',footer.start+4,wrong);
  const pos=actual.source.getLineAndCharacterOfPosition(footer.start+4),segments=actual.decoded.segments.map(s=>[...s]);let at=segments.findIndex(s=>s[0]>pos.line||s[0]===pos.line&&s[1]>=pos.character);if(at<0)at=segments.length;segments.splice(at,0,[pos.line,pos.character,...wrong],[pos.line,pos.character]);reject('hidden zero-width source grant',stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),mappings:encode(segments)})));
  for(const[key,value]of[['names',['forged']],['sourceRoot','../forged/'],['file','forged.js'],['sources',['forged.ts']],['sourcesContent',[original+' ']]])reject('complete metadata '+key,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[key]:value})));
  reject('unexpected complete comment inventory',stream(ts,actual.code+'\n/* extra effect */\n',actual.raw));
  controls.push(...require('./ge5_safe_home_semantic_mutations.cjs').run(ts,native,actual,options,original,proof));
  return controls;
}
module.exports={run};
