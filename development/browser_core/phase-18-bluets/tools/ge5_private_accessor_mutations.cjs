// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every control remains a valid complete publication before role comparison.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs');
const {mutate,encode}=require('./ge5_role_mutations.cjs'),roles=require('./ge5_private_accessor_roles.cjs');
function coherent(ts,view){const marker='//# sourceMappingURL=data:application/json;base64,',start=view.code.indexOf(marker);if(start<0)return view;const end=view.code.indexOf('\n',start),code=view.code.slice(0,start)+marker+Buffer.from(view.raw).toString('base64')+(end<0?'':view.code.slice(end));return stream(ts,code,view.raw);}
function run(ts,native,actual,options,original,proof){
  const controls=[],check=view=>roles.compare(ts,stream(ts,native.code,native.raw),coherent(ts,stream(ts,view.code,view.raw)),options,original),positions=new Set();
  assert.doesNotThrow(()=>check(actual),'fresh unchanged complete artifacts pass before any negative control');
  function mapping(label,offset,owner){assert.notDeepEqual(actual.owner(offset),owner,'effective mapping control '+label);assert.throws(()=>check(mutate(ts,actual,offset,owner)),undefined,label);controls.push({label,offset,rejected:true});}
  const tokens=actual.tokens(actual.source),mapped=tokens.filter(token=>token.owners.some(Boolean));
  for(const token of mapped){
    const points=new Set([token.start,token.end-1]);if(token.text.startsWith('#'))for(let at=token.start;at<token.end;at++)points.add(at);
    for(const offset of points)if(actual.owner(offset)!==null&&!positions.has(offset)){positions.add(offset);mapping('erase source UTF-16 '+offset,offset,null);mapping('retarget source UTF-16 '+offset,offset,JSON.stringify(actual.owner(offset))==='[0,0,0]'?[0,0,1]:[0,0,0]);}
    if(actual.owner(token.end)!==null&&!positions.has(token.end)){positions.add(token.end);mapping('erase logical token endpoint '+token.end,token.end,null);}
  }
  const generated=tokens.filter(token=>token.owners.every(owner=>owner===null));
  const selected=new Map();for(const token of generated){const kind=token.text.startsWith('__')?'helper-binding':token.text==='WeakMap'?'private-allocation':token.text==='constructor'?'constructor-entry':token.text==='var'?'generated-declaration':token.text==='require'?'dependency-loader':null;if(kind&&!selected.has(kind))selected.set(kind,token.start);}
  for(const[kind,offset]of selected)mapping('grant generated '+kind,offset,[0,0,0]);
  const metadata=JSON.parse(actual.raw);for(const[field,value]of[['file','wrong.js'],['names',['fabricated']],['sourceRoot','/wrong/'],['sources',['../wrong.ts']],['sourcesContent',[Object.hasOwn(metadata,'sourcesContent')?'x'+original.slice(1):original]],['unreviewed',true]]){assert.notDeepEqual(metadata[field],value);assert.throws(()=>check(stream(ts,actual.code,JSON.stringify({...metadata,[field]:value}))),undefined,'metadata '+field);controls.push({label:'complete metadata '+field,rejected:true});}
  if(!options.inlineSourceMap){const code=actual.code.replace(/(\/\/# sourceMappingURL=)[^\r\n]+/,'$1wrong.js.map');assert.throws(()=>check(stream(ts,code,actual.raw)));controls.push({label:'retarget external map publication rejected',rejected:true});}
  if(!Object.hasOwn(metadata,'sourcesContent')){
    const segments=actual.decoded.segments.map(segment=>[...segment]),index=segments.findIndex(segment=>segment.length>2),first=segments[index];assert(first);segments.splice(index,0,[first[0],first[1],0,original.split('\n').length+1,0]);
    const changed=stream(ts,actual.code,JSON.stringify({...metadata,mappings:encode(segments)}));for(const token of tokens)assert.deepEqual(changed.owner(token.start),actual.owner(token.start),'hidden invalid coordinate leaves every visible token unchanged');assert.throws(()=>check(changed),/every private source coordinate/);controls.push({label:'hidden invalid original coordinate rejected',rejected:true});
  }
  function code(label,node,replacement){const start=node.getStart(actual.source);assert(replacement.length<=node.end-start);const changed=actual.code.slice(0,start)+replacement.padEnd(node.end-start,' ')+actual.code.slice(node.end);assert.notEqual(changed,actual.code);assert.throws(()=>check(stream(ts,changed,actual.raw)),undefined,label);controls.push({label,rejected:true});}
  if(proof.lowered){
    for(const operation of proof.operations){
      const call=operation.call;
      if(operation.kind==='Get')code('private read rejects a method-kind helper',call.arguments.at(-1),'"m"');
      if(operation.kind==='Set')code('private setter rejects another value binding',call.arguments[2],'other');
      if(operation.kind==='In')code('private membership rejects a foreign state',call.arguments[0],'_Bad_state');
    }
    const provider=proof.providers.actual;if(provider.helpers.size){const fn=provider.helpers.get('Get').fn;const value=actual.tokens(fn).find(token=>token.text==='"m"');assert(value);const node={getStart:()=>value.start,end:value.end};code('complete private helper algorithm is binding checked',node,'"x"');}
    const helper=require('./ge5_private_accessor_helpers.cjs'),operation=proof.operations[0];
    assert.throws(()=>helper.reference(ts,{...proof.providers},provider,operation.call.expression));controls.push({label:'cloned helper proof rejected',rejected:true});
    assert.throws(()=>helper.reference(ts,proof.providers,{...provider},operation.call.expression));controls.push({label:'cloned helper provider rejected',rejected:true});
  }
  assert.throws(()=>roles.compare(ts,stream(ts,native.code,native.raw),stream(ts,actual.code,actual.raw),options,original.replace('= 40;','= 41;')),undefined,'wrong original initializer');controls.push({label:'wrong original initializer rejected',rejected:true});
  assert.throws(()=>roles.compare(ts,stream(ts,native.code,native.raw),stream(ts,actual.code,actual.raw),options,original.replace('return this.#value','return this.#other')),undefined,'wrong original private body');controls.push({label:'wrong original private body rejected',rejected:true});
  return controls;
}
module.exports={run,coherent};
