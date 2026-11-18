// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c=require('./contextual_member_results/common.cjs');
const maps=require('./ge5_maps.cjs');
const {spawnSync}=require('node:child_process'),{isDeepStrictEqual}=require('node:util');
const {fs,path,assert,sha}=c;
const compiler=fs.realpathSync(process.argv[2]),compilerSha256=sha(fs.readFileSync(compiler));
const input=c.read('cases.json'),reference=c.read('reference.json'),downInput=c.read('downstream-input.json'),down=c.read('downstream-reference.json');
const ctx=c.context(),frozen=c.authorities(ctx),root=c.capture();
const report={schema:1,compiler,compilerSha256,root,conclusion:'in_progress',cases:[],consumers:[],syntax:[],executions:[],differences:[]};
const expectedConsumers=new Map(down.consumers.map(row=>[row.id+'/'+row.polarity,row]));
const expectedRuntime=new Map(down.executions.map(row=>[row.id,row]));
const save=()=>{const tmp=path.join(root,'report.tmp');fs.writeFileSync(tmp,JSON.stringify(report,null,2)+'\n');fs.renameSync(tmp,path.join(root,'report.json'));};
function build(directory,surface,ownerKey) {
  const state=path.join(root,'owner-state',ownerKey);fs.mkdirSync(state,{recursive:true,mode:0o700});
  const args=surface==='native'?['--project','tsconfig.json','--diagnostics-json']:['build','--config','tsconfig.json','--diagnostics-json'];
  const result=spawnSync(compiler,args,{cwd:directory,encoding:'utf8',timeout:30000,env:{...process.env,BLUETSC_OWNER_STATE_DIR:state}});
  fs.writeFileSync(path.join(directory,'build.stdout'),result.stdout||'');fs.writeFileSync(path.join(directory,'build.stderr'),result.stderr||'');if(result.error)throw result.error;
  const diagnostics=(result.stderr||'').split(/\r?\n/).filter(Boolean).map(line=>{const row=JSON.parse(line);try{return c.blueDiagnostic(row);}catch{return{unmatchedBlueDiagnostic:row};}});
  return{accepts:result.status===0,exitCode:result.status,diagnostics};
}
function bounds(decoded,outputs,sourceFiles,name) {
  const raw=JSON.parse(outputs[name]);
  for(const segment of decoded.segments) {
    if(segment.length<5)continue;
    const id=path.basename(raw.sources[segment[2]].replaceAll('\\','/'));
    assert(Object.hasOwn(sourceFiles,id),'map refers to an unselected source');
    const lines=sourceFiles[id].split(/\r\n|\r|\n/);assert(segment[3]<lines.length&&segment[4]<=lines[segment[3]].length,'source map bounds');
  }
  assert.deepEqual(decoded.coordinateIssues,[],'complete generated map bounds');
}
function consumer(row,surface,profile,polarity,producer,engine,graph) {
  const directory=path.join(root,'consumers',surface,producer,engine,row.id,polarity),source=downInput.consumers[row.form][polarity];
  const options={...input.options,...profile.compilerOptions};c.consumerDirectory(directory,options,source,graph);
  const expected=expectedConsumers.get(row.id+'/'+polarity);assert(expected);
  let actual;
  if(engine==='native') {
    const converted=ctx.ts.convertCompilerOptionsFromJson(options,directory);assert.deepEqual(converted.errors,[]);
    actual=ctx.consumer(directory,converted.options,source,graph);
  } else {
    const result=build(directory,surface,['consumer',surface,producer,engine,row.id,polarity].join('/'));
    actual={accepts:result.accepts,diagnostics:result.diagnostics};
  }
  assert.deepEqual(actual,{accepts:expected.accepts,diagnostics:expected.diagnostics},row.id+': '+producer+'/'+engine+' '+polarity);
  fs.writeFileSync(path.join(directory,'observation.json'),JSON.stringify(actual,null,2)+'\n');
  report.consumers.push({id:row.id,surface,producer,engine,polarity,...actual});
}
try {
  save();
  for(const surface of ['native','legacy'])for(const wanted of reference.cases) {
    const profile=input.profiles.find(p=>p.id===wanted.profile),entry=input.cases.find(e=>e.id===wanted.form),representation=input.representations.find(r=>r.id===wanted.representation);
    const transformed={...entry,source:representation.prefix+entry.source,files:Object.fromEntries(Object.entries(entry.files||{}).map(([name,text])=>[name,representation.prefix+text]))};
    const prepared=c.prepare(path.join(root,'producers',surface,representation.id),input,profile,transformed);
    const actual=build(prepared.cwd,surface,['producer',surface,wanted.id].join('/'));
    const {outputs,ownerManifest,rawInventory}=c.compilerArtifacts(path.join(prepared.cwd,'dist'),surface,actual.accepts);
    const row={id:wanted.id,surface,...actual,ownerManifest,rawInventory,differences:[]};
    if(actual.accepts!==wanted.accepts)row.differences.push('verdict');
    if(!isDeepStrictEqual(actual.diagnostics,wanted.diagnostics))row.differences.push('complete-diagnostics');
    if(!isDeepStrictEqual(Object.keys(outputs).sort(),Object.keys(wanted.outputs).sort()))row.differences.push('complete-inventory');
    for(const[name,text]of Object.entries(wanted.outputs))if(!name.endsWith('.map')&&outputs[name]!==text)row.differences.push('exact-byte:'+name);
    if(actual.accepts&&wanted.accepts)for(const name of wanted.mapFiles) {
      try {
        const code=name.slice(0,-4),decoded=maps.decode(outputs[name],outputs[code]);
        bounds(decoded,outputs,{'main.ts':transformed.source,...transformed.files},name);
        if(!isDeepStrictEqual(decoded,maps.decode(wanted.outputs[name],wanted.outputs[code])))row.differences.push('complete-map:'+name);
      }catch(error){row.differences.push('invalid-map:'+name+':'+String(error));}
    }
    if(!wanted.accepts&&Object.keys(outputs).length)row.differences.push('rejected-artifacts');
    fs.writeFileSync(path.join(prepared.cwd,'observation.json'),JSON.stringify(row,null,2)+'\n');report.cases.push(row);
    if(row.differences.length)report.differences.push({id:wanted.id,surface,differences:row.differences});
    else if(wanted.accepts) {
      const nativeGraph=c.declarations(wanted.outputs),blueGraph=c.declarations(outputs);
      for(const polarity of ['positive','negative'])for(const producer of ['native','blue'])for(const engine of ['native','blue'])
        consumer(wanted,surface,profile,polarity,producer,engine,producer==='native'?nativeGraph:blueGraph);
      for(const[name,source]of Object.entries(outputs).filter(([name])=>name.endsWith('.js'))){c.syntax(ctx,source,profile);report.syntax.push({id:wanted.id,surface,file:name,javascriptSha256:sha(source)});}
      if(expectedRuntime.has(wanted.id)) {
        const expected=expectedRuntime.get(wanted.id),actual=c.runtime(path.join(root,'runtime',surface,wanted.id),wanted,profile,outputs,expected);
        assert.deepEqual(actual,{exit:expected.exit,stdout:expected.stdout,stderr:expected.stderr,runnerSha256:expected.runnerSha256},wanted.id+': unchanged execution');
        report.executions.push({id:wanted.id,surface,...actual});
      }
    }
    if(report.cases.length%20===0)save();
  }
  assert.equal(report.cases.length,960);assert.equal(report.differences.length,0,'complete contextual-member producer contract; every difference is retained in report.json');
  assert.equal(report.consumers.length,2*4*912);assert.equal(report.syntax.length,2*480);assert.equal(report.executions.length,2*64);
  const original=c.read('original-discovery.json'),originalReference=c.read('original-reference.json');
  report.originalReplay=[];
  for(const surface of ['native','legacy'])for(const wanted of originalReference.cases) {
    const entry=original.cases.find(e=>e.id===wanted.id),prepared=c.prepare(path.join(root,'original-replay',surface),original,originalReference.profile,entry);
    const actual=build(prepared.cwd,surface,['original-replay',surface,wanted.id].join('/'));
    const {outputs,ownerManifest,rawInventory}=c.compilerArtifacts(path.join(prepared.cwd,'dist'),surface,actual.accepts);
    assert.deepEqual({accepts:actual.accepts,diagnostics:actual.diagnostics,declarations:c.declarations(outputs)},
      {accepts:wanted.accepts,diagnostics:wanted.diagnostics,declarations:wanted.declarations},'unchanged original form '+wanted.id);
    report.originalReplay.push({id:wanted.id,surface,accepts:actual.accepts,ownerManifest,rawInventory});
  }
  assert.equal(report.originalReplay.length,40);c.authorities(ctx);assert.equal(sha(fs.readFileSync(compiler)),compilerSha256);
  report.conclusion='complete-contextual-member-contract-passed';save();
  console.log(JSON.stringify({root,compilerSha256,conclusion:report.conclusion,producers:960,consumerPrograms:report.consumers.length,syntax:report.syntax.length,executions:report.executions.length,originalReplay:40,separateLibraryControls:4}));
}catch(error){report.conclusion='failure';report.error=String(error.stack||error);save();console.error(JSON.stringify({root,conclusion:'failure',producers:report.cases.length,differences:report.differences.length,first:report.differences.slice(0,5),error:report.error}));process.exitCode=1;}
