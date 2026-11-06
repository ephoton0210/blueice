// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Preserve the frozen G-E3 emitted-JavaScript-only proposal execution adapter.
const assert=require('node:assert/strict'),observe=require('./ge1_observations.cjs');
function run(ts,parent,original,native,actual) {
  const row={...parent,files:original.files,options:original.options};
  function adapt(outputs) {
    if(!parent.preservedSyntax)return outputs;
    return Object.fromEntries(Object.entries(outputs).map(([name,text])=>[name,/\.(?:js|mjs|cjs)$/.test(name)?ts.transpileModule(text,{fileName:name,compilerOptions:{target:ts.ScriptTarget.ES2022,module:row.runtimeModule==='CommonJS'?ts.ModuleKind.CommonJS:ts.ModuleKind.ESNext,experimentalDecorators:row.mode==='legacy',newLine:ts.NewLineKind.LineFeed}}).outputText:text]));
  }
  const expected=observe.execute(row,adapt(native)),observed=observe.execute(row,adapt(actual));
  assert.equal(expected.exit,0,'actual Native class-expression completion');assert.deepEqual(observed,expected,'actual complete class-expression execution/evaluation effects');
  return {executions:2,adapter:parent.preservedSyntax?'typescript@5.9.3 emitted-JavaScript-only ES2022 lowering':null,observation:observed};
}
module.exports={run};
