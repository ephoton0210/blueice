// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,8);
function run(code){const exports={},module={exports};try{vm.runInNewContext(code,{exports,module},{timeout:1000});return {result:JSON.parse(JSON.stringify(module.exports.result))};}catch(error){return{error:error.name,message:error.message};}}
for(const row of rows){const native=ts.transpileModule(row.source,{fileName:'main.ts',compilerOptions:{target:ts.ScriptTarget[row.target],module:ts.ModuleKind.CommonJS,useDefineForClassFields:false}}).outputText,n=run(native),a=run(row.outputs['out/main.js']);assert.deepEqual(n,{result:['Named']});assert.deepEqual(a,n,'source class lexical binding is initialized before its method decorator observes it');}
console.log(JSON.stringify({ordinaryConfigurations:rows.length,repeatBuilds:2,actualNativeExecutions:rows.length*2,namedClassBindingPrecedesMethodDecoration:true}));
