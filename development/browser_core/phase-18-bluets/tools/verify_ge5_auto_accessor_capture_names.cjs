// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A constructor's observable name includes file-level static private capture reservations.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),rows=JSON.parse(fs.readFileSync(0,'utf8'));
assert.equal(rows.length,32);let executions=0;
(async()=>{for(const row of rows){const options={target:ts.ScriptTarget.ES2015,module:ts.ModuleKind.CommonJS,experimentalDecorators:row.legacy,useDefineForClassFields:false},native=ts.transpileModule(row.source,{fileName:'main.ts',compilerOptions:options}).outputText,configuration={target:'ES2015',module:'CommonJS',experimentalDecorators:row.legacy,useDefineForClassFields:false},files={'main.ts':row.source},expected=await require('./ge5_class_execution.cjs').execute({'out/main.js':native},files,configuration),actual=await require('./ge5_class_execution.cjs').execute({'out/main.js':row.javascript},files,configuration);assert.equal(actual,expected,'complete Native observable name '+row.form+'/'+row.legacy+'/'+row.variant);executions+=2;}console.log(JSON.stringify({configurations:rows.length,executions,passed:true}));})().catch(error=>{console.error(error.stack);process.exitCode=1;});
