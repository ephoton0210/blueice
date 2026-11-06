// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original iterator closing plus complete normal and abrupt generator lifecycles.
const assert=require('node:assert/strict'),vm=require('node:vm');
async function run(native,actual,options) {
  const modes={normal:'[iterator.next(),iterator.next(),iterator.next()]',returnBefore:'[iterator.return(7),iterator.next()]',returnAfter:'[iterator.next(),iterator.return(7),iterator.next()]',throwBefore:'[attempt(function(){return iterator.throw("boom");}),iterator.next()]',throwAfter:'[iterator.next(),attempt(function(){return iterator.throw("boom");}),iterator.next()]'};
  async function execute(view,mode) {
    let observed=null;const context=vm.createContext({observe(value){assert.equal(observed,null);observed=value;}});
    let code=view.code;
    if(mode!=='original')code+='\nobserve(JSON.stringify((function(){log.length=0;var iterator=values();function attempt(operation){try{return operation();}catch(error){return {error:error};}}return ['+modes[mode]+',log];})()));\n';
    let result;
    if(options.module==='ESNext'){const module=new vm.SourceTextModule(code,{context,identifier:'memory:///main.js'});await module.link(name=>assert.fail('unexpected dependency '+name));await module.evaluate();result=module.namespace.result;}
    else {const exports={};Object.assign(context,{exports,module:{exports}});vm.runInContext(code,context,{timeout:1000});result=exports.result;}
    assert.equal(result,'[40,["closed"]]','original binding closes its iterator');return observed;
  }
  const variants=[];
  for(const mode of ['original',...Object.keys(modes)]){const expected=await execute(native,mode),observed=await execute(actual,mode);assert.equal(observed,expected,'actual Native/Blue generator '+mode);if(mode!=='original'){const [steps,log]=JSON.parse(observed);assert.deepEqual(log,['returnBefore','throwBefore'].includes(mode)?[]:['closed']);assert.equal(steps.at(-1).done,true);if(mode==='normal')assert.deepEqual(steps.slice(0,2),[{value:40,done:false},{value:2,done:false}]);if(mode.startsWith('throw'))assert(steps.some(item=>item.error==='boom'));}variants.push({mode,executions:2,result:observed});}
  return {executions:2,additionalFixtureExecutions:10,variants};
}
module.exports={run};
