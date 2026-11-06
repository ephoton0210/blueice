// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),graph=require('./ge5_inputs.cjs');
const forms=new Map([['ge1_downlevel/called-imported-heritage',[51,102]],['ge1_downlevel/indexed-heritage',[51,51]],['ge1_downlevel/computed-constructor-spread',[51,51]],['ge1_downlevel/constructor-and-super-spread',[51,51]],['ge1_downlevel/computed-super-call-order',[51,51]],['ge1_downlevel/super-write-receiver-order',[43,43]],['ge2_module_helpers/inherit-constructor-spread',[363,363]]]);
function verify(root,input,reference,observations,selectedForms=[...forms.keys()]) {
  assert(selectedForms.every(form=>forms.has(form)));assert.equal(new Set(selectedForms).size,selectedForms.length);const selectedSet=new Set(selectedForms);
  assert.equal(graph.sha256(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'))),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
  assert.equal(graph.sha256(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'))),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
  const selected=input.cases.flatMap((row,index)=>selectedSet.has(row.form)?[index]:[]);assert.deepEqual(observations.map(row=>row.index),selected);assert.equal(selected.length,selectedForms.reduce((sum,form)=>sum+forms.get(form)[0],0));
  const compilerSha256=graph.sha256(fs.readFileSync(path.join(root,'target/debug/bluetsc')));
  for(const row of observations){assert.equal(row.compilerSha256,compilerSha256);assert.equal(row.repeatBuilds,2);assert.equal(row.id,reference.cases[row.index].id);}
  for(const [form,[configurations,maps]]of [...forms].filter(([form])=>selectedSet.has(form))) {
    const rows=selected.filter(index=>input.cases[index].form===form);assert.equal(rows.length,configurations);assert.equal(rows.reduce((sum,index)=>sum+Object.values(reference.cases[index].maps).filter(map=>map.kind==='javascript').length,0),maps);
  }
  return {compilerSha256,repeatBuilds:2,forms:Object.fromEntries([...forms].filter(([form])=>selectedSet.has(form))),inputSha256:'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e',referenceSha256:'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3'};
}
module.exports={forms,verify};
