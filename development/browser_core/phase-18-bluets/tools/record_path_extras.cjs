// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('fs'),path=require('path'),vm=require('vm'),assert=require('assert'),crypto=require('crypto');
const repository=path.resolve(__dirname,'../../../..'),support=path.join(repository,'backend/bluets/tests/fixtures/oracle_support');
const ts=require(path.join(support,'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn=require(path.join(support,'load_acorn.cjs'))();
const corpus=path.join(repository,'backend/bluets/tests/fixtures/path_extras');const reference=JSON.parse(fs.readFileSync(path.join(corpus,'reference.json')));const library=path.dirname(ts.getDefaultLibFilePath({})),cache=new Map();
assert.strictEqual(ts.version,'5.9.3');
const temporary=fs.mkdtempSync(path.join(require('os').tmpdir(),'bluets-path-extras-'));
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
 host.writeFile=(file,text,bom)=>outputs[path.relative(directory,file).replaceAll('\\','/')]=(bom?'\ufeff':'')+text;
 const program=ts.createProgram(parsed.fileNames,parsed.options,host);const all=[...parsed.errors,...ts.getPreEmitDiagnostics(program)];const emitted=program.emit();for(const d of emitted.diagnostics)if(!all.some(old=>old.code===d.code&&old.start===d.start&&old.file===d.file))all.push(d);
 const diagnostics=all.map(d=>({code:d.code,file:d.file?path.relative(directory,d.file.fileName).replaceAll('\\','/'):null,start:d.start??null,length:d.length??null,message:ts.flattenDiagnosticMessageText(d.messageText,'\n')}));
 const row={...item,diagnostics,emitSkipped:emitted.emitSkipped,files:Object.keys(outputs).sort(),outputs,selectedInputs:program.getSourceFiles().map(s=>path.relative(directory,s.fileName).replaceAll('\\','/')).filter(name=>!name.startsWith('../')).sort(),observation:null,syntaxError:null};
 if(diagnostics.length){assert.strictEqual(emitted.emitSkipped,true,item.id);assert.deepStrictEqual(row.files,[],item.id);}
 else{const file='out/'+item.entry.replace(/\.ts$/,'.js'),js=outputs[file];assert.ok(js,item.id+':entry');try{acorn.parse(js,{ecmaVersion:2022,sourceType:item.runtimeModule==='CommonJS'?'script':'module'});}catch(e){row.syntaxError=String(e);}assert.strictEqual(row.syntaxError,null,item.id);let exported;if(item.runtimeModule==='CommonJS'){const module={exports:{}};vm.runInNewContext(js,{module,exports:module.exports},{filename:item.id});exported=module.exports;}else exported=await import('data:text/javascript;base64,'+Buffer.from(js+'\n//# sourceURL='+item.id).toString('base64'));row.observation=JSON.parse(JSON.stringify(exported.result));assert.deepStrictEqual(row.observation,[42],item.id);}
 cases.push(row);
 }
 assert.strictEqual(cases.length,46);assert.strictEqual(cases.filter(c=>!c.diagnostics.length).length,38);
 const matrix=cases.map(c=>`${c.id}/${c.entry}\t${c.diagnostics.length?'reject':'accept'}\n`).join('');
 if(process.env.BLUEICE_WRITE_PATH_EXTRAS_MATRIX==='1'){fs.writeFileSync(path.join(corpus,'reference.json'),JSON.stringify({typescript:ts.version,cases},null,2)+'\n');fs.writeFileSync(path.join(corpus,'path-extras-checker-matrix.tsv'),matrix);}
 else{assert.deepStrictEqual({typescript:ts.version,cases},reference);assert.strictEqual(fs.readFileSync(path.join(corpus,'path-extras-checker-matrix.tsv'),'utf8').replaceAll('\r\n','\n'),matrix);}
 console.log(JSON.stringify({cases:cases.length,accepts:cases.filter(c=>!c.diagnostics.length).length,rejects:cases.filter(c=>c.diagnostics.length).length,actualNodeExecutions:cases.filter(c=>c.observation).length}));
}
main().catch(error=>{console.error(error);process.exitCode=1;}).finally(()=>fs.rmSync(temporary,{recursive:true,force:true}));
