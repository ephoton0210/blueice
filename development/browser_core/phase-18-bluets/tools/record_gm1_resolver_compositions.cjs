// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Only pinned TypeScript supplies G-M1 expectations; no BlueTS verdict input.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),support=path.join(root,'backend/bluets/tests/fixtures/oracle_support');
const ts=require(path.join(support,'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const acorn=require(path.join(support,'load_acorn.cjs'))();
const observe=require('./gm1_observations.cjs');
const directory=path.join(root,'backend/bluets/tests/fixtures/gm1_resolver_compositions');
const input=JSON.parse(fs.readFileSync(path.join(directory,'cases.json')));
assert.equal(ts.version,'5.9.3');assert.equal(acorn.version,'8.15.0');assert.equal(input.typescript,ts.version);
const libraries=new Map(),libRoot=path.dirname(ts.getDefaultLibFilePath({}));
const sorted=o=>Object.fromEntries(Object.entries(o).sort(([a],[b])=>a.localeCompare(b)));
function compile(base,row,consumer){
  const source=consumer?`import {result} from './out/${row.entry.replace(/\.(?:js|ts)$/i,'.js').replace(/\.mts$/i,'.mjs').replace(/\.cts$/i,'.cjs')}';\nconst value: ${consumer} = result;\n`:null;
  const consumerName='consumer.ts';
  if(source)observe.write(base,{[consumerName]:source});
  const config={compilerOptions:{...row.options,...(consumer?{noEmit:true,declaration:false}:{})},files:[consumer?consumerName:row.entry]};
  const configPath=path.join(base,'tsconfig.json');observe.write(base,{'tsconfig.json':JSON.stringify(config,null,2)+'\n'});
  const parsed=ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath,fs.readFileSync(configPath,'utf8')),ts.sys,base,{},configPath);
  const host=ts.createCompilerHost(parsed.options),read=host.getSourceFile,outputs={};
  host.getSourceFile=(file,...args)=>{
    if(path.dirname(file)!==libRoot)return read(file,...args);
    const key=file+JSON.stringify(args[0]);if(!libraries.has(key))libraries.set(key,read(file,...args));return libraries.get(key);
  };
  host.writeFile=(file,text,bom)=>{outputs[path.relative(base,file).replaceAll(path.sep,'/')]=(bom?'\ufeff':'')+text;};
  const program=ts.createProgram(parsed.fileNames,parsed.options,host);
  const describe=d=>({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,'\n').replaceAll(base,'<root>'),
    module:d.file?path.relative(base,d.file.fileName).replaceAll(path.sep,'/'):null,start:d.start??null,length:d.length??null,
    related:(d.relatedInformation||[]).map(describe)});
  const diagnostics=ts.sortAndDeduplicateDiagnostics([...parsed.errors,...ts.getPreEmitDiagnostics(program)]);
  if(!diagnostics.length&&!consumer)program.emit();
  const selectedInputs=program.getSourceFiles().map(s=>path.relative(base,s.fileName).replaceAll(path.sep,'/')).filter(s=>!s.startsWith('../')).sort();
  return {diagnostics:diagnostics.map(describe),selectedInputs,outputs:sorted(outputs)};
}
function declarations(outputs){return Object.fromEntries(Object.entries(outputs).filter(([name])=>/\.d\.(?:ts|mts|cts)$/.test(name)));}
async function main(){
  const temporary=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'bluets-gm1-native-')));
  try{
    const cases=[];
    for(const [index,row]of input.cases.entries()){
      const base=path.join(temporary,String(index));fs.mkdirSync(base);observe.write(base,row.files);
      const result=compile(base,row);
      if(!process.argv.includes('--probe'))assert.equal(!result.diagnostics.length,row.accepts,row.id+': '+JSON.stringify(result.diagnostics));
      const actual={id:row.id,...result,imports:observe.imports(ts,result.outputs),consumers:null,execution:null};
      if(!result.diagnostics.length){
        for(const [name,source]of Object.entries(result.outputs).filter(([n])=>/\.(?:js|mjs|cjs)$/.test(n)))acorn.parse(source,{ecmaVersion:2022,sourceType:name.endsWith('.cjs')?'script':name.endsWith('.mjs')?'module':row.runtimeModule==='CommonJS'?'script':'module'});
        observe.write(base,result.outputs);
        actual.consumers={accept:compile(base,row,'number').diagnostics,reject:compile(base,row,'string').diagnostics};
        assert.deepEqual(actual.consumers.accept,[],row.id+': accepted consumer');assert(actual.consumers.reject.length,row.id+': rejecting consumer');
        actual.execution=observe.execute(row,result.outputs);
        if(!process.argv.includes('--probe'))assert.equal(actual.execution.exit,0,row.id+': '+actual.execution.stderr);
      }
      cases.push(actual);
      if(process.argv.includes('--probe'))console.log(row.id+' '+JSON.stringify({codes:result.diagnostics.map(d=>d.code),selected:result.selectedInputs,execution:actual.execution}));
    }
    const reference={schema:1,typescript:ts.version,cases,ownerControls:input.ownerControls};
    if(process.env.BLUEICE_WRITE_GM1==='1')fs.writeFileSync(path.join(directory,'reference.json'),JSON.stringify(reference,null,2)+'\n');
    else if(!process.argv.includes('--probe'))assert.deepEqual(reference,JSON.parse(fs.readFileSync(path.join(directory,'reference.json'))));
    console.log(JSON.stringify({observations:cases.length,accepted:cases.filter(r=>!r.diagnostics.length).length,declarationGraphs:cases.filter(r=>!r.diagnostics.length).length,declarationFiles:cases.reduce((n,r)=>n+Object.keys(declarations(r.outputs)).length,0),executions:cases.filter(r=>r.execution?.exit===0).length}));
  }finally{fs.rmSync(temporary,{recursive:true,force:true});}
}
if(process.argv.includes('--outputs')){
  (async()=>{
    const actual=JSON.parse(fs.readFileSync(0,'utf8')),reference=JSON.parse(fs.readFileSync(path.join(directory,'reference.json'))).cases.filter(r=>!r.diagnostics.length);
    assert.equal(actual.length,reference.length);
    const temporary=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'bluets-gm1-consumer-')));
    try{for(let i=0;i<actual.length;i++){
      const a=actual[i],r=reference[i],row=input.cases.find(row=>row.id===r.id);assert.equal(a.id,r.id);
      assert.deepEqual(Object.keys(a.outputs).sort(),Object.keys(r.outputs).sort(),r.id+': artifacts');
      assert.deepEqual(sorted(declarations(a.outputs)),sorted(declarations(r.outputs)),r.id+': declaration bytes');
      assert.deepEqual(observe.imports(ts,a.outputs),r.imports,r.id+': imports');
      for(const [name,source]of Object.entries(a.outputs).filter(([n])=>/\.(?:js|mjs|cjs)$/.test(n)))acorn.parse(source,{ecmaVersion:2022,sourceType:name.endsWith('.cjs')?'script':name.endsWith('.mjs')?'module':row.runtimeModule==='CommonJS'?'script':'module'});
      const base=path.join(temporary,String(i));fs.mkdirSync(base);observe.write(base,row.files);observe.write(base,a.outputs);
      assert.deepEqual({accept:compile(base,row,'number').diagnostics,reject:compile(base,row,'string').diagnostics},r.consumers,r.id+': downstream diagnostics');
      assert.deepEqual(observe.execute(row,a.outputs),r.execution,r.id+': execution');
    }console.log(JSON.stringify({declarations:actual.length,consumers:actual.length*2,executions:actual.length}));}finally{fs.rmSync(temporary,{recursive:true,force:true});}
  })().catch(e=>{console.error(e);process.exitCode=1;});
}else main().catch(e=>{console.error(e);process.exitCode=1;});
