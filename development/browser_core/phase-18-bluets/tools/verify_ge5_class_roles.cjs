// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs');
const {stream}=require('./ge5_role_stream.cjs'),{compare}=require('./ge5_class_module.cjs');
const root=path.resolve(__dirname,'../../../..'),directory=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes=fs.readFileSync(path.join(directory,'cases.json')),referenceBytes=fs.readFileSync(path.join(directory,'reference.json'));
assert.equal(graph.sha256(inputBytes),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input=JSON.parse(inputBytes),reference=JSON.parse(referenceBytes),parents=graph.parents(root,input);
const forms=new Map([['ge1_downlevel/called-imported-heritage',[51,102]],['ge1_downlevel/indexed-heritage',[51,51]],['ge1_downlevel/computed-constructor-spread',[51,51]],['ge1_downlevel/constructor-and-super-spread',[51,51]],['ge1_downlevel/computed-super-call-order',[51,51]],['ge1_downlevel/super-write-receiver-order',[43,43]],['ge2_module_helpers/inherit-constructor-spread',[363,363]]]);
const selected=input.cases.flatMap((row,index)=>forms.has(row.form)?[index]:[]);
const observations=JSON.parse(fs.readFileSync(0,'utf8'));assert.deepEqual(observations.map(row=>row.index),selected);assert.equal(selected.length,661);
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const compilerSha256=graph.sha256(fs.readFileSync(path.join(root,'target/debug/bluetsc'))),verified=[],failures=[],mutations=[];
for(const row of observations) {
  const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];
  assert.equal(row.id,expected.id);assert.equal(row.compilerSha256,compilerSha256);assert.equal(row.repeatBuilds,2);assert.deepEqual(row.options,original.options);
  for(const [name,map]of Object.entries(expected.maps)) {
    if(map.kind!=='javascript')continue;
    try {
      const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString('utf8'):row.outputs[name];
      const native=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),actual=stream(ts,code,raw);
      assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
      if(native.code===actual.code)assert.deepEqual(actual.decoded,native.decoded,'every decoded entry for identical JavaScript');
      const sourceName=JSON.parse(raw).sources[0].replace(/^.*\//,'');
      const proof=compare(ts,native,actual,row.options,original.files[sourceName]);
      const controls=require('./ge5_class_mutations.cjs').run(ts,native,actual,row.options,original.files[sourceName],proof,compare);
      mutations.push({index:row.index,id:row.id,map:name,controls});
      verified.push({index:row.index,id:row.id,map:name,allCharactersClassified:true,
        nativeJavaScriptSha256:expected.outputs[map.output],nativeStockMapSha256:map.raw,
        nativeMapSha256:graph.sha256(native.raw),actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw)});
    }catch(error){failures.push({index:row.index,id:row.id,map:name,error:error.message,stack:error.stack});}
  }
}
const report={schema:1,complete:failures.length===0,configurations:observations.length,maps:verified.length+failures.length,compilerSha256,repeatBuilds:2,forms:Object.fromEntries(forms),verified,mutations,failures};
assert.equal(report.maps,712);
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'class-super.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify(report,null,2));if(failures.length)process.exitCode=1;
