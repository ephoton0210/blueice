// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c = require('./assertion_results/common.cjs');
const observations = require('./assertion_results/observations.cjs');
const {fs,path,assert,sha} = c;
const compiler = fs.realpathSync(process.argv[2]),compilerSha256 = sha(fs.readFileSync(compiler));
if (process.env.BLUEICE_ASSERTION_RESULTS_EXPECTED_COMPILER_SHA256)
    assert.equal(compilerSha256,process.env.BLUEICE_ASSERTION_RESULTS_EXPECTED_COMPILER_SHA256);
const ctx = c.context(),authority = c.authorities(ctx),root = c.capture();
const input = c.read('cases.json'),reference = c.read('reference.json'),sites = c.read('mutation-sites.json');
const consumers = new Map(reference.consumers.map(row => [row.id+'/'+row.polarity,row]));
const executions = new Map(reference.executions.map(row => [row.id,row]));
assert.equal(consumers.size,768);assert.equal(executions.size,384);
const report = {schema:1,root,compiler,compilerSha256,
    sourceRevision:process.env.BLUEICE_ASSERTION_RESULTS_SOURCE_REVISION || null,
    contractSha256:authority.contractSha256,policyAmendmentSha256:authority.policyAmendmentSha256,
    oracleHostCorrectionsSha256:authority.oracleHostCorrectionsSha256,
    frozenInputs:authority.frozen.sha256,
    implementationSha256:authority.frozen.implementationSha256,libraries:authority.frozen.libraries,
    toolHashes:c.toolHashes(),
    conclusion:'in_progress',cases:[],consumers:[],executions:[],syntax:[],counts:{}};
