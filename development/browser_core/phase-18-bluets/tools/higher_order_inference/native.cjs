// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c = require('./common.cjs');
const {assert,fs,path,fixture,sha,read} = c;

module.exports = function verify() {
    assert.deepEqual(process.argv.slice(2), ['--verify'], 'Native expectations are verification-only');
    const original = fs.readFileSync(path.join(fixture,'reference.json'));
    const inputBytes = fs.readFileSync(path.join(fixture,'cases.json')), input = JSON.parse(inputBytes);
    const ctx = c.context(), root = c.capture('native'), libraries = {}, rows = [];
    c.authorities(ctx);
    for(const profile of input.profiles) for(const entry of input.cases) {
        const {cwd,options,config} = c.prepare(root,input,profile,entry);
        const native = c.nativeProject(ctx,cwd,config);
        Object.assign(libraries,native.libraries);
        const accepts = native.diagnostics.length===0;
        const row = {id:profile.id+'/'+entry.id, accepts,diagnostics:native.diagnostics,
            declarations:Object.fromEntries(Object.entries(native.outputs).filter(([name])=>/\.d\.(ts|mts|cts)$/.test(name))),
            authoredVerdictAgrees:accepts===(entry.nativeExpectedAcceptsByProfile?.[profile.id]??entry.nativeExpectedAccepts)};
        if(accepts)Object.assign(row,c.downstream(ctx,cwd,profile,entry,options,native.outputs));
        else assert.deepEqual(native.outputs,{},row.id+': rejected input published artifacts');
        fs.writeFileSync(path.join(cwd,'observation.json'),JSON.stringify(row,null,2)+'\n');
        rows.push(row);
    }
    const report = {schema:1,typescript:ctx.ts.version,implementationSha256:sha(fs.readFileSync(ctx.implementation)),libraries,
        inputSha256:sha(inputBytes),...c.counts(rows),verdictCalibrationQuestions:rows.filter(row=>!row.authoredVerdictAgrees).map(row=>row.id),cases:rows};
    fs.writeFileSync(path.join(root,'report.json'),JSON.stringify(report,null,2)+'\n');
    assert.deepEqual(report,JSON.parse(original));
    const boundaryBytes = fs.readFileSync(path.join(fixture,'boundary-controls.json'));
    const controls=[];
    for(const entry of JSON.parse(boundaryBytes).controls) {
        const profile={id:'boundaries',compilerOptions:{target:'ES2022',module:'CommonJS'}};
        const files=entry.declaration?{'ambient.d.ts':entry.declaration+'\n'}:{};
        const {cwd,config}=c.prepare(root,input,profile,{...entry,files});
        const native=c.nativeProject(ctx,cwd,config);
        const row={id:entry.id,accepts:native.diagnostics.length===0,diagnostics:native.diagnostics};
        controls.push(row);fs.writeFileSync(path.join(cwd,'observation.json'),JSON.stringify(row,null,2)+'\n');
    }
    const boundaries={typescript:ctx.ts.version,implementationSha256:report.implementationSha256,inputSha256:sha(boundaryBytes),controls,directAgreementCredit:0};
    fs.writeFileSync(path.join(root,'boundaries-report.json'),JSON.stringify(boundaries,null,2)+'\n');
    assert.deepEqual(boundaries,read(path.join(fixture,'boundary-reference.json')));
    assert.deepEqual(fs.readFileSync(path.join(fixture,'reference.json')),original);
    c.authorities(ctx);
    console.log(JSON.stringify({root,conclusion:'success',...c.counts(rows),separateBoundaryControls:controls.length}));
};
