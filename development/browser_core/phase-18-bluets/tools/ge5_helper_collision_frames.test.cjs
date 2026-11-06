// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Emitted bytes below are regression input; the independent frozen Native
// program and map remain the authority for the complete module comparison.
const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path');
const root=path.resolve(__dirname,'../../../..');
const ts=require(root+'/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const input=require(root+'/backend/bluets/tests/fixtures/ge5_source_maps/cases.json'),reference=require(root+'/backend/bluets/tests/fixtures/ge5_source_maps/reference.json');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_helper_collision_module.cjs');
const emittedCode="\"use strict\"; Object.defineProperty(exports, \"__esModule\", { value: true }); exports.result = void 0; const tslib_1= 7;\nconst __blueice_target_generator_0= 1;\nfunction compute () {\nif (new.target) throw new TypeError(\"Async function is not a constructor\");\nreturn __awaiter(this, arguments, void 0, function* () { return (yield (Promise.resolve(34) )); });\n}\nexports.result= compute().then(value => value + tslib_1 + __blueice_target_generator_0);\n\n//# sourceMappingURL=main.js.map\n";
const emittedMap="{\"version\":3,\"file\":\"main.js\",\"sourceRoot\":\"\",\"sources\":[\"../main.ts\"],\"sourcesContent\":[\"const tslib_1: number = 7;\\nconst __blueice_target_generator_0: number = 1;\\nasync function compute(): Promise<number> { return await Promise.resolve(34); }\\nexport const result: Promise<number> = compute().then(value => value + tslib_1 + __blueice_target_generator_0);\\n\"],\"names\":[],\"mappings\":\"A,a,yFAAA,MAAM,OAAO,EAAW,CAAC,CAAC;AAC1B,MAAM,4BAA4B,EAAW,CAAC,CAAC;AAC/C,SAAe,OAAO,C,GAAA,C;;O,S,0B,UAAA,CAAA,G,CAAsB,O,CAAO,M,CAAM,OAAO,CAAC,OAAO,CAAC,EAAE,CAAC,C,EAAA,CAAC,CAAC,CAAC;A,CAAA;AAClE,cAAM,EAAoB,OAAO,EAAE,CAAC,IAAI,CAAC,KAAK,CAAC,CAAA,CAAE,CAAC,KAAK,GAAG,OAAO,GAAG,4BAA4B,CAAC,CAAC\"}";

function fixture(){
 const index=5582,specification=input.cases[index],expected=reference.cases[index];
 assert.equal(specification.id,'ge2_module_helpers/helper-name-collision-cjs-es2015-i0-n1/original/external');
 const original=graph.resolve(input,specification,graph.parents(root,input)),name='out/main.js.map';
 assert.deepEqual(JSON.parse(emittedMap).sourcesContent,[original.files['main.ts']]);
 const native=()=>stream(ts,reference.blobs[expected.outputs['out/main.js']],authority.raw(reference,expected,name)),actual=()=>stream(ts,emittedCode,emittedMap);
 const check=changed=>compare(ts,native(),changed,original.options,original.files['main.ts']);
 return {native,actual,check};
}
test('a moved async signature retains the exact Native name-to-signature formatting boundary',()=>{
 const f=fixture(),native=f.native(),actual=f.actual(),proof=f.check(actual),n=proof.frames.native.fn,a=proof.frames.actual.fn;
 const open=actual.tokens(a).find(token=>token.text==='(');
 assert.equal(actual.code.slice(a.name.end,open.start),' ');
 assert.deepEqual(actual.owner(a.name.end),native.owner(n.name.end));
 actual.complete('complete collision module including the signature boundary');
});
test('the identity-to-arity formatting gap cannot acquire an unrelated source role',()=>{
 const f=fixture(),actual=f.actual(),proof=f.check(actual),at=proof.frames.actual.fn.name.end;
 assert.notDeepEqual(actual.owner(at),[0,0,0]);
 assert.throws(()=>f.check(mutate(ts,f.actual(),at,[0,0,0])),/original async identity\/signature boundary/);
});
test('the generated outer arity cannot inherit the adjacent original signature role',()=>{
 const f=fixture(),actual=f.actual(),proof=f.check(actual),fn=proof.frames.actual.fn,open=actual.tokens(fn).find(token=>token.text==='(');
 assert.equal(actual.owner(open.start),null);
 assert.throws(()=>f.check(mutate(ts,f.actual(),open.start,actual.owner(fn.name.end))),/generated original async outer arity/);
});
test('the moved callback signature still rejects a different original role',()=>{
 const f=fixture(),actual=f.actual(),proof=f.check(actual),open=actual.tokens(proof.frames.actual.callback).find(token=>token.text==='(');
 assert.notDeepEqual(actual.owner(open.start),[0,0,0]);
 assert.throws(()=>f.check(mutate(ts,f.actual(),open.start,[0,0,0])),/complete moved original async signature/);
});