function save() {
    const tmp = path.join(root,'report.tmp');
    fs.writeFileSync(tmp,JSON.stringify(report,null,2)+'\n');
    fs.renameSync(tmp,path.join(root,'report.json'));
}
try {
    save();
    for (const surface of ['native','legacy']) for (const wanted of reference.cases) {
        const entry = input.cases.find(row => row.id === wanted.form);
        const profile = input.profiles.find(row => row.id === wanted.profile);
        const representation = input.representations.find(row => row.id === wanted.representation);
        const transformed = {...entry,source:representation.prefix+entry.source,
            files:Object.fromEntries(Object.entries(entry.files || {}).map(([name,text]) => [name,representation.prefix+text]))};
        const prepared = c.prepare(path.join(root,'producers',surface,representation.id),input,profile,transformed);
        const actual = observations.build(compiler,prepared.cwd,surface,root,['producer',surface,wanted.id].join('/'));
        const inventory = c.compilerArtifacts(path.join(prepared.cwd,'dist'),surface,actual.accepts);
        const row = {id:wanted.id,surface,role:entry.role,...actual,...inventory,completeMaps:{}};
        fs.writeFileSync(path.join(prepared.cwd,'observation.json'),JSON.stringify(row,null,2)+'\n');
        report.cases.push(row);
        const expectedAccepts = entry.role === 'owner' ? entry.bluePolicyExpectedAccepts : wanted.accepts;
        assert.equal(actual.accepts,expectedAccepts,wanted.id+': producer verdict');
        if (entry.role === 'semantic') {
            assert.deepEqual(actual.blueOnlyDiagnostics,[],wanted.id+': unmatched semantic diagnostic');
            assert.deepEqual(actual.diagnostics,wanted.diagnostics,wanted.id+': complete semantic diagnostics');
        } else if (!actual.accepts) {
            const site = sites.cases.find(row => row.id === entry.id).sites[0];
            assert(observations.policyRefusal(actual.rawDiagnostics,site,representation.prefix),
                wanted.id+': refusal must concern readonly/opaque mutation, never an unrelated return error');
        } else assert.equal(actual.rawDiagnostics.filter(row => row.severity === 'error').length,0);
        if (!actual.accepts) assert.deepEqual(inventory.outputs,{},wanted.id+': rejected source emitted output');
        else {
            assert.deepEqual(inventory.outputs,wanted.outputs,wanted.id+': exact complete Native artifact bytes');
            const authored = {'main.ts':transformed.source,...transformed.files};
            for (const [name,text] of Object.entries(inventory.outputs)) {
                if (name.endsWith('.map')) {
                    const decoded = c.completeMap(text,inventory.outputs[name.slice(0,-4)],authored,name);
                    const expected = c.completeMap(wanted.outputs[name],wanted.outputs[name.slice(0,-4)],authored,name);
                    assert.deepEqual(decoded,expected,wanted.id+': complete map '+name);
                    row.completeMaps[name] = {rawSha256:sha(text),decodedSha256:sha(JSON.stringify(decoded)),segments:decoded.segments.length};
                }
                if (name.endsWith('.js')) {
                    c.syntax(ctx,text,profile);
                    report.syntax.push({id:wanted.id,surface,file:name,javascriptSha256:sha(text),completeSelectedEdition:true});
                }
            }
            if (entry.role === 'semantic') {
                for (const polarity of ['positive','negative']) for (const producer of ['native','blue']) for (const engine of ['native','blue']) {
                    const expected = consumers.get(wanted.id+'/'+polarity);assert(expected);
                    const graph = c.declarations(producer === 'native' ? wanted.outputs : inventory.outputs);
                    report.consumers.push(observations.consumer(ctx,compiler,root,input,wanted,surface,profile,producer,engine,expected,graph));
                }
                const expected = executions.get(wanted.id);assert(expected);
                report.executions.push(observations.runtime(root,wanted,surface,profile,inventory.outputs,expected));
            }
        }
        fs.writeFileSync(path.join(prepared.cwd,'observation.json'),JSON.stringify(row,null,2)+'\n');
        if (report.cases.length % 12 === 0) {
            save();console.log(JSON.stringify({producers:report.cases.length,consumers:report.consumers.length,
                executions:report.executions.length,last:wanted.id,surface}));
        }
    }
    const semantic = report.cases.filter(row => row.role === 'semantic'),owner = report.cases.filter(row => row.role === 'owner');
    const declarations = rows => rows.reduce((n,row) => n+Object.keys(c.declarations(row.outputs)).length,0);
    const maps = rows => rows.reduce((n,row) => n+Object.keys(row.completeMaps).length,0);
    const segments = rows => rows.reduce((n,row) => n+Object.values(row.completeMaps).reduce((n,map) => n+map.segments,0),0);
    report.counts = {semanticProducers:semantic.length,ownerProducers:owner.length,
        semanticAccepted:semantic.filter(row => row.accepts).length,semanticRejected:semantic.filter(row => !row.accepts).length,
        exactSemanticDeclarations:declarations(semantic),completeSemanticMaps:maps(semantic),completeSemanticSegments:segments(semantic),
        consumerPrograms:report.consumers.length,consumerDirections:['native/native','native/blue','blue/native','blue/blue'],
        realExecutions:report.executions.length,ownerNegativeRefusalsAtAuthoredMutation:owner.filter(row => !row.accepts).length,
        ownerPositiveControls:owner.filter(row => row.accepts).length,exactOwnerPositiveDeclarations:declarations(owner),
        completeOwnerPositiveMaps:maps(owner),completeOwnerPositiveSegments:segments(owner),
        totalExactDeclarations:declarations(report.cases),totalCompleteMaps:maps(report.cases),totalSyntaxChecks:report.syntax.length};
    assert.deepEqual(report.counts,authority.contract.perActualCompiler,'entire frozen per-compiler contract');
    c.authorities(ctx);assert.equal(sha(fs.readFileSync(compiler)),compilerSha256);
    assert.deepEqual(c.toolHashes(),report.toolHashes,'actual verifier source files changed');
    report.conclusion='complete-assertion-result-contract-passed';save();
    console.log(JSON.stringify({root,compilerSha256,conclusion:report.conclusion,counts:report.counts}));
} catch (error) {
    report.conclusion='failure';report.error=String(error.stack || error);save();
    console.error(JSON.stringify({root,conclusion:report.conclusion,producers:report.cases.length,
        consumers:report.consumers.length,executions:report.executions.length,error:report.error}));
    process.exitCode=1;
}
