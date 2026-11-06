// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,16);
function run(code){const exports={};vm.runInNewContext(code,{exports,module:{exports}},{timeout:2000});return JSON.parse(JSON.stringify(exports.result));}
for(const row of rows){const native=ts.transpileModule(row.source,{compilerOptions:{target:ts.ScriptTarget[row.target],module:ts.ModuleKind.CommonJS,experimentalDecorators:true,useDefineForClassFields:false}}).outputText,expected=run(native);const early=row.target==='ES2015'&&row.published;assert.deepEqual(expected,[early,early?'function':'undefined',2]);assert.deepEqual(run(row.javascript),expected,'static storage initialization sees the original published constructor');}
console.log(JSON.stringify({configurations:rows.length,executions:rows.length*2,passed:true}));
