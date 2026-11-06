// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Coherent attacks cover every frozen consumer/provider product against its complete contract.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{Worker,isMainThread,parentPort,workerData}=require('node:worker_threads');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs');
const root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps'),input=require(path.join(fixture,'cases.json')),reference=require(path.join(fixture,'reference.json')),parents=graph.parents(root,input),scope=isMainThread?(process.argv[2]??'classes'):workerData.scope,forms=(scope==='callable'?['imported-callable-alias']:scope==='namespace-reexport'?['namespace-reexported-class']:scope==='reexport'?['reexported-class']:scope==='string-enum'?['imported-string-enum']:scope==='numeric-enum'?['imported-numeric-enum']:scope==='inline-type'?['inline-import-type-class']:scope==='import-type'?['import-type-class']:scope==='nullable'?['imported-nullable-alias']:scope==='primitive'?['imported-primitive-alias']:scope==='interface'?['imported-interface']:scope==='local-alias'?['local-class-type-alias']:scope==='alias'?['imported-class-type-alias']:['imported-class','renamed-class','namespace-class','default-class']).map(name=>'ge3_decorator_compositions/'+name),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
assert(['callable','classes','alias','local-alias','interface','primitive','nullable','import-type','inline-type','numeric-enum','string-enum','reexport','namespace-reexport'].includes(scope),'complete fixed metadata family scope');const configurations=['local-alias','import-type','inline-type','numeric-enum','string-enum','reexport','namespace-reexport'].includes(scope)?43:['callable','alias','interface','primitive','nullable'].includes(scope)?41:172,maps=['reexport','namespace-reexport'].includes(scope)?129:['local-alias','import-type','inline-type','numeric-enum','string-enum','reexport','namespace-reexport'].includes(scope)?86:['callable','alias','interface','primitive','nullable'].includes(scope)?82:344,reportName=scope==='callable'?'metadata-callable-alias-controls':scope==='namespace-reexport'?'metadata-namespace-reexported-class-controls':scope==='reexport'?'metadata-reexported-class-controls':scope==='string-enum'?'metadata-string-enum-controls':scope==='numeric-enum'?'metadata-numeric-enum-controls':scope==='inline-type'?'metadata-inline-import-type-class-controls':scope==='import-type'?'metadata-import-type-class-controls':scope==='nullable'?'metadata-nullable-alias-controls':scope==='primitive'?'metadata-primitive-alias-controls':scope==='interface'?'metadata-imported-interface-controls':scope==='local-alias'?'metadata-local-class-type-alias-controls':scope==='alias'?'metadata-class-type-alias-controls':'metadata-class-controls';
assert.equal(graph.sha256(fs.readFileSync(path.join(fixture,'cases.json'))),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(fs.readFileSync(path.join(fixture,'reference.json'))),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
async function main() {
  const rows=JSON.parse(fs.readFileSync(0,'utf8')),selected=input.cases.flatMap((row,index)=>forms.includes(row.form)?[index]:[]),compilerSha256=graph.sha256(fs.readFileSync(path.join(root,'target/debug/bluetsc')));
  assert.deepEqual(rows.map(row=>row.index),selected);assert.equal(rows.length,configurations);
  const directory=process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,progress=directory&&path.join(directory,reportName+'-progress.jsonl');if(directory)fs.mkdirSync(directory,{recursive:true});if(progress)fs.writeFileSync(progress,'');
  const results=await Promise.all(Array.from({length:4},(_,worker)=>new Promise((resolve,reject)=>{
    const child=new Worker(__filename,{workerData:{rows:rows.filter((_,index)=>index%4===worker),compilerSha256,scope}});let result;
    child.on('message',message=>{if(message.kind==='result')result=message;else if(progress)fs.appendFileSync(progress,JSON.stringify(message)+'\n');});child.on('error',reject);child.on('exit',code=>code?reject(new Error('metadata controls worker exit '+code)):resolve(result));
  })));
  const products=results.flatMap(result=>result.products).sort((a,b)=>a.index-b.index||a.map.localeCompare(b.map)),failures=results.flatMap(result=>result.failures),controls=products.reduce((sum,row)=>sum+row.controls.length,0);
  const report={scope:'complete Native-derived consumer/provider contracts; coherent effective map and valid JavaScript controls',compilerSha256,configurations:rows.length,maps:products.length,controls,products,failures};
  if(directory)fs.writeFileSync(path.join(directory,reportName+'.json'),JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify({compilerSha256,configurations:rows.length,maps:products.length,controls,failures:failures.length,examples:failures.slice(0,4)},null,2));
  assert.equal(failures.length,0,'every effective complete-contract control is rejected');assert.equal(products.length,maps,'every frozen JavaScript map receives complete-contract controls');
}
function worker() {
  const products=[],failures=[];
  for(const row of workerData.rows) {
    const spec=input.cases[row.index],original=graph.resolve(input,spec,parents),expected=reference.cases[row.index];assert.equal(row.compilerSha256,workerData.compilerSha256);assert.equal(row.repeatBuilds,2);assert.deepEqual(row.options,original.options);assert.equal(row.id,expected.id);
    for(const[name,map]of Object.entries(expected.maps)) {
      if(map.kind!=='javascript')continue;
      try {
        const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name],n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,code,raw),filename=path.posix.relative(original.options.outDir,map.output).replace(/\.js$/,'.ts');assert(Object.hasOwn(original.files,filename));assert.deepEqual(a.decoded.metadata,n.decoded.metadata);
        const proof=filename===original.entry?require('./ge5_metadata_class_module.cjs').compare(ts,n,a,original.options,original.files,original.entry):require('./ge5_metadata_class_outputs.cjs').compare(ts,n,a,original.options,original.files[filename]);
        const controls=require('./ge5_metadata_class_mutations.cjs').run(ts,n,a,original.options,original.files,original.entry,filename,proof);
        products.push({index:row.index,map:name,module:filename,nativeJavaScriptSha256:expected.outputs[map.output],nativeStockMapSha256:map.raw,nativeMapSha256:graph.sha256(n.raw),actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw),controls});
        parentPort.postMessage({kind:'product',index:row.index,map:name,controls:controls.length});
      }catch(error){failures.push({index:row.index,map:name,error:error.message,stack:error.stack});parentPort.postMessage({kind:'failure',index:row.index,map:name,error:error.message});}
    }
  }
  parentPort.postMessage({kind:'result',products,failures});
}
if(isMainThread)main().catch(error=>{console.error(error.stack);process.exitCode=1;});else worker();
