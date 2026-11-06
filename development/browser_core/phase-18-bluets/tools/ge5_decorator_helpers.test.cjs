// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict'),path=require('node:path'),test=require('node:test');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),{semantic}=require('./ge5_decorator_helpers.cjs');
function shape(code){const source=ts.createSourceFile('helper.js','('+code+')',ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);assert.equal(source.parseDiagnostics.length,0);return semantic(ts,source.statements[0].expression.expression);}
test('complete helper semantics retain ordinary, generator and async function kind',()=>{
  const ordinary=shape('function(value){return value;}');
  assert.notDeepEqual(shape('function*(value){return value;}'),ordinary);
  assert.notDeepEqual(shape('async function(value){return value;}'),ordinary);
  assert.deepEqual(shape('function(copy){return copy;}'),ordinary,'independent local alpha binding remains legal');
});
test('nested helper callable kind remains part of the complete algorithm',()=>{
  assert.notDeepEqual(shape('function(value){function* accept(item){return item;}return accept(value);}'),shape('function(value){function accept(item){return item;}return accept(value);}'));
});
