// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Two public CLI builds retain every diagnostic, input and emitted artifact.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict'),{spawnSync}=require('node:child_process');
const graph=require('./ge5_inputs.cjs'),maps=require('./ge5_maps.cjs');
function collect(compiler,expectedHash,{form='ge1_downlevel/async-arrow-default-block'}={}) {
  const counts=new Map([['ge1_downlevel/async-arrow-default-block',27],['ge1_downlevel/async-arrow-lexical-this',51],['ge1_downlevel/async-generator-exception-template',27],['ge2_module_helpers/async-generator-finally',363],['ge2_module_helpers/helper-name-collision',363],['ge2_module_helpers/async-arrow-finally',363],['ge2_module_helpers/standard-decorator',363],['ge3_decorator_compositions/static-super-computed-reads',51],['ge3_decorator_compositions/static-super-simple-writes',43],['ge3_decorator_compositions/static-super-compound-logical',43],['ge3_decorator_compositions/static-super-prefix-postfix',43],['ge3_decorator_compositions/static-super-strict-failed-write',51],['ge3_decorator_compositions/static-super-arrows-nested-homeobjects',51]]);assert(counts.has(form),'closed async-arrow source family');
  const root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps');
  const inputBytes=fs.readFileSync(path.join(fixture,'cases.json')),referenceBytes=fs.readFileSync(path.join(fixture,'reference.json'));
  assert.equal(graph.sha256(inputBytes),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
  assert.equal(graph.sha256(referenceBytes),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
  const input=JSON.parse(inputBytes),reference=JSON.parse(referenceBytes),parents=graph.parents(root,input),sha=()=>graph.sha256(fs.readFileSync(compiler)),compilerSha256=sha();
  if(expectedHash)assert.equal(compilerSha256,expectedHash,'explicit immutable CLI capability');
  const temporary=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'ge5-async-arrow-')),observations=[];
  function outputs(directory,result={}){if(fs.existsSync(directory))for(const entry of fs.readdirSync(directory,{withFileTypes:true})){const name=path.join(directory,entry.name);if(entry.isDirectory())outputs(name,result);else result[path.relative(temporary,name).replace(/\\/g,'/').replace(/^\d+\//,'')]=fs.readFileSync(name,'utf8');}return result;}
  try {
    for(const [index,specification]of input.cases.entries())if(specification.form===form){
      const row=graph.resolve(input,specification,parents),native=reference.cases[index],directory=path.join(temporary,String(index));assert.deepEqual(native.diagnostics,[]);
      for(const [name,text]of Object.entries(row.files)){const file=path.join(directory,name);fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,text);}
      const configuration={compilerOptions:row.options,files:[row.entry]},raw=native.configuration?reference.blobs[native.configuration]:JSON.stringify(configuration,null,2)+'\n';assert.deepEqual(JSON.parse(raw),configuration);fs.writeFileSync(path.join(directory,'tsconfig.json'),raw);
      function build(){const run=spawnSync(compiler,['--project',path.join(directory,'tsconfig.json'),'--diagnostics-json','--listFiles'],{encoding:'utf8',maxBuffer:16*1024*1024});assert(!run.error,run.error?.message);const diagnostics=run.stderr.split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line));const selected=run.stdout.split(/\r?\n/).filter(Boolean).map(file=>path.relative(directory,file).replace(/\\/g,'/')).filter(name=>Object.hasOwn(row.files,name)).sort();return {status:run.status,diagnostics,selectedInputs:[...new Set(selected)],outputs:{...outputs(path.join(directory,'out')),...outputs(path.join(directory,'types'))}};}
      const first=build(),second=build();assert.deepEqual(second,first,native.id+': complete repeated public build');assert.equal(first.status,0,native.id);assert.deepEqual(first.diagnostics,[]);assert.deepEqual(first.selectedInputs,[...native.selectedInputs].sort());assert.deepEqual(Object.keys(first.outputs).sort(),Object.keys(native.outputs).sort());
      for(const [name,text]of Object.entries(first.outputs))if(/\.d\.(?:ts|mts|cts)(?:\.map)?$/.test(name)){const wanted=reference.blobs[native.outputs[name]];if(name.endsWith('.map'))assert.deepEqual(maps.decode(text,first.outputs[name.slice(0,-4)]),maps.decode(wanted,reference.blobs[native.outputs[name.slice(0,-4)]]),'complete declaration map '+name);else assert.equal(text,wanted,'raw declaration '+name);}
      observations.push({index,id:native.id,repeatBuilds:2,compilerSha256,files:row.files,options:row.options,outputs:first.outputs});
    }
    assert.equal(observations.length,counts.get(form));assert.equal(sha(),compilerSha256,'CLI remains immutable across the complete family');return observations;
  }finally{fs.rmSync(temporary,{recursive:true,force:true});}
}
module.exports={collect};
if(require.main===module){assert(process.argv[2],'explicit read-only CLI path required');process.stdout.write(JSON.stringify(collect(path.resolve(process.argv[2]),process.argv[3]))+'\n');}
