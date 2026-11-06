// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Changed legacy completion transport preserves the complete adjacent wrapper ABI.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs');
const root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps'),input=require(path.join(fixture,'cases.json')),reference=require(path.join(fixture,'reference.json')),parents=graph.parents(root,input),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),rows=JSON.parse(fs.readFileSync(0,'utf8')),wanted=[4492,4493,4532,4533,4572,4573,4612,4613,4652,4653],products=[];assert.deepEqual(rows.map(row=>row.index),wanted);
for(const row of rows) {
  const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index],name='out/main.js.map',map=expected.maps[name],n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,row.outputs[map.output],row.outputs[name]),compare=require('./ge5_decorator_legacy_module.cjs').compare;
  const proof=compare(ts,n,a,original.options,original.files[original.entry]),phases=require('./ge5_decorator_roles.cjs').compare(ts,stream(ts,n.code,n.raw),stream(ts,a.code,a.raw),[original.files[original.entry]]);
  for(const pair of phases.statementPairs)assert.deepEqual(a.owner(pair.actual.end),n.owner(pair.native.end),'adjacent complete decorator statement logical endpoint');
  let controls=0;for(const item of proof.classes) {
    const offset=item.actual.node.end;for(const owner of[null,[0,0,1]])if(JSON.stringify(a.owner(offset))!==JSON.stringify(owner)) {
      const changed=coherent(ts,mutate(ts,a,offset,owner));assert.throws(()=>compare(ts,stream(ts,n.code,n.raw),changed,original.options,original.files[original.entry]));controls++;
    }
  }
  products.push({index:row.index,classes:proof.classes.length,controls,nativeJavaScriptSha256:expected.outputs[map.output],nativeMapSha256:graph.sha256(n.raw),actualJavaScriptSha256:graph.sha256(a.code),actualMapSha256:graph.sha256(a.raw)});
}
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'metadata-class-adjacent.json'),JSON.stringify({products},null,2)+'\n');}
console.log(JSON.stringify({configurations:rows.length,completeWrapperContracts:products.length,controls:products.reduce((sum,row)=>sum+row.controls,0)}));
