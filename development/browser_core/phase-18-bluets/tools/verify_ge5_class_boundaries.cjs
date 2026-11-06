// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ordinary public regression: complete class/module roles at exact AST boundaries.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),{compare}=require('./ge5_class_module.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input);
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.deepEqual(rows.map(row=>row.index),[264,265,312,313,360,361,1000,1001,1040,1041]);let maps=0;
for(const row of rows) {
  const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];
  for(const [name,map]of Object.entries(expected.maps)) {
    if(map.kind!=='javascript')continue;
    const native=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),actual=stream(ts,row.outputs[map.output],row.outputs[name]);
    const source=JSON.parse(row.outputs[name]).sources[0].replace(/^.*\//,'');
    compare(ts,native,actual,original.options,original.files[source]);maps++;
  }
}
console.log(JSON.stringify({ordinaryConfigurations:rows.length,maps,completeTokenInteriorsAndOrderedGaps:true}));
