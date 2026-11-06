// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute every actual module and separate full-loop, zero-value and abrupt fixtures.
const assert=require('node:assert/strict'),vm=require('node:vm'),{execute}=require('./ge5_module_execution.cjs');
async function run(ts,native,actual,options,proof,files) {
  const exports={},module={exports};vm.runInNewContext(files['node_modules/tslib/tslib.js'],{exports,module},{timeout:1000});const library=module.exports;
  const profile={format:proof.native.format,module:options.module,dependency:proof.native.dependencies,esm:proof.native.format==='plain'&&native.source.statements.some(node=>node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword))};
  const branches=options.module==='UMD'?['commonjs','amd']:['default'],variants=[];
  function fixture(view,side,mode) {
    if(mode==='original')return view;const fn=proof.functions[side].fn,body=proof.loop[side].body,edits=[];
    if(mode==='full'||mode==='throw') {const broken=body.statements.find(ts.isBreakStatement);assert(broken);edits.push([broken.getStart(view.source),broken.end,mode==='full'?'':'throw new Error("loop probe");']);}
    if(mode==='zero') {const literals=[];function visit(node){if(ts.isNumericLiteral(node)&&node.text==='20')literals.push(node);ts.forEachChild(node,visit);}visit(fn);assert.equal(literals.length,1);edits.push([literals[0].getStart(view.source),literals[0].end,'0']);}
    if(mode==='throw') {const effects=[];function visit(node){if(ts.isCallExpression(node)&&node.expression.getText()==='events.push')effects.push(node);ts.forEachChild(node,visit);}visit(fn);assert.equal(effects.length,1);const effect=effects[0];edits.push([effect.getStart(view.source),effect.end,'(events.push(1),observe(events.length))']);}
    let code=view.code;for(const [from,to,text]of edits.sort((x,y)=>y[0]-x[0]))code=code.slice(0,from)+text+code.slice(to);return {code};
  }
  for(const mode of ['original','full','zero','throw']) {
    let executions=0;const observations=[];
    for(const branch of branches)for(const [side,view]of [['native',native],['actual',actual]]){
      const events=[],environment={tslib:library,members:['__generator','__read','__values'],globals:{...(options.noEmitHelpers&&!options.importHelpers?{__generator:library.__generator,__read:library.__read,__values:library.__values}:{}),observe(value){events.push(value);}}};
      if(mode==='throw'){let thrown=null;try{await execute(ts,fixture(view,side,mode),profile,branch,63,environment);}catch(error){thrown=error;}assert(thrown&&thrown.message==='loop probe','source throw propagates through actual iteration');assert.deepEqual(events,[1,2],'binding and abrupt loop both close their iterators');observations.push({side,error:thrown.message,events});}
      else {const wanted=mode==='full'?184:mode==='zero'?23:63;const result=await execute(ts,fixture(view,side,mode),profile,branch,wanted,environment);observations.push({side,result});}
      executions++;
    }
    variants.push({mode,executions,observations});
  }
  return {executions:variants[0].executions,additionalFixtureExecutions:variants.slice(1).reduce((sum,row)=>sum+row.executions,0),variants};
}
module.exports={run};
