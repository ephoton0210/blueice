// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// All frozen standard-decorator products retain their complete public contracts.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),graph=require('./ge5_inputs.cjs');
const ts=require(root+'/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const {stream}=require('./ge5_role_stream.cjs'),authority=require('./ge5_native_authority.cjs'),roles=require('./ge5_standard_decorator_roles.cjs');
const input=require(root+'/backend/bluets/tests/fixtures/ge5_source_maps/cases.json'),reference=require(root+'/backend/bluets/tests/fixtures/ge5_source_maps/reference.json'),parents=graph.parents(root,input);
assert.equal(graph.sha256(fs.readFileSync(root+'/backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(fs.readFileSync(root+'/backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const compiler=process.argv[2],expectedHash=process.argv[3],compilerSha256=graph.sha256(fs.readFileSync(compiler));if(expectedHash)assert.equal(compilerSha256,expectedHash);
const observationPath=process.env.BLUEICE_GE5_EXISTING_OBSERVATIONS;
const observations=observationPath?JSON.parse(fs.readFileSync(observationPath)):require('./ge5_standard_decorator_observe.cjs').collect(compiler,compilerSha256);
const selected=input.cases.flatMap((row,index)=>row.form==='ge2_module_helpers/standard-decorator'?[index]:[]);assert.equal(selected.length,363);assert.deepEqual(observations.map(row=>row.index),selected);
const shardText=process.env.BLUEICE_GE5_STANDARD_SHARD,shard=shardText?shardText.split('/').map(Number):null;
if(shard)assert(shard.length===2&&shard.every(Number.isInteger)&&shard[1]===2&&shard[0]>=0&&shard[0]<shard[1],'exact two-worker whole-family partition');
const visited=observations.filter((row,index)=>!shard||index%shard[1]===shard[0]);
const dependencies=new Map();function tools(name){if(dependencies.has(name))return;const file=path.join(__dirname,name),bytes=fs.readFileSync(file);dependencies.set(name,graph.sha256(bytes));for(const match of bytes.toString().matchAll(/require\(['"]\.\/([^'"]+\.cjs)['"]\)/g))tools(match[1]);}
tools(path.basename(__filename));const toolHashes=Object.fromEntries([...dependencies].sort());
const verified=[],failures=[];
(async()=>{for(const row of visited){assert.equal(row.compilerSha256,compilerSha256);assert.equal(row.repeatBuilds,2);const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];assert.equal(row.id,expected.id);assert.deepEqual(row.files,original.files);assert.deepEqual(row.options,original.options);assert.deepEqual(Object.keys(row.outputs).sort(),Object.keys(expected.outputs).sort());
for(const[file,text]of Object.entries(row.outputs))if(/\.d\.(?:ts|mts|cts)(?:\.map)?$/.test(file)){const wanted=reference.blobs[expected.outputs[file]];if(file.endsWith('.map')){const maps=require('./ge5_maps.cjs');assert.deepEqual(maps.decode(text,row.outputs[file.slice(0,-4)]),maps.decode(wanted,reference.blobs[expected.outputs[file.slice(0,-4)]]),'complete declaration map');}else assert.equal(text,wanted,'raw declaration artifact');}
for(const[name,artifact]of Object.entries(expected.maps))if(artifact.kind==='javascript')try{
const code=row.outputs[artifact.output],raw=name.endsWith('#inline')?Buffer.from(code.split('base64,')[1],'base64').toString():row.outputs[name],ncode=reference.blobs[expected.outputs[artifact.output]],nraw=authority.raw(reference,expected,name),native=stream(ts,ncode,nraw),actual=stream(ts,code,raw);
const proof=roles.compare(ts,native,actual,row.options,row.files['main.ts']);
const controls=process.env.BLUEICE_GE5_POSITIVE_ONLY?[]:require('./ge5_standard_decorator_mutations.cjs').run(ts,native,actual,row.options,row.files['main.ts'],proof);
const nativeOutputs=Object.fromEntries(Object.entries(expected.outputs).map(([file,hash])=>[file,reference.blobs[hash]]));
const runtime=process.env.BLUEICE_GE5_POSITIVE_ONLY?null:await require('./ge5_standard_decorator_execution.cjs').run(ts,nativeOutputs,row.outputs,row.files,row.options);
verified.push({index:row.index,id:row.id,map:name,completeUTF16ModuleRoles:true,actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw),nativeJavaScriptSha256:expected.outputs[artifact.output],nativeMapSha256:graph.sha256(nraw),controls,runtime});
}catch(error){failures.push({index:row.index,id:row.id,map:name,error:error.message,stack:error.stack});}if(process.env.BLUEICE_GE5_STANDARD_PROGRESS)fs.writeFileSync(process.env.BLUEICE_GE5_STANDARD_PROGRESS,JSON.stringify({compilerSha256,shard,total:visited.length,verified:verified.length,failed:failures.length,lastIndex:row.index,controls:verified.reduce((sum,row)=>sum+row.controls.length,0),executions:verified.reduce((sum,row)=>sum+(row.runtime?.executions||0)+(row.runtime?.additionalFixtureExecutions||0),0),firstFailure:failures[0]})+'\n');}
assert.equal(graph.sha256(fs.readFileSync(compiler)),compilerSha256,'immutable compiler throughout whole family');
for(const[name,hash]of dependencies)assert.equal(graph.sha256(fs.readFileSync(path.join(__dirname,name))),hash,'immutable whole-family tool '+name);
const report={toolHashes,shard,visitedConfigurations:visited.length,family:'ge2_module_helpers/standard-decorator',configurations:observations.length,maps:verified.length+failures.length,compilerSha256,repeatBuilds:2,existingObservationsSha256:observationPath?graph.sha256(fs.readFileSync(observationPath)):null,completeFamily:!shard&&!process.env.BLUEICE_GE5_POSITIVE_ONLY&&failures.length===0,verified,failures};
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'standard-decorator.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify(report,null,2));if(failures.length)process.exitCode=1;
})().catch(error=>{console.error(error.stack);process.exitCode=1;});
