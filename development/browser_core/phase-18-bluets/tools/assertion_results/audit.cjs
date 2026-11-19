// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Re-read producer, consumer and execution files rather than trusting totals.
const c = require('./common.cjs');
const {policyRefusal} = require('./observations.cjs');
const {fs,path,assert,sha} = c;
const reportFile = path.resolve(process.argv[2]),root = path.dirname(reportFile);
const report = JSON.parse(fs.readFileSync(reportFile));
const compiler = process.argv[3] || report.compiler;
const output = process.argv[4] ? path.resolve(process.argv[4]) : path.join(root,'independent-audit.json');
assert(!fs.existsSync(output),'never overwrite an earlier independent audit');
const ctx = c.context(),authority = c.authorities(ctx),input = c.read('cases.json'),reference = c.read('reference.json');
const mutation = c.read('mutation-sites.json');
const read = file => JSON.parse(fs.readFileSync(file));
const byId = new Map(reference.cases.map(row => [row.id,row]));
const expectedConsumers = new Map(reference.consumers.map(row => [row.id+'/'+row.polarity,row]));
const expectedExecutions = new Map(reference.executions.map(row => [row.id,row]));
const physical = [],coordinates = [],syntax = [];
const physicalByPath = new Map();
let physicalReads = 0;
const sourceSizes = new Map();
const counts = {semanticProducers:0,ownerProducers:0,semanticAccepted:0,semanticRejected:0,
    exactSemanticDeclarations:0,completeSemanticMaps:0,completeSemanticSegments:0,consumerPrograms:0,
    consumerDirections:['native/native','native/blue','blue/native','blue/blue'],realExecutions:0,
    ownerNegativeRefusalsAtAuthoredMutation:0,ownerPositiveControls:0,exactOwnerPositiveDeclarations:0,
    completeOwnerPositiveMaps:0,completeOwnerPositiveSegments:0,totalExactDeclarations:0,totalCompleteMaps:0,totalSyntaxChecks:0};
