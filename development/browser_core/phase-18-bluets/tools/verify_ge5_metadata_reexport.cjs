// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every frozen imported re-exported class product proves both complete modules and erased runtime dependence.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input),forms=['ge3_decorator_compositions/reexported-class'],ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
assert.equal(graph.sha256(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'))),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');assert.equal(graph.sha256(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'))),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
async function main() {
const rows=JSON.parse(fs.readFileSync(0,'utf8')),selected=input.cases.flatMap((row,index)=>forms.includes(row.form)?[index]:[]);assert.deepEqual(rows.map(row=>row.index),selected);assert.equal(rows.length,43);const compilerSha256=graph.sha256(fs.readFileSync(path.join(root,'target/debug/bluetsc'))),verified=[],failures=[],metadata=[],runtime=[],syntax=[];
for(const row of rows) {
  const spec=input.cases[row.index],original=graph.resolve(input,spec,parents),expected=reference.cases[row.index];assert.equal(row.compilerSha256,compilerSha256);assert.equal(row.repeatBuilds,2);assert.deepEqual(row.options,original.options);assert.equal(row.id,expected.id);assert.deepEqual(Object.keys(original.files).sort(),['barrel.ts','main.ts','types.ts']);
  for(const[name,map]of Object.entries(expected.maps)) {
    if(map.kind!=='javascript')continue;
    const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name];
    try {
      const n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,code,raw);assert.deepEqual(a.decoded.metadata,n.decoded.metadata);metadata.push({index:row.index,map:name,nativeJavaScriptSha256:expected.outputs[map.output],nativeStockMapSha256:map.raw,nativeMapSha256:graph.sha256(n.raw),actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw)});
      const filename=path.posix.relative(original.options.outDir,map.output).replace(/\.js$/,'.ts');assert(Object.hasOwn(original.files,filename));
      const proof=filename===original.entry?require('./ge5_metadata_class_module.cjs').compare(ts,n,a,row.options,original.files,original.entry):require('./ge5_metadata_class_outputs.cjs').compare(ts,n,a,row.options,original.files[filename]);
      verified.push({index:row.index,map:name,module:filename,classes:proof.classes.length,metadataConstructors:filename===original.entry?proof.delegated.graph.expected.length:0,...(filename===original.entry?{metadataProviderAcquired:proof.delegated.graph.runtime,originalReexportEdges:proof.delegated.graph.reexports.map(hop=>({file:hop.file,exported:hop.name,imported:hop.imported,target:hop.target})),originalEnumDeclarations:proof.delegated.graph.enums.map(node=>({kind:ts.SyntaxKind[node.kind],name:node.name.text,members:node.members.length})),builtinMetadata:[...new Set(proof.delegated.graph.expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).map(type=>type.builtin).filter(Boolean))]}:{enumScaffoldEndpoints:(proof.delegated.enumProofs??[]).reduce((sum,enumeration)=>sum+enumeration.pairs.length,0)})});
      for(const[producer,view]of[['Native',n],['Blue',a]])syntax.push({index:row.index,map:name,producer,codeSha256:graph.sha256(view.code),...require('./ge5_class_expression_syntax.cjs').parse(ts,view.code,row.options)});
    }catch(error){failures.push({index:row.index,id:row.id,map:name,error:error.message,stack:error.stack});}
  }
  try {
    const parent=parents[spec.origin.suite].byId.get(spec.origin.id),outputs=Object.fromEntries(Object.entries(expected.outputs).map(([name,hash])=>[name,reference.blobs[hash]])),rowForExecution={...parent,files:original.files,options:original.options},execute=require('./ge1_observations.cjs').execute,n=execute(rowForExecution,outputs),a=execute(rowForExecution,row.outputs);assert.equal(n.exit,0);assert.deepEqual(a,n,'complete actual metadata constructor identity/module side effects');runtime.push({index:row.index,executions:2,observation:a});
  }catch(error){failures.push({index:row.index,id:row.id,runtime:true,error:error.message,stack:error.stack});}
}
const providerRuntime=[];
for(const row of rows){const spec=input.cases[row.index],original=graph.resolve(input,spec,parents),expected=reference.cases[row.index],provider=Object.keys(original.files).find(name=>name!==original.entry),output=path.posix.join(original.options.outDir,provider.replace(/\.ts$/,'.js'));const n=await require('./ge5_metadata_enums.cjs').execute(ts,reference.blobs[expected.outputs[output]],row.options,original.files[provider]),a=await require('./ge5_metadata_enums.cjs').execute(ts,row.outputs[output],row.options,original.files[provider]);assert.deepEqual(a,n);providerRuntime.push({index:row.index,executions:2,enums:a});}
const reexportRuntime=[];
for(const row of rows){const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index],outputs=Object.fromEntries(Object.entries(expected.outputs).map(([name,hash])=>[name,reference.blobs[hash]])),n=await require('./ge5_metadata_reexport_runtime.cjs').execute(ts,outputs,row.options,original.files),a=await require('./ge5_metadata_reexport_runtime.cjs').execute(ts,row.outputs,row.options,original.files);assert.deepEqual(a,n);reexportRuntime.push({index:row.index,executions:2,observation:a});}
const report={scope:'complete Native-derived consumer/provider module roles, artifacts and execution',compilerSha256,configurations:rows.length,maps:metadata.length,repeatBuilds:2,metadata,verified,syntax,runtime,providerRuntime,reexportRuntime,failures};assert.equal(report.maps,129);
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'metadata-reexported-class.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify({compilerSha256,configurations:rows.length,maps:metadata.length,verified:verified.length,programs:syntax.length,runtime:runtime.length,failures:failures.length,examples:failures.slice(0,5)},null,2));
if(failures.length)process.exitCode=1;
else {
  const result=require('node:child_process').spawnSync(process.execPath,['--experimental-vm-modules',path.join(__dirname,'verify_ge5_metadata_class_controls.cjs'),'reexport'],{input:JSON.stringify(rows),encoding:'utf8',maxBuffer:16*1024*1024});
  process.stdout.write(result.stdout??'');process.stderr.write(result.stderr??'');assert.equal(result.status,0,'complete coherent effective metadata dependency controls');
}

}
main().catch(error=>{console.error(error.stack);process.exitCode=1;});
