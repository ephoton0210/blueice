// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),selection=require('./ge5_class_cases.cjs');
const {stream}=require('./ge5_role_stream.cjs'),{compare}=require('./ge5_class_retained.cjs'),mutation=require('./ge5_class_mutations.cjs'),execution=require('./ge5_class_execution.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input);
const observations=JSON.parse(fs.readFileSync(0,'utf8')),identity=selection.verify(root,input,reference,observations,['ge1_downlevel/super-write-receiver-order']);
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const acorn=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'))(process.env.BLUEICE_ACORN_ORACLE||'acorn');
(async()=>{
  const verified=[],failures=[];
  for(const row of observations) {
    const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];assert.deepEqual(row.options,original.options);
    try {
      const maps=[];
      for(const [name,map]of Object.entries(expected.maps)) {
        if(map.kind!=='javascript')continue;
        const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString('utf8'):row.outputs[name];
        const native=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),actual=stream(ts,code,raw);
        const edition=Number(row.options.target.slice(2));assert(edition>=2015&&edition<=2022);
        for(const view of [native,actual])acorn.parse(view.code,{ecmaVersion:edition,sourceType:row.options.module==='ESNext'?'module':'script'});
        const proof=compare(ts,native,actual,row.options,original.files['main.ts']);
        const controls=mutation.run(ts,native,actual,row.options,original.files['main.ts'],proof,compare);
        maps.push({map:name,actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw),nativeJavaScriptSha256:expected.outputs[map.output],nativeMapSha256:graph.sha256(authority.raw(reference,expected,name)),allCharactersClassified:true,selectedEditionSyntax:true,controls});
      }
      const nativeOutputs=Object.fromEntries(Object.entries(expected.outputs).map(([name,hash])=>[name,reference.blobs[hash]]));
      const runtime=await execution.run(nativeOutputs,row.outputs,original.files,row.options);
      verified.push({index:row.index,id:row.id,maps,runtime});
    }catch(error){failures.push({index:row.index,id:row.id,error:error.message,stack:error.stack});}
  }
  const report={...identity,scope:'complete frozen super-write-receiver-order source family',configurations:observations.length,maps:verified.reduce((sum,row)=>sum+row.maps.length,0),controls:verified.reduce((sum,row)=>sum+row.maps.reduce((sum,map)=>sum+map.controls.length,0),0),executions:verified.reduce((sum,row)=>sum+row.runtime.executions,0),verified,failures};
  if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'class-super-write.json'),JSON.stringify(report,null,2)+'\n');}
  console.log(JSON.stringify(report,null,2));if(!failures.length){assert.equal(report.configurations,43);assert.equal(report.maps,43);assert.equal(report.executions,86);}else process.exitCode=1;
})().catch(error=>{console.error(error);process.exitCode=1;});
