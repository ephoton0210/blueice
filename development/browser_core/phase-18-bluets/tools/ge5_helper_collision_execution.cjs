// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute actual modules and independent value/provider/getter/rejection probes.
const assert=require('node:assert/strict'),execution=require('./ge5_class_execution.cjs');
async function run(ts,native,actual,files,options){const effects='__ge5_collision_effects',probe='__ge5_collision_probe',variants=[
 {name:'original',expected:42},
 {name:'zero-operand',argument:'0',expected:8},
 {name:'string-operand',argument:'"34"',expected:'3471'},
 {name:'source-tslib-binding',binding:'tslib_1',value:'70',expected:105},
 {name:'source-generator-binding',binding:'__blueice_target_generator_0',value:'9',expected:50},
 {name:'rejected-source-promise',argument:'Promise.reject("source rejection")',error:'source rejection'},
 {name:'thenable-adoption',argument:'({then:function(resolve){resolve(34);}})',expected:42},
 {name:'resolve-getter-once',getter:'return function(value){'+effects+'.push("call");return Promise.resolve(value);};',expected:JSON.stringify([42,['get','call']])},
 {name:'resolve-getter-abrupt',getter:'throw "source lookup";',expected:JSON.stringify([{error:'source lookup'},['get']])},
 {name:'resolve-call-abrupt',getter:'return function(value){'+effects+'.push("call");throw "source invocation";};',expected:JSON.stringify([{error:'source invocation'},['get','call']])},
 ];
 function edited(outputs,variant){if(variant.name==='original')return outputs;const code=outputs['out/main.js'],source=ts.createSourceFile('runtime.js',code,99,true),calls=[],functions=[],bindings=[],results=[];function visit(node){if(ts.isFunctionDeclaration(node)&&node.name?.text==='compute')functions.push(node);if(ts.isVariableDeclaration(node)&&ts.isIdentifier(node.name)&&node.name.text===variant.binding&&node.initializer)bindings.push(node.initializer);if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(node.left)&&node.left.text===variant.binding)bindings.push(node.right);if(ts.isCallExpression(node)&&node.expression.getText(source)==='Promise.resolve'&&node.arguments.length===1&&node.arguments[0].getText(source)==='34')calls.push(node);if(ts.isCallExpression(node)&&ts.isPropertyAccessExpression(node.expression)&&node.expression.name.text==='then'&&ts.isCallExpression(node.expression.expression)&&node.expression.expression.expression.getText(source)==='compute')results.push(node);ts.forEachChild(node,visit);}visit(source);assert.equal(calls.length,1);assert.equal(functions.length,1);assert.equal(results.length,1);const edits=[];
  if(variant.argument)edits.push([calls[0].arguments[0].getStart(source),calls[0].arguments[0].end,variant.argument]);
  if(variant.binding){assert.equal(bindings.length,1);edits.push([bindings[0].getStart(source),bindings[0].end,variant.value]);}
  if(variant.getter){assert(!code.includes(effects)&&!code.includes(probe));const setup='var '+effects+'=[],'+probe+'={};Object.defineProperty('+probe+',"resolve",{get:function(){'+effects+'.push("get");'+variant.getter+'}});\n';edits.push([functions[0].getStart(source),functions[0].getStart(source),setup]);edits.push([calls[0].expression.getStart(source),calls[0].expression.end,probe+'.resolve']);edits.push([results[0].getStart(source),results[0].end,'compute().then(function(value){return JSON.stringify([value+tslib_1+__blueice_target_generator_0,'+effects+']);},function(error){return JSON.stringify([{error:String(error)},'+effects+']);})']);}
  let changed=code;for(const[from,to,text]of edits.sort((a,b)=>b[0]-a[0]))changed=changed.slice(0,from)+text+changed.slice(to);return {...outputs,'out/main.js':changed};
 }
 async function outcome(outputs,branch){try{return {value:await execution.execute(outputs,files,options,branch)};}catch(error){return {error:String(error)};}}
 const branches=options.module==='UMD'?['commonjs','amd']:['commonjs'],verified=[];for(const variant of variants){for(const branch of branches){const expected=await outcome(edited(native,variant),branch),observed=await outcome(edited(actual,variant),branch);assert.deepEqual(expected,Object.hasOwn(variant,'error')?{error:variant.error}:{value:variant.expected},'pinned Native helper-collision scenario '+variant.name+'/'+branch);assert.deepEqual(observed,expected,'actual module helper-collision scenario '+variant.name+'/'+branch);}verified.push({name:variant.name,executions:branches.length*2});}
 return {executions:branches.length*2,additionalFixtureExecutions:branches.length*2*(variants.length-1),variants:verified};
}
module.exports={run};
