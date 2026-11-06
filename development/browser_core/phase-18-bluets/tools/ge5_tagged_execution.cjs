// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Frozen numeric execution plus a separate repeated-site/raw-descriptor fixture.
const vm=require('node:vm'),assert=require('node:assert/strict'),{execute}=require('./ge5_module_execution.cjs');
async function run(ts,native,actual,options,proof,files) {
  const exports={},module={exports};vm.runInNewContext(files['node_modules/tslib/tslib.js'],{exports,module},{timeout:1000});
  assert.equal(typeof module.exports.__makeTemplateObject,'function');
  const environment={tslib:module.exports,members:['__makeTemplateObject'],globals:options.noEmitHelpers&&!options.importHelpers?{__makeTemplateObject:module.exports.__makeTemplateObject}:{}};
  const profile={format:proof.native.format,module:options.module,dependency:proof.native.dependency,
    esm:proof.native.format==='plain'&&native.source.statements.some(statement=>statement.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword))};
  const branches=options.module==='UMD'?['commonjs','amd']:['default'],variants=[];
  function repeated(view,item,binding) {
    const body=item.user[0].body,call=binding.expression.getText(view.source);assert(body&&item.user[0].parameters[0].name.text==='parts');
    const expression=`(function(){function __ge5_site(){return ${call};}var first=__ge5_site(),second=__ge5_site();var d=Object.getOwnPropertyDescriptor(first,'raw');return JSON.stringify([first===second,first.length,first[0],first[1],first.raw[0],first.raw[1],d.enumerable,d.writable,d.configurable,Object.isFrozen(first)]);})()`;
    const edits=[[body.getStart(view.source),body.end,'{return parts;}'],[binding.expression.getStart(view.source),binding.expression.end,expression]].sort((a,b)=>b[0]-a[0]);
    let code=view.code;for(const[from,to,text]of edits)code=code.slice(0,from)+text+code.slice(to);return {code};
  }
  for(const mode of ['original','repeated-site']) {
    const wanted=mode==='original'?42:JSON.stringify([true,2,'a','','a','',false,false,false,options.target!=='ES5']);let executions=0;
    for(const branch of branches) {
      const expected=await execute(ts,mode==='original'?native:repeated(native,proof.native,proof.bindings.native),profile,branch,wanted,environment);
      const observed=await execute(ts,mode==='original'?actual:repeated(actual,proof.actual,proof.bindings.actual),profile,branch,wanted,environment);assert.equal(observed,expected);executions+=2;
    }
    variants.push({mode,executions,result:wanted});
  }
  return {executions:variants[0].executions,branches,actualResult:42,nativeResult:42,additionalFixtureExecutions:variants[1].executions,variants};
}
module.exports={run};
