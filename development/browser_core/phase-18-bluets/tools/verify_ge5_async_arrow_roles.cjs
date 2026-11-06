// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A complete frozen family remains failing until every physical role closes.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),contract=require('./ge5_async_arrow_compare.cjs'),frames=require('./ge5_async_arrow_frames.cjs'),machine=require('./ge5_continuation_machine.cjs'),guard=require('./ge5_async_arrow_guard.cjs'),guardControls=require('./ge5_async_arrow_guard_mutations.cjs');
const root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps'),inputBytes=fs.readFileSync(path.join(fixture,'cases.json')),referenceBytes=fs.readFileSync(path.join(fixture,'reference.json'));
assert.equal(graph.sha256(inputBytes),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');assert.equal(graph.sha256(referenceBytes),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input=JSON.parse(inputBytes),reference=JSON.parse(referenceBytes),parents=graph.parents(root,input),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const observations=process.argv[2]?require('./ge5_async_arrow_observe.cjs').collect(path.resolve(process.argv[2]),process.argv[3]):JSON.parse(fs.readFileSync(0,'utf8'));
assert.deepEqual(observations.map(row=>row.index),input.cases.flatMap((row,index)=>row.form==='ge1_downlevel/async-arrow-default-block'?[index]:[]));
if(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY,'async-arrow-default-block.json'),JSON.stringify(observations)+'\n');}
(async()=>{const verified=[],failures=[],components=[];for(const row of observations){const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];assert.equal(row.id,expected.id);assert.deepEqual(row.options,original.options);assert.deepEqual(row.files,original.files);assert.equal(row.repeatBuilds,2);
  for(const[name,map]of Object.entries(expected.maps))if(map.kind==='javascript'){
    const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name],views=()=>[stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),stream(ts,code,raw)];
    let runtime,physicalRoles=[];const[n,a]=views(),np=contract.read(ts,n,row.options,original.files['main.ts'],false),ap=contract.read(ts,a,row.options,original.files['main.ts'],true);
    runtime=await require('./ge5_callable_execution.cjs').run(ts,n,a,row.options,{native:np,actual:ap});
    if(!np.retained)physicalRoles=frames.audit(ts,n,np,a,ap);
    const component={id:row.id,runtime,physicalRoles};
    if(row.options.target==='ES5'){
      function check(actual,suppliedNative){const native=suppliedNative??views()[0],nn=contract.read(ts,native,row.options,original.files['main.ts'],false),aa=contract.read(ts,actual,row.options,original.files['main.ts'],true),nm=machine.read(ts,native,nn.callback,'native',[original.files['main.ts']],{bodyStatements:nn.callback.body.statements.filter(node=>node!==nn.defaultGuard)}),am=machine.read(ts,actual,aa.callback,'blue',[original.files['main.ts']]);return guard.read(ts,native,nn,actual,aa,nm,am,aa.env.exact);}
      const actual=views()[1],proof=check(actual);component.guard={complete:true,controls:guardControls.run(ts,actual,proof,check),prologue:require('./ge5_async_arrow_prologue_mutations.cjs').run(ts,native=>check(views()[1],native),row.options,original.files['main.ts'])};
    }
    components.push(component);
    try{const[native,actual]=views(),proof=contract.compare(ts,native,actual,row.options,original.files['main.ts']),controls=require('./ge5_callable_mutations.cjs').run(ts,native,actual,row.options,original.files['main.ts'],proof,(ts,n,a,o,s)=>contract.compare(ts,n,guardControls.coherent(ts,a),o,s));controls.push(...require('./ge5_async_arrow_publication_mutations.cjs').run(ts,native,actual,row.options,original.files['main.ts'],changed=>contract.compare(ts,native,changed,row.options,original.files['main.ts'])));verified.push({id:row.id,map:name,runtime,controls,allCharactersClassified:true,actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw)});}
    catch(error){failures.push({id:row.id,map:name,error:error.message,stack:error.stack,physicalRoles});}
  }
}
const report={form:'async-arrow-default-block',configurations:observations.length,maps:verified.length+failures.length,repeatBuilds:2,compilerSha256:observations[0].compilerSha256,completeFamily:failures.length===0,verified,failures,components};assert(observations.every(row=>row.compilerSha256===report.compilerSha256));if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'async-arrow-default-block.json'),JSON.stringify(report,null,2)+'\n');}console.log(JSON.stringify(report,null,2));if(failures.length)process.exitCode=1;
})().catch(error=>{console.error(error);process.exitCode=1;});
