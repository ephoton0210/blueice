// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test=require('node:test');
const os=require('node:os');
const c=require('./higher_order_inference/common.cjs');
const {assert,fs,path,read,fixture}=c;
const input=read(path.join(fixture,'cases.json'));
const original=input.cases.find(row=>row.id==='runtime-imported-compose');
for(const module of ['CommonJS','ESNext'])for(const rejected of [false,true]) {
    test(module+': '+(rejected?'reject a failing execution through actual imported output':'execute actual imported compiler output'),()=>{
        const root=fs.mkdtempSync(path.join(os.tmpdir(),'bluets-higher-order-runtime-test-'));
        try {
            const profile={id:module,compilerOptions:{target:'ES2022',module,strictFunctionTypes:true}};
            const entry={...original,runtime:rejected?'throw new Error("authored runtime failure");\n':original.runtime};
            const ctx=c.context();
            const {cwd,options,config}=c.prepare(root,input,profile,entry);
            const native=c.nativeProject(ctx,cwd,config);assert.deepEqual(native.diagnostics,[]);
            if(rejected)assert.throws(()=>c.downstream(ctx,cwd,profile,entry,options,native.outputs),/authored runtime failure/);
            else assert.deepEqual(c.downstream(ctx,cwd,profile,entry,options,native.outputs).runtime,{exit:0,stdout:'',stderr:''});
        }finally{fs.rmSync(root,{recursive:true,force:true});}
    });
}
