// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('fs'),path=require('path'),cp=require('child_process'),assert=require('assert'),crypto=require('crypto');
const repository=path.resolve(__dirname,'../../../..'),support=path.join(repository,'backend/bluets/tests/fixtures/oracle_support');
const ts=require(path.join(support,'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn=require(path.join(support,'load_acorn.cjs'))();
const corpus=path.join(repository,'backend/bluets/tests/fixtures/source_extras');const reference=JSON.parse(fs.readFileSync(path.join(corpus,'reference.json')));const library=path.dirname(ts.getDefaultLibFilePath({})),cache=new Map();
assert.strictEqual(ts.version,'5.9.3');
const temporary=fs.realpathSync(fs.mkdtempSync(path.join(require('os').tmpdir(),'bluets-source-extras-')));
async function main(){const cases=[];for(const item of reference.cases){
 const originalDirectory=path.join(corpus,item.id),directory=path.join(temporary,item.id);fs.mkdirSync(directory);
 for(const name of Object.keys(item.sources)){const to=path.join(directory,name);fs.mkdirSync(path.dirname(to),{recursive:true});fs.copyFileSync(path.join(originalDirectory,name),to);}
 fs.copyFileSync(path.join(originalDirectory,'tsconfig.json'),path.join(directory,'tsconfig.json'));
 assert.deepStrictEqual(JSON.parse(fs.readFileSync(path.join(originalDirectory,'metadata.json'))),Object.fromEntries(['id','form','entry','rootFiles','runtimeModule','options'].map(k=>[k,item[k]])),item.id);
 const configPath=path.join(directory,'tsconfig.json');
 const config=JSON.parse(fs.readFileSync(configPath));assert.deepStrictEqual(config.compilerOptions,item.options,item.id);assert.deepStrictEqual(config.files,item.rootFiles,item.id);
 for(const[name,hash]of Object.entries(item.sources))assert.strictEqual(crypto.createHash('sha256').update(fs.readFileSync(path.join(directory,name))).digest('hex'),hash,item.id+':'+name);
 const parsed=ts.parseJsonSourceFileConfigFileContent(ts.readJsonConfigFile(configPath,ts.sys.readFile),ts.sys,directory,{},configPath);const host=ts.createCompilerHost(parsed.options),original=host.getSourceFile,outputs={};
 host.getSourceFile=(file,...args)=>{if(path.dirname(file)!==library)return original(file,...args);const key=file+JSON.stringify(args[0]);if(!cache.has(key))cache.set(key,original(file,...args));return cache.get(key);};
 host.writeFile=(file,text,bom)=>{const name=path.relative(directory,file).replaceAll('\\','/');outputs[name]=(bom?'\ufeff':'')+text;fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,outputs[name]);};
 const program=ts.createProgram(parsed.fileNames,parsed.options,host);const all=[...parsed.errors,...ts.getPreEmitDiagnostics(program)];const emitted=program.emit();for(const d of emitted.diagnostics)if(!all.some(old=>old.code===d.code&&old.start===d.start&&old.file===d.file))all.push(d);
 const diagnostics=all.map(d=>({code:d.code,file:d.file?path.relative(directory,d.file.fileName).replaceAll('\\','/'):null,start:d.start??null,length:d.length??null,message:ts.flattenDiagnosticMessageText(d.messageText,'\n').replaceAll(directory,'<root>')}));
 const row={...item,diagnostics,emitSkipped:emitted.emitSkipped,files:Object.keys(outputs).sort(),outputs,selectedInputs:program.getSourceFiles().map(s=>path.relative(directory,s.fileName).replaceAll('\\','/')).filter(name=>!name.startsWith('../')).sort(),observation:null,syntaxErrors:[]};
 if(diagnostics.length){assert.strictEqual(emitted.emitSkipped,true,item.id);assert.deepStrictEqual(row.files,[],item.id);}
 else {
  for(const [file,text] of Object.entries(outputs))if(file.endsWith('.js')){try{acorn.parse(text,{ecmaVersion:'latest',sourceType:item.runtimeModule==='CommonJS'?'script':'module'});}catch(e){row.syntaxErrors.push({file,message:String(e)});}const parsed=ts.createSourceFile(file,text,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);assert.deepStrictEqual(parsed.parseDiagnostics,[],item.id+':'+file);}
  assert.deepStrictEqual(row.syntaxErrors,[],item.id);
  const file=path.join(directory,'out',item.entry.replace(/\.(ts|js)$/,'.js'));
  const code=item.runtimeModule==='CommonJS'?'process.stdout.write(JSON.stringify(require(process.argv[1]).result))':'import(require("url").pathToFileURL(process.argv[1]).href).then(m=>process.stdout.write(JSON.stringify(m.result)))';
  const run=cp.spawnSync('node',['-e',code,file],{encoding:'utf8'});assert.strictEqual(run.status,0,item.id+':'+run.stderr);row.observation=JSON.parse(run.stdout);assert.deepStrictEqual(row.observation,item.form==='q3'?[42,7]:[42],item.id);
 }
 cases.push(row);
 }
 assert.strictEqual(cases.length,28);assert.strictEqual(cases.filter(c=>!c.diagnostics.length).length,18);
 const matrix=cases.map(c=>`${c.id}/${c.entry}\t${c.diagnostics.length?'reject':'accept'}\n`).join('');
 if(process.env.BLUEICE_WRITE_SOURCE_EXTRAS_MATRIX==='1'){fs.writeFileSync(path.join(corpus,'reference.json'),JSON.stringify({typescript:ts.version,cases},null,2)+'\n');fs.writeFileSync(path.join(corpus,'source-extras-checker-matrix.tsv'),matrix);}
 else{assert.deepStrictEqual({typescript:ts.version,cases},reference);assert.strictEqual(fs.readFileSync(path.join(corpus,'source-extras-checker-matrix.tsv'),'utf8').replaceAll('\r\n','\n'),matrix);}
 console.log(JSON.stringify({cases:cases.length,accepts:cases.filter(c=>!c.diagnostics.length).length,rejects:cases.filter(c=>c.diagnostics.length).length,actualNodeExecutions:cases.filter(c=>c.observation).length}));
}
main().catch(error=>{console.error(error);process.exitCode=1;}).finally(()=>fs.rmSync(temporary,{recursive:true,force:true}));
