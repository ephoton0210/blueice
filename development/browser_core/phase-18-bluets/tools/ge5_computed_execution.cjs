// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute both actual modules and distinguish the saved receiver from a rebind.
const assert=require('node:assert/strict'),{execute}=require('./ge5_module_execution.cjs');
async function run(ts,native,actual,options,proof) {
  const profile={format:'plain',module:options.module,esm:options.module==='ESNext'},variants=[];
  const step=(value,done)=>({value,done});
  function fixture(view,trace,mode) {
    if(mode==='original')return view;
    const statement=view.source.statements.at(-1),expression=ts.isVariableStatement(statement)?statement.declarationList.declarations[0].initializer:statement.expression.right;
    const store=trace.events.find(event=>event.kind==='store');assert(store&&store.left.text==='values'&&ts.isArrayLiteralExpression(store.right));
    const prefix=`(function(){var current=sequence();var first=current.next();`;
    let text;
    if(mode==='key-zero')text=prefix+`return JSON.stringify([first,current.next(0),current.next(7)]);})()`;
    if(mode==='rebound')text=prefix+`var rebound=[10,20];globalThis.__ge5_reset(rebound);var second=current.next(1);var last=current.next(42);return JSON.stringify([first,second,last,globalThis.__ge5_initial,rebound]);})()`;
    if(mode==='early-return')text=prefix+`var second=current.next(1);var last=current.return(9);return JSON.stringify([first,second,last,globalThis.__ge5_initial]);})()`;
    assert(text);const edits=[[expression.getStart(view.source),expression.end,text]];
    if(mode==='rebound'||mode==='early-return')edits.push([store.right.getStart(view.source),store.right.end,`(globalThis.__ge5_reset=function(array){values=array;},globalThis.__ge5_initial=[0,0])`]);
    let code=view.code;for(const[from,to,text]of edits.sort((a,b)=>b[0]-a[0]))code=code.slice(0,from)+text+code.slice(to);return {code};
  }
  for(const mode of ['original','key-zero','rebound','early-return']) {
    const expected=JSON.stringify(mode==='original'?[step(1,false),step(2,false),step(42,true)]:mode==='key-zero'?[step(1,false),step(2,false),step(0,true)]:mode==='rebound'?[step(1,false),step(2,false),step(20,true),[0,42],[10,20]]:[step(1,false),step(2,false),step(9,true),[0,0]]);
    const a=await execute(ts,fixture(native,proof.native,mode),profile,'default',expected),b=await execute(ts,fixture(actual,proof.actual,mode),profile,'default',expected);assert.equal(b,a);variants.push({mode,executions:2,result:expected});
  }
  return {executions:2,additionalFixtureExecutions:6,variants};
}
module.exports={run};
