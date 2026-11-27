// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

'use strict';
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const fixture = path.join(repo,'backend/bluets/tests/fixtures/erased_type_regions');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const read = name => JSON.parse(fs.readFileSync(path.join(fixture,name)));
const frozen = read('FROZEN.json'), input = read('cases.json'), reference = read('reference.json');
const controls = read('discovery-controls.json');
const contract = JSON.parse(fs.readFileSync(path.join(__dirname,'../ERASED_TYPE_REGIONS_CONTRACT.json')));
for (const [name,digest] of Object.entries(contract.authorities)) assert.equal(sha(fs.readFileSync(path.join(fixture,name))),digest,name);
const binary = path.resolve(process.argv[2]), root = path.resolve(process.argv[3]);
const implementation = process.env.BLUEICE_TYPESCRIPT_ROOT
    ? path.join(path.resolve(process.env.BLUEICE_TYPESCRIPT_ROOT),'lib/typescript.js')
    : require(path.join(repo,'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'))(
        process.env.BLUEICE_BLUETSC_ORACLE,'typescript','lib/typescript.js');
const tsRoot = path.dirname(path.dirname(implementation)), ts = require(implementation);
const acornImplementation = process.env.BLUEICE_ACORN_ORACLE
    ? require(path.join(repo,'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'))(
        process.env.BLUEICE_ACORN_ORACLE,'acorn','dist/acorn.js')
    : path.resolve(tsRoot,'../acorn/dist/acorn.js');
