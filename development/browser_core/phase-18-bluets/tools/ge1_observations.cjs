// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Bounded real-process completion, output and rejection observations.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {spawnSync}=require('node:child_process'),{pathToFileURL}=require('node:url');
const {owned,write,imports}=require('./gm1_observations.cjs');
function execute(row,outputs){
  const root=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'bluets-ge1-run-')));
  try{
    write(root,row.files);write(root,outputs);
    write(root,{'out/package.json':JSON.stringify({type:row.runtimeModule==='CommonJS'?'commonjs':'module'})});
    const entry=owned(root,'out/main.js');
    const load=row.runtimeModule==='CommonJS'?'Promise.resolve().then(()=>require('+JSON.stringify(entry)+'))':'import('+JSON.stringify(pathToFileURL(entry).href)+')';
    const script=load+'.then(async m=>process.stdout.write(JSON.stringify(await m.result))).catch(e=>{process.stderr.write(e.name+": "+e.message);process.exitCode=1;});';
    const result=spawnSync(process.execPath,['-e',script],{encoding:'utf8',timeout:10000,maxBuffer:2*1024*1024});
    if(result.error)throw result.error;
    return {exit:result.status,stdout:result.stdout,stderr:result.stderr.replaceAll(root,'<root>')};
  }finally{fs.rmSync(root,{recursive:true,force:true});}
}
module.exports={owned,write,imports,execute};
