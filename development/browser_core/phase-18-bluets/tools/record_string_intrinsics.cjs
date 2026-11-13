// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independently calibrate authored inputs/consumers before production changes.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os'),cp=require('node:child_process'),crypto=require('node:crypto');
const repo=path.resolve(__dirname,'../../../..'),directory=path.join(repo,'backend/bluets/tests/fixtures/string_intrinsics');
const resolve=require(path.join(repo,'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation=resolve(process.env.BLUEICE_BLUETSC_ORACLE,'typescript','lib/typescript.js'),ts=require(implementation);assert.equal(ts.version,'5.9.3');
const verify=process.argv[2]==='--verify';assert.deepEqual(process.argv.slice(2),verify?['--verify']:[]);
const inputPath=path.join(directory,'cases.json'),bytes=fs.readFileSync(inputPath),input=JSON.parse(bytes),referencePath=path.join(directory,'reference.json');
const frozenReference=verify?fs.readFileSync(referencePath):null;
if(!verify)assert(!fs.existsSync(referencePath),'never overwrite frozen expectations; use --verify to compare fresh observations');
const sha=value=>crypto.createHash('sha256').update(value).digest('hex'),utf8=value=>Buffer.from(value,'utf8').toString('utf8');
const capture=process.env.BLUEICE_STRING_INTRINSICS_OBSERVATIONS?path.join(path.resolve(process.env.BLUEICE_STRING_INTRINSICS_OBSERVATIONS),'native-'+process.pid):fs.mkdtempSync(path.join(os.tmpdir(),'bluets-string-intrinsics-native-'));fs.mkdirSync(capture,{recursive:true});
const libraries={},cache=new Map();
function diagnostic(d,root){let position=null,file=null;if(d.file&&d.start!==undefined){const p=d.file.getLineAndCharacterOfPosition(d.start);position={line:p.line+1,column:p.character+1,length:d.length};file=path.relative(root,d.file.fileName).replaceAll('\\','/');}return {code:d.code,message:utf8(ts.flattenDiagnosticMessageText(d.messageText,'\n')),file,position,relatedInformation:(d.relatedInformation||[]).map(value=>diagnostic(value,root))};}
function compile(root,files,options,emit){
 const host=ts.createCompilerHost(options),get=host.getSourceFile;
 host.getSourceFile=(file,version,...rest)=>{const relative=path.relative(path.dirname(implementation),file);if(relative.startsWith('..'+path.sep)||path.isAbsolute(relative))return get(file,version,...rest);const key=file+':'+version;if(!cache.has(key))cache.set(key,get(file,version,...rest));if(fs.existsSync(file))libraries[path.basename(file)]=sha(fs.readFileSync(file));return cache.get(key);};
 const output={};host.writeFile=(file,text)=>{const relative=path.relative(root,file).replaceAll('\\','/');assert(!relative.startsWith('../'));const data=Buffer.from(text,'utf8');output[relative]=data.toString('utf8');fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,data);};
 const program=ts.createProgram(files.map(file=>path.join(root,file)),options,host),errors=ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program));if(emit&&!errors.length)program.emit();
 return {accepts:errors.length===0,diagnostics:errors.map(d=>diagnostic(d,root)),output,selectedInputs:program.getSourceFiles().map(file=>path.relative(root,file.fileName).replaceAll('\\','/')).filter(file=>!file.startsWith('../')).sort()};
}
const observations=[],authoringErrors=[];let consumers=0,runtime=0;
for(const profile of input.profiles)for(const entry of input.cases){
 const id=profile.id+'/'+entry.id,root=path.join(capture,profile.id,entry.id);fs.mkdirSync(root,{recursive:true});fs.writeFileSync(path.join(root,'main.ts'),entry.source);
 for(const [name,text]of Object.entries(entry.files||{})){assert(!path.isAbsolute(name)&&!name.split(/[\\/]/).includes('..'));fs.mkdirSync(path.dirname(path.join(root,name)),{recursive:true});fs.writeFileSync(path.join(root,name),text);}
 const converted=ts.convertCompilerOptionsFromJson({...input.options,...profile.compilerOptions,outDir:'dist'},root);assert.deepEqual(converted.errors,[]);
 const native=compile(root,['main.ts',...Object.keys(entry.files||{})],converted.options,true),expected=entry.acceptsByProfile?entry.acceptsByProfile[profile.id]:entry.accepts;
 const declarations=Object.fromEntries(Object.entries(native.output).filter(([name])=>name.endsWith('.d.ts')).map(([name,text])=>[name.slice('dist/'.length),text]));
 const row={id,form:entry.id,profile:profile.id,accepts:native.accepts,diagnostics:native.diagnostics,declarations};
 if(native.accepts!==expected)authoringErrors.push({id,kind:'source',expected,actual:native.accepts,diagnostics:native.diagnostics});
 if(entry.consumer&&native.accepts){
  const consumerRoot=path.join(root,'consumer');fs.mkdirSync(consumerRoot);for(const [name,text]of Object.entries(declarations)){fs.mkdirSync(path.dirname(path.join(consumerRoot,name)),{recursive:true});fs.writeFileSync(path.join(consumerRoot,name),text);}
  row.consumers={};for(const kind of ['positive','negative']){
   fs.writeFileSync(path.join(consumerRoot,'consumer.ts'),entry.consumer[kind]);const roots=['consumer.ts',...Object.keys(declarations)].sort(),actual=compile(consumerRoot,roots,{...converted.options,noEmit:true},false);assert.deepEqual(actual.selectedInputs,roots,id+': consumer must select only declarations');
   row.consumers[kind]={accepts:actual.accepts,diagnostics:actual.diagnostics};consumers++;if(actual.accepts!==(kind==='positive'))authoringErrors.push({id,kind:'consumer-'+kind,actual:row.consumers[kind]});
  }
 }
 if(entry.runtime&&native.accepts){
  const esm=profile.compilerOptions.module==='ESNext',check=esm?'if(runtime!==2)throw Error("runtime result");':'if(module.exports.runtime!==2)throw Error("runtime result");';
  const result=cp.spawnSync(process.execPath,['--input-type='+(esm?'module':'commonjs')],{input:native.output['dist/main.js']+'\n'+check+'\n',encoding:'utf8',timeout:120000});if(result.error)throw result.error;row.runtime={exit:result.status,stdout:result.stdout,stderr:result.stderr};runtime++;if(result.status!==0)authoringErrors.push({id,kind:'runtime',actual:row.runtime});
 }
 fs.writeFileSync(path.join(root,'observation.json'),JSON.stringify(row,null,2)+'\n');observations.push(row);
}
const report={schema:1,typescript:ts.version,node:process.version,unicode:process.versions.unicode,icu:process.versions.icu,implementationSha256:sha(fs.readFileSync(implementation)),libraries,inputSha256:sha(bytes),cases:observations,forms:input.cases.length,profiles:input.profiles.length,comparisons:observations.length,accepted:observations.filter(row=>row.accepts).length,declarationFiles:observations.reduce((sum,row)=>sum+Object.keys(row.declarations).length,0),consumers,runtime,authoringErrors,conclusion:authoringErrors.length?'authoring-errors':'independently-calibrated'};
fs.writeFileSync(path.join(capture,'report.json'),JSON.stringify(report,null,2)+'\n');
if(authoringErrors.length){console.error(JSON.stringify({capture,errors:authoringErrors.length,first:authoringErrors.slice(0,16)},null,2));process.exitCode=1;}else{
 if(verify){
  // Node's executable version is provenance; Unicode/ICU, compiler bytes,
  // libraries, every diagnostic and every physical declaration still match.
  const {node:expectedNode,...expected}=JSON.parse(frozenReference);
  const {node:actualNode,...actual}=report;
  assert.deepEqual(actual,expected,'frozen Native string-intrinsic observations changed');
  assert.deepEqual(fs.readFileSync(referencePath),frozenReference,'verification must not change the frozen reference');
  fs.writeFileSync(path.join(capture,'verification.json'),JSON.stringify({schema:1,conclusion:'success',mode:'verify',expectedNode,actualNode,inputSha256:sha(bytes),referenceSha256:sha(frozenReference),reportSha256:sha(fs.readFileSync(path.join(capture,'report.json'))),comparisons:report.comparisons,accepted:report.accepted,declarationFiles:report.declarationFiles,consumers,runtime},null,2)+'\n',{flag:'wx'});
 }else fs.writeFileSync(referencePath,JSON.stringify(report,null,2)+'\n',{flag:'wx'});
 console.log(JSON.stringify({capture,mode:verify?'verify':'record',forms:report.forms,profiles:report.profiles,comparisons:report.comparisons,accepted:report.accepted,declarationFiles:report.declarationFiles,consumers,runtime,referenceSha256:sha(fs.readFileSync(referencePath))},null,2));
}
