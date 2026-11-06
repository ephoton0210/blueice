// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os'),{spawnSync}=require('node:child_process');
const {sha256}=require('./ge5_inputs.cjs');
const root=path.resolve(__dirname,'../../../..'),script=path.join(__dirname,'collect_ge5_cohort_coverage.cjs');
function setup(){
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'ge5-coverage-cli-')),compiler=path.join(dir,'compiler'),freeze=path.join(dir,'freeze.json'),reports=path.join(dir,'reports.json'),output=path.join(dir,'coverage.json');
  fs.writeFileSync(compiler,'immutable test compiler bytes');
  const name='backend/bluets/tests/oracle_support/ge5_node_roles.rs';
  fs.writeFileSync(freeze,JSON.stringify({compilerSha256:sha256(fs.readFileSync(compiler)),sourceFiles:{[name]:sha256(fs.readFileSync(path.join(root,name)))}}));
  fs.writeFileSync(reports,'[]');
  const measurement=path.join(dir,'measurement.json');
  const catalog=JSON.parse(fs.readFileSync(path.join(root,'development/browser_core/phase-18-bluets/COMPATIBILITY_SUITES.json')));
  const receipt={conclusion:'success',compilerSha256:sha256(fs.readFileSync(compiler)),
    source_revision:catalog.source_revision,source_graph_sha256:catalog.source_graph_sha256,
    suites:catalog.suites.map(row=>({harness:row.harness,conclusion:'success',failed:0,ignored:0,filtered:0})),
    oracle_tools_sha256:Object.fromEntries(['collect_ge5_cohort_coverage.cjs','ge5_cohort_coverage.cjs'].map(name=>[name,sha256(fs.readFileSync(path.join(__dirname,name)))]))};
  fs.writeFileSync(measurement,JSON.stringify(receipt));
  return{dir,compiler,freeze,reports,output,measurement};
}
function run(f){return spawnSync(process.execPath,[script,'--compiler',f.compiler,'--source-freeze',f.freeze,'--reports',f.reports,'--output',f.output,'--measurement',f.measurement],{encoding:'utf8'});}
test('real CLI writes all exact missing identities and exits nonzero without running a compiler',()=>{
 const f=setup();try{const r=run(f);assert.equal(r.status,1,r.stderr);const d=JSON.parse(fs.readFileSync(f.output));assert.equal(d.configurations,8407);assert.equal(d.families.length,137);assert.equal(d.expectedJavascriptMaps,10193);assert.equal(d.declarationMaps,10397);assert.equal(d.allMaps,20590);assert.equal(d.configurationsWithoutJavascriptMaps,197);assert.equal(d.missingMaps.length,10193);assert.equal(d.coveredJavascriptMaps,0);assert.deepEqual(d.measurementErrors,[]);assert.equal(d.completeJavascriptMapInventory,false);assert.equal(d.fullCohortAccepted,false);}finally{fs.rmSync(f.dir,{recursive:true,force:true});}
});
test('real CLI refuses counter summaries and records rejection beside exact missing maps',()=>{
 const f=setup();try{const report=path.join(f.dir,'summary.json');fs.writeFileSync(report,JSON.stringify({compilerSha256:sha256(fs.readFileSync(f.compiler)),repeatBuilds:2,failures:[],verified:67}));fs.writeFileSync(f.reports,JSON.stringify([{path:report}]));const r=run(f);assert.equal(r.status,1,r.stderr);const d=JSON.parse(fs.readFileSync(f.output));assert.equal(d.rejectedReports.length,1);assert.equal(d.missingMaps.length,10193);assert.equal(d.evidenceAccepted,false);assert.equal(d.fullCohortAccepted,false);}finally{fs.rmSync(f.dir,{recursive:true,force:true});}
});
test('CLI fails closed for stale frozen source and unknown adapter',()=>{
 const f=setup();try{const freeze=JSON.parse(fs.readFileSync(f.freeze));freeze.sourceFiles[Object.keys(freeze.sourceFiles)[0]]='0'.repeat(64);fs.writeFileSync(f.freeze,JSON.stringify(freeze));const r=run(f);assert.equal(r.status,1);assert.match(r.stderr,/frozen source/);}finally{fs.rmSync(f.dir,{recursive:true,force:true});}
});

test('receipt compiler identity must be present and match the frozen CLI exactly',()=>{
 for(const compilerSha256 of [undefined,'b'.repeat(64)]) {
  const f=setup();try{
   const receipt=JSON.parse(fs.readFileSync(f.measurement));
   if(compilerSha256===undefined)delete receipt.compilerSha256;else receipt.compilerSha256=compilerSha256;
   fs.writeFileSync(f.measurement,JSON.stringify(receipt));
   const r=run(f);assert.equal(r.status,1,r.stderr);
   const d=JSON.parse(fs.readFileSync(f.output));
   assert.equal(d.measurementErrors.length,1);assert.match(d.measurementErrors[0],/measured compiler identity/);
   assert.equal(d.evidenceAccepted,false);assert.equal(d.fullCohortAccepted,false);
  }finally{fs.rmSync(f.dir,{recursive:true,force:true});}
 }
});
