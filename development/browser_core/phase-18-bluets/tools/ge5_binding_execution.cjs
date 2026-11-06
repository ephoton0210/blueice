// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Actual frozen modules plus separate getter-order and abrupt-read fixtures.
const assert=require('node:assert/strict'),vm=require('node:vm');
async function run(ts,native,actual,options,form) {
  if(form==='binding-iterator-finally')return require('./ge5_binding_iterator_execution.cjs').run(native,actual,options);
  async function execute(view,mode) {
    const events=[];
    const property=(object,key,value)=>Object.defineProperty(object,key,{enumerable:true,get(){events.push(key);if(mode==='throw'&&events.length===2)throw new Error('binding probe');return value;}});
    const probe=()=>form==='array-nested-binding'?property(property([], '0',property([], '0',40)),'1',2):['object-nested-binding','nested-parameter'].includes(form)?property({},'part',property({},'value',form==='nested-parameter'?42:40)):property({},'part',property(property([],'0',undefined),'2',undefined));
    let code=view.code;
    const fallback40=()=>{events.push('default40');return 40;},fallback2=()=>{events.push('default2');return 2;};
    if(mode!=='original') {
      const nodes=[];
      function find(node){if(form==='array-nested-binding'&&ts.isArrayLiteralExpression(node)&&node.elements.length===2&&ts.isArrayLiteralExpression(node.elements[0]))nodes.push(node);else if(form!=='array-nested-binding'&&ts.isObjectLiteralExpression(node)&&node.properties[0]?.name?.getText()==='part')nodes.push(node);ts.forEachChild(node,find);}find(view.source);
      assert.equal(nodes.length,1,'complete original initializer identity');const node=nodes[0],edits=[{start:node.getStart(view.source),end:node.end,text:'probe()'}];
      if(form==='mixed-nested-default') {const defaults=[];function findDefault(node){if(ts.isBindingElement(node)&&node.initializer&&ts.isNumericLiteral(node.initializer))defaults.push(node.initializer);else if(ts.isConditionalExpression(node)&&ts.isNumericLiteral(node.whenTrue)&&['40','2'].includes(node.whenTrue.text))defaults.push(node.whenTrue);ts.forEachChild(node,findDefault);}findDefault(view.source);assert.equal(defaults.length,2);for(const value of defaults)edits.push({start:value.getStart(view.source),end:value.end,text:value.text==='40'?'fallback40()':'fallback2()'});}
      for(const edit of edits.sort((a,b)=>b.start-a.start))code=code.slice(0,edit.start)+edit.text+code.slice(edit.end);
    }
    try {
      let result;
      if(options.module==='ESNext') {
        const module=new vm.SourceTextModule(code,{context:vm.createContext({probe,fallback40,fallback2}),identifier:'memory:///main.js'});
        await module.link(name=>assert.fail('unexpected dependency '+name));await module.evaluate();assert.deepEqual(Object.keys(module.namespace),form==='nested-parameter'?['read','result']:['result']);result=module.namespace.result;
      } else {
        const exports={},context=vm.createContext({exports,module:{exports},probe,fallback40,fallback2});vm.runInContext(code,context,{timeout:1000});assert.deepEqual(Object.keys(exports).sort(),form==='nested-parameter'?['read','result']:['result']);result=exports.result;
      }
      return {result,events};
    } catch(error) {if(error.code==='ERR_ASSERTION')throw error;return {error:error.name,message:error.message,events};}
  }
  const variants=[];
  for(const mode of ['original','getters','throw']) {
    const expected=await execute(native,mode),observed=await execute(actual,mode);assert.deepEqual(observed,expected,'actual Native/Blue binding '+mode);
    if(mode!=='throw')assert.equal(observed.result,42);
    else {assert.equal(observed.error,'Error');assert.equal(observed.message,'binding probe');}
    if(mode==='getters') {const expectedEvents=form==='array-nested-binding'?(options.target==='ES5'&&options.downlevelIteration?['0','1','0']:['0','0','1']):['object-nested-binding','nested-parameter'].includes(form)?['part','value']:(options.target==='ES5'?['part','0','2','default40','default2']:['part','0','default40','2','default2']);assert.deepEqual(observed.events,expectedEvents,'single reads and ordered defaults');}
    variants.push({mode,executions:2,...observed});
  }
  return {executions:2,additionalFixtureExecutions:4,nativeResult:42,actualResult:42,variants};
}
module.exports={run};
