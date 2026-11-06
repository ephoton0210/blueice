// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// All frozen products close their field, capture, continuation and module roles.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),contract=require('./ge5_lexical_arrow_roles.cjs');
const root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps'),inputBytes=fs.readFileSync(path.join(fixture,'cases.json')),referenceBytes=fs.readFileSync(path.join(fixture,'reference.json'));
assert.equal(graph.sha256(inputBytes),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');assert.equal(graph.sha256(referenceBytes),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input=JSON.parse(inputBytes),reference=JSON.parse(referenceBytes),parents=graph.parents(root,input),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),observations=process.argv[2]?require('./ge5_lexical_arrow_observe.cjs').collect(path.resolve(process.argv[2]),process.argv[3]):JSON.parse(fs.readFileSync(0,'utf8'));
assert.deepEqual(observations.map(row=>row.index),input.cases.flatMap((row,index)=>row.form==='ge1_downlevel/async-arrow-lexical-this'?[index]:[]));assert.equal(observations.length,51);
if(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY,'async-arrow-lexical-this.json'),JSON.stringify(observations)+'\n');}
(async()=>{const verified=[],failures=[];for(const row of observations){const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];assert.equal(row.id,expected.id);assert.deepEqual(row.options,original.options);assert.deepEqual(row.files,original.files);assert.equal(row.repeatBuilds,2);
  for(const[name,map]of Object.entries(expected.maps))if(map.kind==='javascript')try{
    const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name],native=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),actual=stream(ts,code,raw),proof=contract.compare(ts,native,actual,row.options,original.files['main.ts']);
    const controls=require('./ge5_lexical_arrow_mutations.cjs').run(ts,native,actual,row.options,original.files['main.ts'],proof,contract.compare),runtime=await require('./ge5_lexical_arrow_execution.cjs').run(ts,native,actual,row.options,proof);
    verified.push({id:row.id,map:name,controls,runtime,allCharactersClassified:true,actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw)});
  }catch(error){failures.push({id:row.id,map:name,error:error.message,stack:error.stack});}
}
const report={form:'async-arrow-lexical-this',configurations:observations.length,maps:verified.length+failures.length,repeatBuilds:2,compilerSha256:observations[0].compilerSha256,completeFamily:failures.length===0,verified,failures};assert(observations.every(row=>row.compilerSha256===report.compilerSha256));if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'async-arrow-lexical-this.json'),JSON.stringify(report,null,2)+'\n');}console.log(JSON.stringify(report,null,2));if(failures.length)process.exitCode=1;
})().catch(error=>{console.error(error);process.exitCode=1;});