function bytes(file, expected) {
    const value = fs.readFileSync(file);
    if (expected !== undefined) assert(value.equals(Buffer.from(expected,'utf8')),file+': exact bytes');
    physicalReads++;
    const row = {path:path.relative(root,file),sha256:sha(value),bytes:value.length};
    if (physicalByPath.has(row.path)) assert.deepEqual(row,physicalByPath.get(row.path),'file changed between reads');
    else {physicalByPath.set(row.path,row);physical.push(row);}
    return value;
}
function errorOutput(directory) {
    return bytes(path.join(directory,'build.stderr')).toString('utf8').split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line));
}
try {
    assert.equal(report.conclusion,'complete-assertion-result-contract-passed');
    assert.equal(sha(fs.readFileSync(compiler)),report.compilerSha256);
    assert.equal(report.contractSha256,authority.contractSha256);
    assert.equal(report.policyAmendmentSha256,authority.policyAmendmentSha256);
    assert.equal(report.oracleHostCorrectionsSha256,authority.oracleHostCorrectionsSha256);
    if (report.toolHashes) assert.deepEqual(report.toolHashes,c.toolHashes(),'verifier source identity');
    assert.equal(report.cases.length,1296);
    const observed = new Map(report.cases.map(row => [row.surface+'/'+row.id,row]));
    assert.equal(observed.size,1296);
    for (const surface of ['native','legacy']) for (const wanted of reference.cases) {
        const row = observed.get(surface+'/'+wanted.id);assert(row);
        const entry = input.cases.find(entry=>entry.id===wanted.form);
        const profile = input.profiles.find(profile=>profile.id===wanted.profile);
        const prefix = input.representations.find(rep=>rep.id===wanted.representation).prefix;
        const directory = path.join(root,'producers',surface,wanted.representation,wanted.profile,wanted.form);
        const authored = Object.fromEntries(Object.entries({'main.ts':entry.source,...entry.files}).map(([name,text])=>[name,prefix+text]));
        for (const [name,text] of Object.entries(authored)) bytes(path.join(directory,name),text);
        const raw = errorOutput(directory);assert.deepEqual(raw,row.rawDiagnostics);
        const errors = raw.filter(value=>value.severity==='error');
        const accepts = entry.role==='owner'?entry.bluePolicyExpectedAccepts:wanted.accepts;
        assert.equal(row.accepts,accepts);
        assert.equal(row.exitCode===0,accepts);
        assert.deepEqual(read(path.join(directory,'observation.json')),row);
        counts[entry.role==='owner'?'ownerProducers':'semanticProducers']++;
        if (entry.role==='semantic') {
            assert.deepEqual(errors.map(error=>{assert(error.typescript);return c.blueDiagnostic(error);}),wanted.diagnostics);
            counts[accepts?'semanticAccepted':'semanticRejected']++;
        } else if (!accepts) {
            assert(policyRefusal(raw,mutation.cases.find(value=>value.id===entry.id).sites[0],prefix));
            counts.ownerNegativeRefusalsAtAuthoredMutation++;
        } else {assert.deepEqual(errors,[]);counts.ownerPositiveControls++;}
        const inventory = c.compilerArtifacts(path.join(directory,'dist'),surface,accepts);
        assert.deepEqual(inventory.outputs,accepts?wanted.outputs:{});
        assert.deepEqual(inventory.rawInventory,row.rawInventory);
        assert.deepEqual(inventory.ownerManifest,row.ownerManifest);
        for (const [name,digest] of Object.entries(inventory.rawInventory)) {
            const value = bytes(path.join(directory,'dist',name));
            assert.equal(sha(value),digest.sha256);assert.equal(value.length,digest.bytes);
        }
        for (const [name,text] of Object.entries(inventory.outputs)) {
            bytes(path.join(directory,'dist',name),wanted.outputs[name]);
            if (/\.d\.(ts|mts|cts)$/.test(name)) {
                counts.totalExactDeclarations++;
                counts[entry.role==='owner'?'exactOwnerPositiveDeclarations':'exactSemanticDeclarations']++;
            }
            if (name.endsWith('.map')) {
                const decoded = c.completeMap(text,inventory.outputs[name.slice(0,-4)],authored,name);
                assert.deepEqual(decoded,c.completeMap(wanted.outputs[name],wanted.outputs[name.slice(0,-4)],authored,name));
                counts.totalCompleteMaps++;
                counts[entry.role==='owner'?'completeOwnerPositiveMaps':'completeSemanticMaps']++;
                counts[entry.role==='owner'?'completeOwnerPositiveSegments':'completeSemanticSegments']+=decoded.segments.length;
                coordinates.push({id:wanted.id,surface,file:name,segments:decoded.segments.length,decodedSha256:sha(JSON.stringify(decoded))});
            }
            if (name.endsWith('.js')) {
                c.syntax(ctx,text,profile);counts.totalSyntaxChecks++;
                syntax.push({id:wanted.id,surface,file:name,sha256:sha(text)});
            }
        }
        sourceSizes.set(surface+'/'+wanted.id,{directory,outputs:inventory.outputs});
    }
    assert.equal(report.consumers.length,6144);
    const consumerKeys = new Set();
    for (const row of report.consumers) {
        const key=[row.surface,row.producer,row.engine,row.id,row.polarity].join('/');
        assert(!consumerKeys.has(key));consumerKeys.add(key);
        assert(['native','legacy'].includes(row.surface)&&['native','blue'].includes(row.producer)&&['native','blue'].includes(row.engine));
        const producer = byId.get(row.id);assert(producer&&producer.role==='semantic'&&producer.accepts);
        const expected = expectedConsumers.get(row.id+'/'+row.polarity);assert(expected);
        const directory = path.join(root,'consumers',row.surface,row.producer,row.engine,row.id,row.polarity);
        bytes(path.join(directory,'consumer.ts'),expected.source);
        const source = sourceSizes.get(row.surface+'/'+row.id);
        const graph = c.declarations(row.producer==='native'?producer.outputs:source.outputs);
        for (const [name,text] of Object.entries(graph)) bytes(path.join(directory,name),text);
        const config = read(path.join(directory,'tsconfig.json'));
        assert.equal(config.compilerOptions.noEmit,true);
        assert.deepEqual(config.files,['consumer.ts',...Object.keys(graph)].sort());
        const actual = read(path.join(directory,'observation.json'));
        assert.deepEqual(actual,{accepts:expected.accepts,diagnostics:expected.diagnostics});
        assert.equal(row.sourceSha256,expected.sourceSha256);
        assert.equal(sha(expected.source),row.sourceSha256);
        assert.equal(row.accepts,actual.accepts);assert.deepEqual(row.diagnostics,actual.diagnostics);
        if (row.engine==='blue') {
            const errors = errorOutput(directory).filter(value=>value.severity==='error');
            assert.deepEqual(errors.map(error=>{assert(error.typescript);return c.blueDiagnostic(error);}),expected.diagnostics);
            assert.deepEqual(c.inventory(path.join(directory,'dist')),{});
        }
        counts.consumerPrograms++;
    }
    assert.equal(report.executions.length,768);
    const executionKeys = new Set();
    for (const row of report.executions) {
        const key=row.surface+'/'+row.id;assert(!executionKeys.has(key));executionKeys.add(key);
        const expected = expectedExecutions.get(row.id);assert(expected);
        const directory=path.join(root,'runtime',row.surface,row.id),source=sourceSizes.get(key);
        const profile=input.profiles.find(profile=>profile.id===byId.get(row.id).profile);
        const esm=profile.compilerOptions.module==='ESNext';
        assert.equal(sha(bytes(path.join(directory,esm?'check.mjs':'check.cjs'))),expected.runnerSha256);
        assert.equal(read(path.join(directory,'package.json')).type,esm?'module':'commonjs');
        assert.deepEqual(read(path.join(directory,'observation.json')),{exit:expected.exit,stdout:expected.stdout,stderr:expected.stderr,runnerSha256:expected.runnerSha256});
        assert.equal(row.exit,expected.exit);assert.equal(row.stdout,expected.stdout);assert.equal(row.stderr,expected.stderr);
        assert.equal(row.unmodifiedProductJavaScript,true);
        for (const [name,text] of Object.entries(source.outputs).filter(([name])=>name.endsWith('.js'))) {
            assert.equal(sha(bytes(path.join(directory,name),text)),row.products[name]);
        }
        counts.realExecutions++;
    }
    assert.deepEqual(counts,authority.contract.perActualCompiler);
    const audit={conclusion:'complete-assertion-physical-producers-consumers-runtime-and-maps-audited',
        compilerSha256:report.compilerSha256,sourceRevision:report.sourceRevision,
        reportSha256:sha(fs.readFileSync(reportFile)),contractSha256:authority.contractSha256,
        policyAmendmentSha256:authority.policyAmendmentSha256,
        oracleHostCorrectionsSha256:authority.oracleHostCorrectionsSha256,
        counts,physicalFiles:physical.length,physicalReads,physical,coordinates,syntax,
        auditToolSha256:sha(fs.readFileSync(__filename)),wholePackageComplete:false};
    fs.writeFileSync(output,JSON.stringify(audit,null,2)+'\n',{flag:'wx'});
    console.log(JSON.stringify({conclusion:audit.conclusion,compilerSha256:report.compilerSha256,physicalFiles:physical.length,physicalReads,counts}));
} catch(error) {
    const failed={conclusion:'failure',error:String(error.stack||error),counts,physicalFiles:physical.length};
    fs.writeFileSync(output+'.failure.json',JSON.stringify(failed,null,2)+'\n',{flag:'wx'});
    console.error(JSON.stringify(failed));process.exitCode=1;
}
