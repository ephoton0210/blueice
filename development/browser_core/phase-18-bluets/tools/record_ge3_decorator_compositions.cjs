// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native-only authority for the finite decorator and metadata compositions.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),support=path.join(root,'backend/bluets/tests/fixtures/oracle_support');
const ts=require(path.join(support,'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const acorn=require(path.join(support,'load_acorn.cjs'))();
const observe=require('./ge1_observations.cjs');
const directory=path.join(root,'backend/bluets/tests/fixtures/ge3_decorator_compositions');
const input=JSON.parse(fs.readFileSync(path.join(directory,'cases.json')));
assert.equal(ts.version,'5.9.3');assert.equal(acorn.version,'8.15.0');
const libraries=new Map(),libRoot=path.dirname(ts.getDefaultLibFilePath({}));
const sorted=o=>Object.fromEntries(Object.entries(o).sort(([a],[b])=>a.localeCompare(b)));
const declarations=o=>Object.fromEntries(Object.entries(o).filter(([n])=>/\.d\.(?:ts|mts|cts)$/.test(n)));
function compile(base,row,consumer){
  if(consumer)observe.write(base,{'consumer.ts':row[consumer]});
  const config={compilerOptions:{...row.options,...(consumer?{noEmit:true,rootDir:'.'}:{})},files:[consumer?'consumer.ts':row.entry]};
  const file=path.join(base,'tsconfig.json');observe.write(base,{'tsconfig.json':JSON.stringify(config,null,2)+'\n'});
  const parsed=ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(file,fs.readFileSync(file,'utf8')),ts.sys,base,{},file);
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
  let diagnostics=ts.sortAndDeduplicateDiagnostics([...parsed.errors,...ts.getPreEmitDiagnostics(program)]);
  if(!diagnostics.length&&!consumer){const emitted=program.emit();diagnostics=ts.sortAndDeduplicateDiagnostics([...diagnostics,...emitted.diagnostics]);}
  const selectedInputs=program.getSourceFiles().map(s=>path.relative(base,s.fileName).replaceAll(path.sep,'/')).filter(s=>!s.startsWith('../')).sort();
  return {diagnostics:diagnostics.map(describe),selectedInputs,outputs:sorted(outputs)};
}
function syntax(row,outputs){
  const edition=row.options.target==='ESNext'?'latest':row.options.target==='ES5'?5:Number(row.options.target.slice(2));
  for(const [name,source]of Object.entries(outputs).filter(([n])=>/\.(?:js|mjs|cjs)$/.test(n)))
    acorn.parse(source,{ecmaVersion:edition,sourceType:row.runtimeModule==='CommonJS'?'script':'module'});
}
function preserved(outputs){
  const classes=[];
  function nodeShape(node){const children=[];ts.forEachChild(node,child=>{children.push(nodeShape(child));});return {kind:ts.SyntaxKind[node.kind],...(node.text!==undefined?{text:node.text}:{}),children};}
  for(const [name,text]of Object.entries(outputs).filter(([n])=>/\.(?:js|mjs|cjs)$/.test(n))){
    const source=ts.createSourceFile(name,text,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);assert.deepEqual(source.parseDiagnostics,[],name);
    function visit(node){if(ts.isClassLike(node)&&((ts.getDecorators(node)||[]).length||node.members.some(m=>(ts.getDecorators(m)||[]).length||m.modifiers?.some(n=>n.kind===ts.SyntaxKind.AccessorKeyword)))){classes.push({module:name,ast:nodeShape(node)});}else ts.forEachChild(node,visit);}visit(source);
  }return classes;
}
async function main(){
  const temporary=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'bluets-ge3-native-')));
  try{
    const cases=[];
    for(const [index,row]of input.cases.entries()){
      if(process.env.GE3_PROBE_FORM && row.form!==process.env.GE3_PROBE_FORM)continue;
      const base=path.join(temporary,String(index));fs.mkdirSync(base);observe.write(base,row.files);
      const result=compile(base,row);
      const actual={id:row.id,...result,imports:observe.imports(ts,result.outputs),consumers:null,execution:null};
      if(!process.argv.includes('--probe'))assert.equal(!result.diagnostics.length,row.accepts,row.id+': '+JSON.stringify(result.diagnostics));
      if(!result.diagnostics.length){
        observe.write(base,result.outputs);
        actual.consumers={accept:compile(base,row,'consumerAccept').diagnostics,reject:compile(base,row,'consumerReject').diagnostics};
        actual.preservedSyntax=row.preservedSyntax?preserved(result.outputs):null;
        if(row.preservedSyntax){
          assert(actual.preservedSyntax.length,row.id+': no preserved class');
          const adapted=Object.fromEntries(Object.entries(result.outputs).map(([name,text])=>[name,/\.(?:js|mjs|cjs)$/.test(name)?ts.transpileModule(text,{fileName:name,compilerOptions:{target:ts.ScriptTarget.ES2022,module:row.runtimeModule==='CommonJS'?ts.ModuleKind.CommonJS:ts.ModuleKind.ESNext,experimentalDecorators:row.mode==='legacy',newLine:ts.NewLineKind.LineFeed}}).outputText:text]));
          actual.proposalExecution={adapter:'typescript@5.9.3 emitted-JavaScript-only ES2022 lowering',...observe.execute(row,adapted)};
        }
        else { try { syntax(row,result.outputs); actual.syntaxError=null; } catch(error) { actual.syntaxError=error.name+': '+error.message; }
          actual.execution=observe.execute(row,result.outputs);
        }
        if(!process.argv.includes('--probe')){
          assert.deepEqual(actual.consumers.accept,[],row.id);assert(actual.consumers.reject.length,row.id);
          if(row.nativeRuntimeDefect){assert(actual.execution.exit!==0&&/^(?:SyntaxError|TypeError):/.test(actual.execution.stderr),row.id+': missing frozen native defect');}
          else if(!row.preservedSyntax){assert.equal(actual.syntaxError,null,row.id);assert.equal(actual.execution.exit,0,row.id+': '+actual.execution.stderr);}
        }
      }
      cases.push(actual);
      if(process.argv.includes('--probe'))console.log(row.id+' '+JSON.stringify({codes:result.diagnostics.map(d=>d.code),diagnostics:result.diagnostics,consumer:actual.consumers?.accept,syntax:actual.syntaxError,execution:actual.execution}));
    }
    const reference={schema:1,typescript:ts.version,cases,ownerControls:input.ownerControls};
    if(process.argv.includes('--probe-record'))assert(input.cases.every(r=>r.accepts===null),'probe-record cannot rewrite a frozen authority');
    if(process.env.BLUEICE_WRITE_GE3==='1'||process.argv.includes('--probe-record')){
      assert(!process.env.GE3_PROBE_FORM,'cannot publish a filtered native authority');
      assert(!process.argv.includes('--probe')||process.argv.includes('--probe-record'),'probe is not a recording run');
      fs.writeFileSync(path.join(directory,'reference.json'),JSON.stringify(reference,null,2)+'\n');
    }else if(!process.argv.includes('--probe'))assert.deepEqual(reference,JSON.parse(fs.readFileSync(path.join(directory,'reference.json'))));
    console.log(JSON.stringify({observations:cases.length,accepted:cases.filter(r=>!r.diagnostics.length).length,declarationFiles:cases.reduce((n,r)=>n+Object.keys(declarations(r.outputs)).length,0),executions:cases.filter(r=>r.execution?.exit===0).length}));
  }finally{fs.rmSync(temporary,{recursive:true,force:true});}
}
if(process.argv.includes('--outputs')){
  (async()=>{
    const actual=JSON.parse(fs.readFileSync(0,'utf8')),reference=JSON.parse(fs.readFileSync(path.join(directory,'reference.json'))).cases.filter(r=>!r.diagnostics.length);
    assert.equal(actual.length,reference.length);
    const temporary=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'bluets-ge3-consumers-'))),failures=[];
    try{for(let i=0;i<actual.length;i++){
      const a=actual[i],r=reference[i],row=input.cases.find(row=>row.id===r.id);assert.equal(a.id,r.id);try{
      assert.deepEqual(Object.keys(a.outputs).sort(),Object.keys(r.outputs).sort(),r.id+': artifacts');
      assert.deepEqual(sorted(declarations(a.outputs)),sorted(declarations(r.outputs)),r.id+': declaration bytes');
      assert.deepEqual(observe.imports(ts,a.outputs),r.imports,r.id+': imports');if(row.preservedSyntax)assert.deepEqual(preserved(a.outputs),r.preservedSyntax,r.id+': preserved class AST');else syntax(row,a.outputs);
      const base=path.join(temporary,String(i));fs.mkdirSync(base);observe.write(base,row.files);observe.write(base,a.outputs);
      assert.deepEqual({accept:compile(base,row,'consumerAccept').diagnostics,reject:compile(base,row,'consumerReject').diagnostics},r.consumers,r.id+': consumers');
      if(!row.preservedSyntax){const authority=row.safeExecutionReference?reference.find(peer=>peer.id===row.safeExecutionReference):r;assert(authority?.execution?.exit===0,r.id+': missing safe native execution');assert.deepEqual(observe.execute(row,a.outputs),authority.execution,r.id+': actual completion/stdout/stderr');}
      if(row.form==='literal-auto-accessors'&&!row.preservedSyntax){
        function descriptors(outputs){
          const copy={...outputs},name='out/main.js',saved='out/ge3-descriptor-original.js';assert(!(saved in copy));copy[saved]=copy[name];
          const proof='[typeof Object.getOwnPropertyDescriptor(C.prototype, "a b").get, typeof Object.getOwnPropertyDescriptor(C.prototype, "a b").set, Object.prototype.hasOwnProperty.call(new C(), "a b"), typeof Object.getOwnPropertyDescriptor(C, "32").get, typeof Object.getOwnPropertyDescriptor(C, "32").set]';
          copy[name]=row.runtimeModule==='CommonJS'?'const C=require("./ge3-descriptor-original.js").C; exports.result='+proof+';':'import {C} from "./ge3-descriptor-original.js"; export const result='+proof+';';
          return observe.execute(row,copy);
        }
        assert.deepEqual(descriptors(a.outputs),descriptors(r.outputs),r.id+': actual accessor descriptors/private storage');
      }
      if(row.preservedSyntax){const adapted=Object.fromEntries(Object.entries(a.outputs).map(([name,text])=>[name,/\.(?:js|mjs|cjs)$/.test(name)?ts.transpileModule(text,{fileName:name,compilerOptions:{target:ts.ScriptTarget.ES2022,module:row.runtimeModule==='CommonJS'?ts.ModuleKind.CommonJS:ts.ModuleKind.ESNext,experimentalDecorators:row.mode==='legacy',newLine:ts.NewLineKind.LineFeed}}).outputText:text]));assert.deepEqual({adapter:r.proposalExecution.adapter,...observe.execute(row,adapted)},r.proposalExecution,r.id+': emitted-JavaScript-only proposal adapter');}
    }catch(error){failures.push(r.id+': '+error.message);}}assert.deepEqual(failures,[],failures.length+' G-E3 output differences');console.log(JSON.stringify({observations:actual.length,consumers:actual.length*2,directExecutions:reference.filter(r=>r.execution).length,preservedClassAsts:reference.filter(r=>r.preservedSyntax).length,proposalAdapterSuccesses:reference.filter(r=>r.proposalExecution?.exit===0).length,proposalAdapterNativeErrors:reference.filter(r=>r.proposalExecution&&r.proposalExecution.exit!==0).length}));}finally{fs.rmSync(temporary,{recursive:true,force:true});}
  })().catch(e=>{console.error(e);process.exitCode=1;});
}else main().catch(e=>{console.error(e);process.exitCode=1;});
