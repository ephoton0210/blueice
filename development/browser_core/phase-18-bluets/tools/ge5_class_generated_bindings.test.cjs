// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const {stream}=require('./ge5_role_stream.cjs'),{readClass,compareClass}=require('./ge5_class_roles.cjs');
const source='function create() { return class {}; }\n';
const original=ts.createSourceFile('original.ts',source,ts.ScriptTarget.Latest,true),expression=original.statements[0].body.statements[0].expression;
const emitted=ts.transpileModule(source,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES5,sourceMap:true,inlineSources:true}});
function unwrap(node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function read(view){const fn=view.source.statements[0],call=unwrap(fn.body.statements[0].expression),factory=unwrap(call.expression),ctor=factory.body.statements.find(ts.isFunctionDeclaration);return {call,factory,ctor};}
function fixture(code=emitted.outputText.replaceAll('class_1','clazz_9')){const native=stream(ts,emitted.outputText,emitted.sourceMapText),actual=stream(ts,code,emitted.sourceMapText);return {native,actual,n:read(native),a:read(actual)};}
test('fresh constructor identity is bound to the exact factory AST and return',()=>{const f=fixture();const result=compareClass(ts,f.native,f.n.call,f.actual,f.a.call,expression,{classRoles:{generatedBindings:{native:f.n.ctor,actual:f.a.ctor}}});assert.equal(result.native.name,'class_1');assert.equal(result.actual.name,'clazz_9');});
test('a source binding cannot be renamed as a generated constructor',()=>{const f=fixture(emitted.outputText.replaceAll('class_1','create '));assert.throws(()=>readClass(ts,f.actual,f.a.call,expression,{generatedConstructor:f.a.ctor}),/fresh/);});
test('a constructor from another syntax tree or another factory is refused',()=>{const f=fixture(),other=fixture();assert.throws(()=>readClass(ts,f.actual,f.a.call,expression,{generatedConstructor:other.a.ctor}),/exact generated/);const create=f.actual.source.statements[0];assert.throws(()=>readClass(ts,f.actual,f.a.call,expression,{generatedConstructor:create}),/fresh|supplied generated|exact generated/);});
test('changing only the generated return binding is rejected',()=>{const code=emitted.outputText.replaceAll('class_1','clazz_9').replace('return clazz_9;','return clazz_8;'),f=fixture(code);assert.throws(()=>readClass(ts,f.actual,f.a.call,expression,{generatedConstructor:f.a.ctor}),/complete class return/);});
test('a second constructor cannot hide behind a supplied declaration',()=>{const f=fixture();f.a.factory.body.statements.splice(1,0,f.a.ctor);assert.throws(()=>readClass(ts,f.actual,f.a.call,expression,{generatedConstructor:f.a.ctor}),/Expected values to be strictly equal/);});
test('a fresh constructor proof does not rename an unrelated member receiver',()=>{
  const source='function create() { return class { method(){return 1;} }; }\n',original=ts.createSourceFile('original.ts',source,ts.ScriptTarget.Latest,true),expression=original.statements[0].body.statements[0].expression,output=ts.transpileModule(source,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES5,sourceMap:true,inlineSources:true,useDefineForClassFields:true}}),native=stream(ts,output.outputText,output.sourceMapText),code=output.outputText.replaceAll('class_1','clazz_9').replace('clazz_9.prototype','clazz_8.prototype'),actual=stream(ts,code,output.sourceMapText),n=read(native),a=read(actual);
  const positive=stream(ts,output.outputText.replaceAll('class_1','clazz_9'),output.sourceMapText),p=read(positive);compareClass(ts,native,n.call,positive,p.call,expression,{classRoles:{generatedBindings:{native:n.ctor,actual:p.ctor}}});
  assert.equal(actual.source.parseDiagnostics.length,0,'wrong receiver control remains valid JavaScript');assert.throws(()=>compareClass(ts,native,n.call,actual,a.call,expression,{classRoles:{generatedBindings:{native:n.ctor,actual:a.ctor}}}),/spelling|inventory|coherent|complete/);
});
