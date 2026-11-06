// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A harmless layout difference cannot authorize losing a declaration ending.
const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const {stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_decorator_preserved.cjs');
function fixture(){const source='class Box { value=1; read(){return this.value;} }\nconst after=2;\n',emitted=ts.transpileModule(source,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES2022,sourceMap:true,inlineSources:true}}),native=stream(ts,emitted.outputText,emitted.sourceMapText),end=native.source.statements[0].end,actual=stream(ts,emitted.outputText.slice(0,end)+' '+emitted.outputText.slice(end),emitted.sourceMapText);assert.notEqual(native.code,actual.code,'valid independent formatting difference');assert.deepEqual(actual.owner(end),native.owner(end));assert.notEqual(native.owner(end),null,'effective declaration endpoint fixture');return {native,actual,end,next:native.owner(native.source.statements[1].getStart(native.source))};}
test('retained complete declaration survives an added layout space',()=>{const f=fixture();assert(compare(ts,f.native,f.actual));});
for(const[label,owner]of[['erase',null],['retarget','next']])test('retained module cannot '+label+' its exact class declaration endpoint',()=>{const f=fixture(),changed=mutate(ts,f.actual,f.end,owner==='next'?f.next:owner);assert.notDeepEqual(changed.owner(f.end),f.native.owner(f.end),'effective independently valid map mutation');assert.throws(()=>compare(ts,f.native,changed),/exact retained class declaration endpoint/);});
