// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute frozen artifacts, then separately exercise a sparse runtime fixture.
const vm=require('node:vm'),assert=require('node:assert/strict'),{execute}=require('./ge5_module_execution.cjs');
async function run(ts,native,actual,options,proof,files) {
  const exports={},module={exports};
  vm.runInNewContext(files['node_modules/tslib/tslib.js'],{exports,module},{timeout:1000});
  assert.equal(typeof module.exports.__spreadArray,'function');
  const environment={tslib:module.exports,globals:options.noEmitHelpers&&!options.importHelpers?{__spreadArray:module.exports.__spreadArray}:{}};
  const profile={format:proof.native.format,module:options.module,dependency:proof.native.dependency,
    esm:proof.native.format==='plain'&&native.source.statements.some(statement=>statement.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword))};
  const branches=options.module==='UMD'?['commonjs','amd']:['default'],variants=[];
  function sparse(view,side) {
    const array=proof.values[side].expression,result=proof.bindings[side].expression;
    assert(ts.isArrayLiteralExpression(array)&&array.elements.length===2);
    const edits=[[array.getStart(view.source),array.end,'[20,,22]'],[result.getStart(view.source),result.end,'JSON.stringify([copied.length,1 in copied,copied[1]])']].sort((a,b)=>b[0]-a[0]);
    let code=view.code;for(const[from,to,text]of edits)code=code.slice(0,from)+text+code.slice(to);return {code};
  }
  for(const mode of ['original','sparse']) {
    const wanted=mode==='original'?44:'[3,true,null]';let executions=0;
    for(const branch of branches) {
      const expected=await execute(ts,mode==='original'?native:sparse(native,'native'),profile,branch,wanted,environment);
      const observed=await execute(ts,mode==='original'?actual:sparse(actual,'actual'),profile,branch,wanted,environment);assert.equal(observed,expected);executions+=2;
    }
    variants.push({mode,executions,result:wanted});
  }
  return {executions:variants[0].executions,branches,actualResult:44,nativeResult:44,additionalFixtureExecutions:variants[1].executions,variants};
}
module.exports={run};
