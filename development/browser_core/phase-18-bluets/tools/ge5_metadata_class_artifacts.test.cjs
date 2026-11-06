// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path'),root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),g=require('./ge5_inputs.cjs'),{stream}=require('./ge5_role_stream.cjs'),authority=require('./ge5_native_authority.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),artifacts=require('./ge5_metadata_class_artifacts.cjs'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=g.parents(root,input);
for(const index of[7349,7353])test('surviving original comment exact endpoint is independent of newline gaps '+index,()=>{
 const original=g.resolve(input,input.cases[index],parents),expected=reference.cases[index],code=reference.blobs[expected.outputs['out/barrel.js']],raw=authority.raw(reference,expected,'out/barrel.js.map'),view=stream(ts,code,raw),comment=view.comments().find(comment=>comment.text.startsWith('/*'));assert(comment&&view.owner(comment.end));const check=a=>artifacts.compare(ts,stream(ts,code,raw),a,original.options,original.files['barrel.ts']);check(stream(ts,code,raw));
 for(const owner of[null,[0,0,1]]){assert.notDeepEqual(owner,view.owner(comment.end));assert.throws(()=>check(mutate(ts,view,comment.end,owner)),'exact original comment endpoint');}
});
test('a surviving original comment with an unmapped logical end rejects a source grant',()=>{
 const original='/* original */ export const value=1;\n',code=original+'//# sourceMappingURL=out.js.map\n',raw=JSON.stringify({version:3,file:'out.js',sources:['input.ts'],sourcesContent:[original],names:[],mappings:''}),view=stream(ts,code,raw),comment=view.comments()[0],check=a=>artifacts.compare(ts,stream(ts,code,raw),a,{target:'ES2022',module:'ESNext'},original);assert.equal(view.owner(comment.end),null);check(stream(ts,code,raw));assert.throws(()=>check(mutate(ts,view,comment.end,[0,0,0])),'unmapped original comment end cannot gain source');
});
