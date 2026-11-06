// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Full frozen expression-family collection; divergent regions require a complete Native IR.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream,compareTokens,compareGap}=require('./ge5_role_stream.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input);
const forms=new Map(['class-expression-named','class-expression-anonymous','class-expression-per-evaluation','class-expression-computed-heritage'].map(name=>['ge3_decorator_compositions/'+name,[35,35]]));
assert.equal(graph.sha256(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'))),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'))),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const rows=JSON.parse(fs.readFileSync(0,'utf8')),selected=input.cases.flatMap((row,index)=>forms.has(row.form)?[index]:[]);assert.deepEqual(rows.map(row=>row.index),selected);assert.equal(rows.length,140);
const compilerSha256=graph.sha256(fs.readFileSync(path.join(root,'target/debug/bluetsc')));
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
(async()=>{const verified=[],failures=[],runtime=[],prelude=[],mainFrames=[],controls=[],syntax=[];
for(const row of rows) {
  const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];assert.equal(row.compilerSha256,compilerSha256);assert.equal(row.repeatBuilds,2);assert.equal(row.id,expected.id);assert.deepEqual(row.options,original.options);
  for(const [name,map]of Object.entries(expected.maps)) {
    if(map.kind!=='javascript')continue;
    try {
      const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString('utf8'):row.outputs[name];
      const native=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),actual=stream(ts,code,raw);
      assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
      const whole=require('./ge5_class_expression_module.cjs').compare(ts,native,actual,row.options,original.files['main.ts']),preludeProof=whole.delegated.prelude;
      prelude.push({index:row.index,map:name,completeSourceCallables:preludeProof.proofs.length,completeClassValues:preludeProof.classes.length,deferredDecoratedCallables:preludeProof.deferred.length,nativeJavaScriptSha256:graph.sha256(native.code),nativeMapSha256:graph.sha256(native.raw),actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw)});
      if(whole.delegated.main)mainFrames.push({index:row.index,map:name,completeClassExpression:true});
      for(const [producer,view]of[['Native',native],['Blue',actual]])syntax.push({index:row.index,map:name,producer,codeSha256:graph.sha256(view.code),...require('./ge5_class_expression_syntax.cjs').parse(ts,view.code,row.options)});
      const attacks=require('./ge5_class_expression_mutations.cjs').run(ts,native,actual,row.options,original.files['main.ts'],whole);controls.push({index:row.index,map:name,controls:attacks});
      verified.push({index:row.index,map:name});
    }catch(error){failures.push({index:row.index,id:row.id,map:name,error:error.message,stack:error.stack});}
  }
  try {
    const nativeOutputs=Object.fromEntries(Object.entries(expected.outputs).map(([name,hash])=>[name,reference.blobs[hash]])),specification=input.cases[row.index],parent=parents[specification.origin.suite].byId.get(specification.origin.id);
    runtime.push({index:row.index,...require('./ge5_class_expression_execution.cjs').run(ts,parent,original,nativeOutputs,row.outputs)});
  }catch(error){failures.push({index:row.index,id:row.id,runtime:true,error:error.message,stack:error.stack});}
}
const report={compilerSha256,forms:Object.fromEntries(forms),configurations:rows.length,maps:verified.length+failures.filter(row=>row.map).length,repeatBuilds:2,verified,prelude,mainFrames,syntax,controls,runtime,failures};assert.equal(report.maps,140);
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'class-expressions.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify(report,null,2));if(failures.length)process.exitCode=1;
})().catch(error=>{console.error(error);process.exitCode=1;});