const acorn = require(acornImplementation);
assert.equal(acorn.version,'8.15.0');
const baseline = process.argv.includes('--baseline');
const surface = process.argv.includes('--native') ? 'native' : 'legacy';
for (const [name,digest] of Object.entries(frozen.sha256)) assert.equal(sha(fs.readFileSync(path.join(fixture,name))),digest,name);
assert.equal(ts.version,'5.9.3');
assert.equal(sha(fs.readFileSync(path.join(tsRoot,'lib/typescript.js'))),frozen.implementationSha256);
for (const [name,digest] of Object.entries(frozen.libraries)) assert.equal(sha(fs.readFileSync(path.join(tsRoot,'lib',name))),digest,name);
assert(!fs.existsSync(root),'retain every previous attempt');fs.mkdirSync(root,{recursive:true});
const write = (name,value) => fs.writeFileSync(name,JSON.stringify(value,null,2)+'\n',{flag:'wx'});
function diagnostic(d,base) {
    const pos = d.file && d.start !== undefined ? d.file.getLineAndCharacterOfPosition(d.start) : null;
    return {code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),
        file:d.file?path.relative(base,d.file.fileName).split(path.sep).join('/'):null,
        position:pos?{line:pos.line+1,column:pos.character+1,length:d.length}:null,
        relatedInformation:(d.relatedInformation||[]).map(row=>diagnostic(row,base))};
}
function native(dir,files,options) {
    fs.mkdirSync(dir,{recursive:true});
    for (const [name,text] of Object.entries(files)) fs.writeFileSync(path.join(dir,name),text,{flag:'wx'});
    // Preserve the original Native recorder's LF bytes on every guest host.
    const converted = ts.convertCompilerOptionsFromJson({...options,newLine:'lf'},dir);assert.deepEqual(converted.errors,[]);
    const outputs={},host=ts.createCompilerHost(converted.options);
    host.writeFile=(name,text)=>{outputs[path.relative(path.join(dir,'out'),name).split(path.sep).join('/')]=text;};
    const program=ts.createProgram(Object.keys(files).map(name=>path.join(dir,name)),converted.options,host);
    const diagnostics=ts.getPreEmitDiagnostics(program).map(row=>diagnostic(row,dir));
    const emitted=program.emit();
    const result={accepts:!diagnostics.length,diagnostics,emitSkipped:emitted.emitSkipped,outputs};
    write(path.join(dir,'observation.json'),result);return result;
}
function outputs(dir) {
    if (!fs.existsSync(dir)) return {};
    const values={};
    function walk(base) { for (const name of fs.readdirSync(base).sort()) {
        const file=path.join(base,name);
        if (fs.statSync(file).isDirectory()) walk(file);
        else values[path.relative(dir,file).split(path.sep).join('/')]=fs.readFileSync(file,'utf8');
    }}
    walk(dir);return values;
}
function same(a,b) { try {assert.deepEqual(a,b);return true;} catch {return false;} }
function comparable(text,name) {
    // Compare complete JSON map objects independently of object-key order.
    return name.endsWith('.map') ? JSON.parse(text) : text;
}
const rows=[], differences=[];
for (const wanted of reference.cases) {
    const entry=input.cases.find(row=>row.id===wanted.form),profile=input.profiles.find(row=>row.id===wanted.profile);
    const dir=path.join(root,wanted.id);fs.mkdirSync(dir,{recursive:true});
    const files={'main.ts':entry.source,...(entry.files||{})};
    const replay=native(path.join(dir,'native'),files,profile.compilerOptions);
    assert.deepEqual(replay,{accepts:wanted.accepts,diagnostics:wanted.diagnostics,emitSkipped:wanted.emitSkipped,outputs:wanted.outputs},wanted.id+': frozen Native authority');
    const build=path.join(dir,'blue');fs.mkdirSync(build);
    for (const [name,text] of Object.entries(files)) fs.writeFileSync(path.join(build,name),text,{flag:'wx'});
    write(path.join(build,'tsconfig.json'),{compilerOptions:{...profile.compilerOptions,newLine:'lf'},files:Object.keys(files)});
    const args = surface==='native' ? ['--project',path.join(build,'tsconfig.json'),'--diagnostics-json'] :
        ['build','--config',path.join(build,'tsconfig.json'),'--diagnostics-json'];
    const ran=cp.spawnSync(binary,args,
        {cwd:build,encoding:'utf8',maxBuffer:16*1024*1024,env:{...process.env,BLUETSC_OWNER_STATE_DIR:path.join(root,'owner-state')}});
    fs.writeFileSync(path.join(build,'stdout'),ran.stdout||'');fs.writeFileSync(path.join(build,'stderr'),ran.stderr||'');
    assert(!ran.error && ran.signal===null,wanted.id+': CLI terminal result');
    const diagnostics=(ran.stderr||'').split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line)).map(row=>{
        if (!row.typescript) return {unmatchedBlueDiagnostic:row};
        const d=row.typescript;
        const related=row=>( {code:row.code,message:row.message,file:row.span.module.split(path.sep).join('/'),
            position:row.position,relatedInformation:[]} );
        return {code:d.code,message:d.message,file:d.span.module.split(path.sep).join('/'),position:d.position,
            relatedInformation:d.relatedInformation.map(related)};
    });
    // The legacy build surface orders internal BTS spans. Compare complete
    // counterparts in Native position order while retaining the raw JSONL.
    diagnostics.sort((a,b)=>(a.file||'').localeCompare(b.file||'') ||
        (a.position?.line||0)-(b.position?.line||0) ||
        (a.position?.column||0)-(b.position?.column||0) ||
        (a.position?.length||0)-(b.position?.length||0) || (a.code||0)-(b.code||0));
    const actual=outputs(path.join(build,'out'));
    const manifest=actual['bluetsc.manifest.json'];
    delete actual['bluetsc.manifest.json'];
    if (ran.status===0 && surface==='legacy') {
        assert(manifest,wanted.id+': owner generation manifest');
        const owned=JSON.parse(manifest).outputOwnership.files;
        assert.deepEqual(Object.keys(owned).sort(),Object.keys(actual).sort(),wanted.id+': complete owned inventory');
        for (const [name,value] of Object.entries(actual))
            assert.deepEqual(owned[name],{bytes:Buffer.byteLength(value),sha256:sha(value)},wanted.id+': owned bytes '+name);
    } else assert.equal(manifest,undefined,wanted.id+': no legacy generation manifest');
    const failed=[];
    const control=controls.cases.find(row=>row.id===wanted.id);
    const expectedAccepts=control ? control.exitCode===0 : wanted.accepts;
    const expectedDiagnostics=control ? control.diagnostics : wanted.diagnostics;
    if ((ran.status===0)!==expectedAccepts) failed.push('verdict');
    if (!same(diagnostics,expectedDiagnostics)) failed.push('diagnostics');
    if (!same(Object.keys(actual).sort(),Object.keys(control?{}:wanted.outputs).sort())) failed.push('output inventory');
    const outputDifferences=[];
    for (const name of Object.keys(wanted.outputs)) if (actual[name]!==undefined &&
        !same(comparable(actual[name],name),comparable(wanted.outputs[name],name))) outputDifferences.push(name);
    if (outputDifferences.some(name=>name.endsWith('.d.ts'))) failed.push('declaration bytes');
    let maps=0,syntaxChecks=0;
    for (const [name,text] of Object.entries(actual)) {
        if (name.endsWith('.map')) {
            const product=actual[name.slice(0,-4)];assert.equal(typeof product,'string');
            require('./assertion_results/common.cjs').completeMap(text,product,files,name);
            maps++;
        } else if (name.endsWith('.js')) {
            require('./contextual_member_results/common.cjs').syntax({acorn},text,profile);
            syntaxChecks++;
        }
    }
    let consumers=0,executions=0;
    if (ran.status===0 && wanted.accepts && !control && !baseline) {
        for (const polarity of ['positive','negative']) {
            const consumerFiles=Object.fromEntries(Object.entries(actual).filter(([name])=>name.endsWith('.d.ts')));
            consumerFiles['consumer.ts']='import {result} from "./main.js";\n'+entry[polarity];
            const checked=native(path.join(dir,'consumer-'+polarity),consumerFiles,
                {...profile.compilerOptions,declaration:false,declarationMap:false,sourceMap:false,noEmit:true});
            const target=reference.consumers.find(row=>row.id===wanted.id&&row.polarity===polarity);
            if (checked.accepts!==target.accepts || !same(checked.diagnostics,target.diagnostics)) failed.push(polarity+' consumer');
            consumers++;
        }
        if (entry.runtime) {
            fs.writeFileSync(path.join(build,'out/package.json'),JSON.stringify({type:profile.compilerOptions.module==='CommonJS'?'commonjs':'module'}));
            fs.writeFileSync(path.join(build,'out/execute.mjs'),'const m = await import("./main.js");\n'+entry.runtime);
            const executed=cp.spawnSync(process.execPath,['--no-maglev',path.join(build,'out/execute.mjs')],{encoding:'utf8'});
            fs.writeFileSync(path.join(build,'execution.stdout'),executed.stdout||'');fs.writeFileSync(path.join(build,'execution.stderr'),executed.stderr||'');
            if (executed.status!==0 || executed.stdout!=='' || executed.stderr!=='') failed.push('execution');
            executions++;
        }
    }
    const row={id:wanted.id,role:control?'separate-open-discovery':'primary',exitCode:ran.status,
        diagnostics,failed,outputDifferences,consumers,executions,maps,syntaxChecks};
    rows.push(row);write(path.join(dir,'comparison.json'),row);
    if (failed.length) differences.push(row);
    process.stdout.write(JSON.stringify({id:wanted.id,failed,outputDifferences})+'\n');
}
const result={complete:differences.length===0&&!baseline,baseline,surface,compilerSha256:sha(fs.readFileSync(binary)),
    hostNormalization:{newLine:'lf',frozenExpectedBytesUnchanged:true},
    acorn:{version:acorn.version,implementationSha256:sha(fs.readFileSync(acornImplementation))},
    frozenSha256:sha(fs.readFileSync(path.join(fixture,'FROZEN.json'))),
    observations:rows.length,primary:rows.filter(row=>row.role==='primary').length,
    separateOpenDiscovery:rows.filter(row=>row.role==='separate-open-discovery').length,
    passed:rows.filter(row=>!row.failed.length).length,
    maps:rows.reduce((n,row)=>n+row.maps,0),syntaxChecks:rows.reduce((n,row)=>n+row.syntaxChecks,0),
    consumers:rows.reduce((n,row)=>n+row.consumers,0),executions:rows.reduce((n,row)=>n+row.executions,0),rows,differences};
write(path.join(root,'result.json'),result);process.exitCode=differences.length?1:0;
