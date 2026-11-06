// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute the complete module, including private/ambient/import helper routing.
const assert=require('node:assert/strict'),execution=require('./ge5_class_execution.cjs');
async function run(ts,nativeOutputs,actualOutputs,files,options){
  const setup='var fresh=new Box(),descriptor=Object.getOwnPropertyDescriptor(Box.prototype,"value"),get=descriptor.get,set=descriptor.set;';
  const variant=(name,body,expected)=>({name,body,expected:JSON.stringify(expected)});
  const variants=[{name:'original',expected:42},
    variant('fresh-instance-private-state','return fresh.value;',40),
    variant('distinct-instance-states','fresh.value=7;return [fresh.value,(new Box()).value];',[7,40]),
    variant('borrowed-get-correct-brand','return get.call(fresh);',40),
    variant('borrowed-set-correct-brand','var returned=set.call(fresh,9);return [typeof returned,get.call(fresh)];',['undefined',9]),
    variant('private-get-foreign-receiver','return caught(function(){return get.call({value:9});});','TypeError'),
    variant('private-set-foreign-receiver','return caught(function(){return set.call({value:9},10);});','TypeError'),
    variant('prototype-does-not-confer-private-brand','var forged=Object.create(Box.prototype);return [fresh.has(forged),caught(function(){return get.call(forged);})];',[false,'TypeError']),
    variant('subclass-preserves-base-private-brand','class Derived extends Box{}var derived=new Derived();return [fresh.has(derived),derived.value];',[true,40]),
    variant('private-in-object-and-function','return [fresh.has(fresh),fresh.has({}),fresh.has(function(){})];',[true,false,false]),
    variant('private-in-primitives','return [null,void 0,0,"x",true,Symbol("x"),1n].map(function(value){return caught(function(){return fresh.has(value);});});',Array(7).fill('TypeError')),
    variant('proxy-never-acquires-the-target-brand','var effects=0,proxy=new Proxy(fresh,{get:function(){effects++;throw "get trap";},set:function(){effects++;throw "set trap";},has:function(){effects++;throw "has trap";}});return [fresh.has(proxy),caught(function(){return get.call(proxy);}),caught(function(){return set.call(proxy,9);}),effects];',[false,'TypeError','TypeError',0]),
    variant('public-property-shadow-keeps-private-state','Object.defineProperty(fresh,"value",{value:99,writable:true});set.call(fresh,7);return [fresh.value,get.call(fresh)];',[99,7]),
    variant('right-hand-getter-is-evaluated-once','var effects=0,rhs={get next(){effects++;return 8;}};fresh.value=rhs.next;return [effects,get.call(fresh)];',[1,8]),
    variant('abrupt-right-hand-getter-preserves-private-state','var effects=0,rhs={get next(){effects++;throw "rhs getter";}};var error=caught(function(){fresh.value=rhs.next;});return [error,effects,get.call(fresh)];',['rhs getter',1,40]),
    variant('wrong-brand-does-not-read-public-getter','var effects=0,other={get value(){effects++;throw "public getter";}};return [caught(function(){return get.call(other);}),effects];',['TypeError',0]),
    variant('private-get-null-receiver','return caught(function(){return get.call(null);});','TypeError'),
    variant('private-set-primitive-receiver','return caught(function(){return set.call(1,9);});','TypeError'),
  ];
  function edited(outputs,item){if(item.name==='original')return outputs;const code=outputs['out/main.js'],source=ts.createSourceFile('runtime.js',code,99,true),results=[];function visit(node){if(ts.isConditionalExpression(node)&&ts.isCallExpression(node.condition)&&node.condition.expression.getText(source)==='box.has')results.push(node);ts.forEachChild(node,visit);}visit(source);assert.equal(results.length,1,'one exact original result expression');const expression=results[0],replacement='(function(){'+setup+'function caught(operation){try{return operation();}catch(error){return error&&error.name==="TypeError"?"TypeError":String(error);}}return JSON.stringify((function(){'+item.body+'})());})()';return {...outputs,'out/main.js':code.slice(0,expression.getStart(source))+replacement+code.slice(expression.end)};}
  const branches=options.module==='UMD'?['commonjs','amd']:['commonjs'],verified=[];
  for(const item of variants){for(const branch of branches){const expected=await execution.execute(edited(nativeOutputs,item),files,options,branch),actual=await execution.execute(edited(actualOutputs,item),files,options,branch);assert.equal(expected,item.expected,'pinned Native '+item.name+'/'+branch);assert.equal(actual,expected,'actual private accessor '+item.name+'/'+branch);}verified.push({name:item.name,executions:branches.length*2});}
  return {executions:branches.length*2,additionalFixtureExecutions:(variants.length-1)*branches.length*2,variants:verified};
}
module.exports={run};
