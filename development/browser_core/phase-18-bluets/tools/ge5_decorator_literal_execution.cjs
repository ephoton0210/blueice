// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Preserved proposal execution retains the exact emitted-JavaScript-only G-E3 adapter.
const assert=require('node:assert/strict'),execute=require('./ge1_observations.cjs').execute;
function run(ts,acorn,parent,original,native,actual,record){
  const runtime={...parent,options:original.options,files:original.files};
  if(!parent.preservedSyntax){const result=require('./ge5_decorator_execution.cjs').run(acorn,'ge3_decorator_compositions',null,parent,original,native,actual);assert.deepEqual(result.observation,record.execution,'unchanged complete frozen G-E3 execution authority');return {...result,directNodePreservedSyntaxClaim:false};}
  function adapted(outputs){return Object.fromEntries(Object.entries(outputs).map(([name,text])=>[name,/\.(?:js|mjs|cjs)$/.test(name)?ts.transpileModule(text,{fileName:name,compilerOptions:{target:ts.ScriptTarget.ES2022,module:parent.runtimeModule==='CommonJS'?ts.ModuleKind.CommonJS:ts.ModuleKind.ESNext,experimentalDecorators:false,newLine:ts.NewLineKind.LineFeed}}).outputText:text]));}
  const expected=execute(runtime,adapted(native)),observed=execute(runtime,adapted(actual));assert.deepEqual(observed,expected,'actual complete Native/Blue emitted-JavaScript adapter execution');const {adapter,...recorded}=record.proposalExecution;assert.deepEqual(expected,recorded,'unchanged complete frozen Native G-E3 adapter authority');return {executions:2,scenarios:['configured-proposal-adapter'],native:expected,blue:observed,adapter,directNodePreservedSyntaxClaim:false};
}
module.exports={run};
