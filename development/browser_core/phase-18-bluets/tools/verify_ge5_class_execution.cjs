// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),execution=require('./ge5_class_execution.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input);
const observations=JSON.parse(fs.readFileSync(0,'utf8'));const identity=require('./ge5_class_cases.cjs').verify(root,input,reference,observations);
(async()=>{
  const verified=[],failures=[];
  for(const row of observations) {
    const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];
    const native=Object.fromEntries(Object.entries(expected.outputs).map(([name,hash])=>[name,reference.blobs[hash]]));
    try{verified.push({index:row.index,id:row.id,...await execution.run(native,row.outputs,original.files,original.options)});}catch(error){failures.push({index:row.index,id:row.id,error:error.message,stack:error.stack});}
  }
  const report={...identity,scope:'actual class/super artifact executions; map role parity remains separately required',configurations:observations.length,executions:verified.reduce((sum,row)=>sum+row.executions,0),verified,failures};
  if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'class-execution.json'),JSON.stringify(report,null,2)+'\n');}
  console.log(JSON.stringify({scope:report.scope,configurations:report.configurations,executions:report.executions,verified:verified.length,failures},null,2));if(failures.length)process.exitCode=1;
})().catch(error=>{console.error(error);process.exitCode=1;});
