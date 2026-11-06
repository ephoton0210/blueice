// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Metadata, publication and even shadowed raw coordinates remain observable proof.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate,encode}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_async_arrow_guard_mutations.cjs');
function run(ts,native,actual,options,original,check){const controls=[],raw=JSON.parse(actual.raw);function reject(label,changed,reason){assert.throws(()=>check(coherent(ts,changed)),reason,label);controls.push({label,rejected:true,publicationCoherent:true});}
  for(const[label,key,value]of[
    ['wrong map artifact file','file','wrong.js'],['wrong source root','sourceRoot','https://wrong.invalid/'],['unexpected name metadata','names',['unreviewed']],['wrong source table','sources',['../wrong.ts']],
    ['wrong or added source content','sourcesContent',[Object.hasOwn(raw,'sourcesContent')?'x'+original.slice(1):original]],['unreviewed metadata field','unreviewed',true],
  ])reject(label,stream(ts,actual.code,JSON.stringify({...raw,[key]:value})));
  if(!options.inlineSourceMap){const code=actual.code.replace(/(\/\/# sourceMappingURL=)[^\r\n]+/,'$1wrong.js.map');reject('retarget external map publication',stream(ts,code,actual.raw));}
  const comment=actual.comments().find(comment=>comment.text.startsWith('// This Source Code Form'));if(comment)reject('grant generated license comment source',mutate(ts,actual,comment.start+4,[0,0,0]));
  if(!Object.hasOwn(raw,'sourcesContent')){const segments=actual.decoded.segments.map(segment=>[...segment]),index=segments.findIndex(segment=>segment.length>2),originalSegment=segments[index];assert(originalSegment);segments.splice(index,0,[originalSegment[0],originalSegment[1],0,original.split('\n').length+1,0]);const changed=stream(ts,actual.code,JSON.stringify({...raw,mappings:encode(segments)}));
    for(const token of actual.tokens(actual.source))assert.deepEqual(changed.owner(token.start),actual.owner(token.start),'a shadowed raw coordinate leaves every visible token owner unchanged');
    reject('shadowed source coordinate outside original content',changed,/every callable source coordinate/);
  }
  return controls;
}
module.exports={run};
