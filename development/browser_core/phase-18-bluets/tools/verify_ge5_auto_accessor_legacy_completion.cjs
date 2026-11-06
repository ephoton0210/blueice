// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps'),input=require(path.join(fixture,'cases.json')),reference=require(path.join(fixture,'reference.json')),graph=require('./ge5_inputs.cjs'),parents=graph.parents(root,input),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,8);let controls=0;
for(const row of rows){
 const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];assert.equal(original.options.target,'ES2015');assert(original.options.experimentalDecorators);
 for(const[name,map]of Object.entries(expected.maps)){
  if(map.kind!=='javascript')continue;const code=reference.blobs[expected.outputs[map.output]],raw=authority.raw(reference,expected,name),a=stream(ts,row.outputs[map.output],row.outputs[name]);
  function check(candidate){return require('./ge5_decorator_auto_module.cjs').compare(ts,stream(ts,code,raw),stream(ts,candidate.code,candidate.raw),original.options,original.files[original.entry]);}
  const proof=check(a);check(a);const entry=[...proof.delegated.components.core.actual.entries].find(([plan])=>plan.static)[1],end=entry.allocation.parent.end,owner=a.owner(end);assert(owner,'original static auto-accessor statement completion remains mapped');
  for(const wrong of [null,[0,0,1]]){assert.notDeepEqual(owner,wrong);assert.throws(()=>check(coherent(ts,mutate(ts,a,end,wrong))));controls++;}
 }
}
console.log(JSON.stringify({configurations:rows.length,controls,passed:true}));
