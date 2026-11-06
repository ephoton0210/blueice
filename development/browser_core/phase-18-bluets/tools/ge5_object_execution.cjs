// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Actual module loaders plus separate getter, symbol, descriptor and abrupt fixtures.
const assert=require('node:assert/strict'),vm=require('node:vm'),{execute}=require('./ge5_module_execution.cjs');
async function run(ts,native,actual,options,proof,files) {
  const exports={},module={exports};vm.runInNewContext(files['node_modules/tslib/tslib.js'],{exports,module},{timeout:1000});const library=module.exports;
  const profile={format:proof.native.format,module:options.module,dependency:proof.native.dependencies,esm:proof.native.format==='plain'&&native.source.statements.some(node=>node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword))};
  const branches=options.module==='UMD'?['commonjs','amd']:['default'],variants=[];
  function rhs(node,view){if(ts.isVariableStatement(node))return node.declarationList.declarations[0].initializer;let expression=node.expression;if(ts.isCallExpression(expression))expression=expression.arguments[1];assert(ts.isBinaryExpression(expression));return expression.right;}
  function fixture(view,side,mode) {
    if(mode==='original')return view;
    const source=rhs(proof[side].user[0],view),result=rhs(proof[side].user[3],view);
    const replacement='JSON.stringify([a,remaining.b,copy.a,Object.keys(remaining),Object.keys(copy),Object.getOwnPropertySymbols(remaining).map(function(k){return remaining[k];}),Object.getOwnPropertySymbols(copy).map(function(k){return copy[k];}),"hidden" in remaining,"inherited" in copy,!!Object.getOwnPropertyDescriptor(copy,"b").get,Object.getOwnPropertyDescriptor(copy,"b").writable])';
    const edits=[[source.getStart(view.source),source.end,'probe()'],[result.getStart(view.source),result.end,replacement]].sort((x,y)=>y[0]-x[0]);let code=view.code;for(const [from,to,text]of edits)code=code.slice(0,from)+text+code.slice(to);return {code};
  }
  for(const mode of ['original','getters','throw']) {
    let executions=0;const results=[];
    for(const branch of branches)for(const [side,view]of [['native',native],['actual',actual]]) {
      const events=[],symbol=Symbol('probe');
      function probe(){const value=Object.create({inherited:99});for(const [key,label,result]of [['a','a',20],['b','b',21],[symbol,'symbol',7]])Object.defineProperty(value,key,{enumerable:true,get(){events.push(label);if(mode==='throw'&&label==='b')throw new Error('object probe');return result;}});Object.defineProperty(value,'hidden',{value:88});return value;}
      const environment={tslib:library,members:['__assign','__rest'],globals:{...(options.noEmitHelpers&&!options.importHelpers?{__assign:library.__assign,__rest:library.__rest}:{}),probe}};
      const wanted=mode==='original'?42:'[20,21,1,["b"],["a","b"],[7],[7],false,false,false,true]';
      if(mode==='throw'){let thrown=null;try{await execute(ts,fixture(view,side,mode),profile,branch,wanted,environment);}catch(error){thrown=error;}assert(thrown&&thrown.message==='object probe','abrupt getter propagates through actual module');assert.deepEqual(events,['a','b']);results.push({side,error:thrown.message,events});}
      else {const result=await execute(ts,fixture(view,side,mode),profile,branch,wanted,environment);if(mode==='getters')assert.deepEqual(events,['a','b','symbol','a','b','symbol'],'one ordinary read, rest copy and spread copy in order');results.push({side,result,events});}
      executions++;
    }
    variants.push({mode,executions,results});
  }
  return {executions:variants[0].executions,additionalFixtureExecutions:variants.slice(1).reduce((sum,row)=>sum+row.executions,0),variants};
}
module.exports={run};
