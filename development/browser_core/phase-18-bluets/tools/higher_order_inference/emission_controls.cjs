// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Supplementary emitter regressions; no compatibility-inventory credit.
const {spawnSync} = require('node:child_process');
const c = require('./common.cjs');
const {assert,fs,path,sha} = c;
const cases = [
    {id:'returned-shorthand',source:'export function box<T>(value: T): { value: T } { return { value }; }\nexport const result = box(3);\n',check:'subject.result.value !== 3'},
    {id:'generic-method-constraint',source:'export const provider = { map<T extends (input: number) => number>(value: T): T[] { return [value]; } };\nexport const result = provider.map((input: number): number => input + 1);\n',check:'subject.result[0](3) !== 4'},
    {id:'renamed-block-binding-key',source:'export function read(): { value: number } { let answer = { value: 0 }; { let value = 4; answer = { value }; } return answer; }\nexport const result = read();\n',check:'subject.result.value !== 4 || Object.keys(subject.result).join() !== "value"'},
    {id:'label-block-and-nested-objects',source:'export const value = 5;\ncheckpoint: { value }\nexport const result = { nested: { value }, values: [{ value }], value };\n',check:'subject.result.value !== 5 || subject.result.nested.value !== 5 || subject.result.values[0].value !== 5'},
];

function observe(ctx,cwd,profile,outputs,entry) {
    const javascriptSha256 = {};
    for(const [name,source] of Object.entries(outputs).filter(([name])=>name.endsWith('.js'))) {
        ctx.acorn.parse(source,{ecmaVersion:5,sourceType:profile.compilerOptions.module==='CommonJS'?'script':'module'});
        javascriptSha256[name]=sha(source);
    }
    const esm=profile.compilerOptions.module==='ESNext';
    fs.writeFileSync(path.join(cwd,'package.json'),JSON.stringify({type:esm?'module':'commonjs'})+'\n');
    const runner=path.join(cwd,esm?'check.mjs':'check.cjs');
    fs.writeFileSync(runner,(esm?'import * as subject from "./dist/main.js";\n':'var subject = require("./dist/main.js");\n')+
        'if ('+entry.check+') throw new Error("runtime relationship changed");\n');
    const result=spawnSync(process.execPath,[runner],{cwd,encoding:'utf8',timeout:120000});
    if(result.error)throw result.error;
    const runtime={exit:result.status,stdout:result.stdout,stderr:result.stderr};
    fs.writeFileSync(path.join(cwd,'runtime.json'),JSON.stringify(runtime,null,2)+'\n');
    assert.equal(result.status,0,entry.id+': actual runtime: '+result.stderr);
    return {javascriptSha256,runtime};
}

module.exports=function verify() {
    const expectRed=process.argv[2]==='--expect-red';
    assert.equal(process.argv.length,expectRed?3:2);
    const compiler=fs.realpathSync(process.env.BLUEICE_HIGHER_ORDER_INFERENCE_COMPILER);
    const compilerSha256=sha(fs.readFileSync(compiler));
    const ctx=c.context(),root=c.capture('emission-controls');
    const report={schema:1,conclusion:'in_progress',compilerSha256,toolSha256:sha(fs.readFileSync(__filename)),
        implementationSha256:sha(fs.readFileSync(ctx.implementation)),inventoryCredit:0,cases:[]};
    const save=()=>fs.writeFileSync(path.join(root,'report.json'),JSON.stringify(report,null,2)+'\n');
    const input={options:{strict:true,declaration:true,noEmitOnError:true,newLine:'lf',lib:['ES5']}};
    try {
        c.authorities(ctx);save();
        for(const module of ['CommonJS','ESNext'])for(const entry of cases) {
            const profile={id:'es5-'+module.toLowerCase(),compilerOptions:{target:'ES5',module}};
            const native=c.prepare(path.join(root,'native'),input,profile,entry);
            const wanted=c.nativeProject(ctx,native.cwd,native.config);
            assert.deepEqual(wanted.diagnostics,[],entry.id+': Native authoring question');
            const nativeRuntime=observe(ctx,native.cwd,profile,wanted.outputs,entry);
            const actual=c.prepare(path.join(root,'blue'),input,profile,entry);
            const row={id:profile.id+'/'+entry.id,nativeRuntime};
            try {
                const result=spawnSync(compiler,['build','--config','tsconfig.json','--diagnostics-json'],{cwd:actual.cwd,encoding:'utf8',timeout:120000});
                fs.writeFileSync(path.join(actual.cwd,'build.stdout'),result.stdout||'');
                fs.writeFileSync(path.join(actual.cwd,'build.stderr'),result.stderr||'');
                if(result.error)throw result.error;
                assert.equal(result.status,0,result.stderr);
                const outputs=c.inventory(path.join(actual.cwd,'dist'));
                const declarations=out=>Object.fromEntries(Object.entries(out).filter(([name])=>name.endsWith('.d.ts')));
                row.nativeDeclarations=declarations(wanted.outputs);row.blueDeclarations=declarations(outputs);
                assert.deepEqual(row.blueDeclarations,row.nativeDeclarations,entry.id+': exact declarations');
                row.blueRuntime=observe(ctx,actual.cwd,profile,outputs,entry);
                assert.deepEqual(row.blueRuntime.runtime,nativeRuntime.runtime,entry.id+': runtime agreement');
                row.agrees=true;
            }catch(error){row.agrees=false;row.error=String(error.stack||error);}
            report.cases.push(row);save();
        }
        const differences=report.cases.filter(row=>!row.agrees).length;
        Object.assign(report,{comparisons:report.cases.length,differences});
        assert.equal(report.comparisons,8);
        c.authorities(ctx);assert.equal(sha(fs.readFileSync(compiler)),compilerSha256);
        assert.equal(expectRed?differences>0:differences===0,true,JSON.stringify(report.cases.filter(row=>!row.agrees)));
        report.conclusion=expectRed?'expected-red':'success';save();
        console.log(JSON.stringify({root,conclusion:report.conclusion,comparisons:report.comparisons,differences,inventoryCredit:0}));
    }catch(error){report.conclusion='failure';report.error=String(error.stack||error);save();throw error;}
};
if(require.main===module)module.exports();
