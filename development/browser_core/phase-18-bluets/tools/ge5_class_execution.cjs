// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute complete emitted artifacts through each selected real module ABI.
const assert=require('node:assert/strict'),vm=require('node:vm'),path=require('node:path');
const ts=require(path.resolve(__dirname,'../../../../backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
async function execute(outputs,files,options,branch='commonjs') {
  const cache=new Map(),publications=[];
  const library={exports:{}};
  if(files['node_modules/tslib/tslib.js'])vm.runInNewContext(files['node_modules/tslib/tslib.js'],{exports:library.exports,module:library},{timeout:1000});
  const globals=!options.importHelpers&&options.noEmitHelpers?{...library.exports}:{},context=vm.createContext(globals);
  function dependency(from,name) {
    if(name==='tslib')return library.exports;
    assert(name.startsWith('./')||name.startsWith('../'),'reviewed class module dependency');
    const resolved=path.posix.normalize(path.posix.join(path.posix.dirname(from),name));
    assert(Object.hasOwn(outputs,resolved),'actual emitted class dependency '+resolved);return load(resolved);
  }
  function load(name) {
    if(cache.has(name))return cache.get(name);
    const exports={},module={exports};cache.set(name,exports);let registered=null,defined=0;
    const require=specifier=>dependency(name,specifier);
    function define(dependencies,factory) {
      defined++;const result=factory(...dependencies.map(value=>value==='exports'?exports:value==='require'?require:require(value)));
      if(result!==undefined)module.exports=result;
    }
    define.amd={};
    const format=String(options.module).toLowerCase(),sandbox={...globals};
    if(format==='system')sandbox.System={register(dependencies,factory){assert.equal(registered,null);registered={dependencies,body:factory((key,value)=>{if(typeof key==='string'){exports[key]=value;publications.push([key,value]);}else Object.assign(exports,key);return value;},{id:name})};}};
    else if(format==='amd'||format==='umd'&&branch==='amd')sandbox.define=define;
    else Object.assign(sandbox,{exports,module,require});
    vm.runInNewContext(outputs[name],sandbox,{timeout:1000});
    if(format==='system') {
      assert(registered&&Array.isArray(registered.body.setters)&&typeof registered.body.execute==='function');assert.equal(registered.body.setters.length,registered.dependencies.length);
      registered.body.setters.forEach((setter,index)=>setter(require(registered.dependencies[index])));registered.body.execute();
    }else if(format==='amd'||format==='umd'&&branch==='amd')assert.equal(defined,1);
    cache.set(name,module.exports);return module.exports;
  }
  const parsed=ts.createSourceFile('main.js',outputs['out/main.js'],ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  assert.equal(parsed.parseDiagnostics.length,0,'actual executed module syntax');
  const esmOutput=parsed.statements.some(node=>ts.isImportDeclaration(node)||ts.isExportDeclaration(node)||ts.isExportAssignment(node)||node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));
  if(esmOutput) {
    const modules=new Map();
    function esm(name) {
      if(modules.has(name))return modules.get(name);
      const instance=new vm.SourceTextModule(outputs[name],{context,identifier:'memory:///'+name});modules.set(name,instance);return instance;
    }
    const main=esm('out/main.js');await main.link(async(specifier,from)=>{
      if(specifier==='tslib')return new vm.SyntheticModule(Object.keys(library.exports),function(){for(const key of Object.keys(library.exports))this.setExport(key,library.exports[key]);},{context});
      assert(specifier.startsWith('./')||specifier.startsWith('../'));
      const current=from.identifier.slice('memory:///'.length),name=path.posix.normalize(path.posix.join(path.posix.dirname(current),specifier));assert(Object.hasOwn(outputs,name));return esm(name);
    });await main.evaluate({timeout:1000});assert(Object.hasOwn(main.namespace,'result'));return main.namespace.result;
  }
  return load('out/main.js').result;
}
async function run(native,actual,files,options) {
  const branches=String(options.module).toLowerCase()==='umd'?['commonjs','amd']:['commonjs'];
  const results=[];
  for(const branch of branches) {
    const expected=await execute(native,files,options,branch),observed=await execute(actual,files,options,branch);
    assert.equal(observed,expected,'actual class/super execution '+options.module+'/'+branch);results.push({branch,result:expected});
  }
  return {executions:branches.length*2,results};
}
module.exports={execute,run};
