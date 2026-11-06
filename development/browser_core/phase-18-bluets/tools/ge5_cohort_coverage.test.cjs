// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test'), assert = require('node:assert/strict');
const {sha256} = require('./ge5_inputs.cjs');
const {inventory, auditReport} = require('./ge5_cohort_coverage.cjs');
const compiler = 'a'.repeat(64);
function fixture() {
  const input = {cases: [{id: 'a/0', form: 'a'}, {id: 'a/1', form: 'a'}, {id: 'types/0', form: 'types'}]};
  const reference = {cases: [], blobs: {}};
  const verified = [];
  for (const [index, row] of input.cases.entries()) {
    const expected = {id: row.id, outputs: {}, maps: {}};
    for (const name of index === 0 ? ['main'] : index === 1 ? ['main', 'provider'] : []) {
      const output = name + '.js', map = output + '.map';
      const javascript = `const ${name} = ${index};`, raw = JSON.stringify({file: output, mappings: 'AAAA'});
      const jsHash = sha256(javascript), mapHash = sha256(raw);
      reference.blobs[jsHash] = javascript; reference.blobs[mapHash] = raw;
      expected.outputs[output] = jsHash; expected.outputs[map] = mapHash;
      expected.maps[map] = {kind: 'javascript', output, raw: mapHash, coordinateIssues: []};
      verified.push({index, id: row.id, map, allCharactersClassified: true,
        nativeJavaScriptSha256: jsHash, nativeMapSha256: mapHash,
        actualJavaScriptSha256: sha256(javascript + ' '), actualMapSha256: mapHash});
    }
    reference.cases.push(expected);
  }
  return {input, reference, report: {compilerSha256: compiler, repeatBuilds: 2, failures: [], verified}};
}

test('coverage uses exact artifact identities and deduplicates identical overlap', () => {
  const {input, reference, report} = fixture();
  const result = inventory(input, reference, [report, structuredClone(report)], compiler);
  assert.equal(result.expectedJavascriptMaps, 3);
  assert.equal(result.coveredJavascriptMaps, 3);
  assert.equal(result.duplicateConsistentRows, 3);
  assert.deepEqual(result.missingMaps, []);
  assert.equal(result.configurationsWithoutJavascriptMaps, 1);
  assert.equal(result.completeJavascriptMapInventory, true);
  assert.equal(result.fullCohortAccepted, false, 'inventory never certifies role/runtime/process acceptance');
});

test('a missing provider map remains missing despite extra consistent consumer reports', () => {
  const {input, reference, report} = fixture();
  report.verified.pop();
  const result = inventory(input, reference, [report, structuredClone(report)], compiler);
  assert.equal(result.completeJavascriptMapInventory, false);
  assert.equal(result.coveredJavascriptMaps, 2);
  assert.deepEqual(result.missingMaps.map(row => [row.id, row.map]), [['a/1', 'provider.js.map']]);
});

test('coverage rejects stale, partial, misidentified and foreign Native evidence', () => {
  for (const alter of [
    report => { report.compilerSha256 = 'b'.repeat(64); },
    report => { report.repeatBuilds = 1; },
    report => { report.failures.push({error: 'failed'}); },
    report => { report.probeOnly = true; },
    report => { report.completeFamily = false; },
    report => { report.verified[0].id = 'a/1'; },
    report => { report.verified[0].index = -1; },
    report => { report.verified[0].map = 'foreign.js.map'; },
    report => { report.verified[0].allCharactersClassified = false; },
    report => { report.verified[0].nativeMapSha256 = 'c'.repeat(64); },
    report => { report.verified[0].nativeJavaScriptSha256 = 'd'.repeat(64); },
    report => { report.verified[0].actualMapSha256 = 'unbound'; },
  ]) {
    const {input, reference, report} = fixture(); alter(report);
    assert.throws(() => inventory(input, reference, [report], compiler));
  }
});

test('overlapping observations cannot hide nondeterministic artifacts or duplicate case identities', () => {
  const {input, reference, report} = fixture(), changed = structuredClone(report);
  changed.verified[0].actualJavaScriptSha256 = 'e'.repeat(64);
  assert.throws(() => inventory(input, reference, [report, changed], compiler));
  input.cases[1].id = input.cases[0].id;
  reference.cases[1].id = input.cases[0].id;
  assert.throws(() => inventory(input, reference, [report], compiler));
});

function staticFixture(inline = false) {
  const form = 'ge3_decorator_compositions/static-super-computed-reads';
  const input = {cases:[{id:'static/0',form}]};
  const raw = JSON.stringify({version:3,file:'main.js',sources:['main.ts'],sourcesContent:['x'],names:[],mappings:'AAAA'})+'\r\n';
  const code = 'const x=1;\n'+(inline?'//# sourceMappingURL=data:application/json;base64,'+Buffer.from(raw).toString('base64')+'\n':'');
  const map = inline?'main.js#inline':'main.js.map', outputs = {'main.js':code}; if(!inline)outputs[map]=raw;
  const jsHash=sha256(code),mapHash=sha256(raw),reference={cases:[{id:'static/0',outputs:{'main.js':jsHash},maps:{[map]:{kind:'javascript',output:'main.js',raw:mapHash,coordinateIssues:[]}}}],blobs:{[jsHash]:code,[mapHash]:raw}};
  const observations=[{index:0,id:'static/0',compilerSha256:compiler,repeatBuilds:2,outputs}];
  const record={index:0,id:'static/0',map,completeModuleRoles:true,controls:[{rejected:true}],nativeJavaScriptSha256:jsHash,nativeMapSha256:mapHash,actualJavaScriptSha256:jsHash,actualMapSha256:mapHash};
  const report={compilerSha256:compiler,frozenConfigurations:1,probeOnly:false,completeFamily:true,failures:[],verified:[record],executions:[{index:0,executions:2}],totalExecutions:2};
  return{input,reference,observations,report,raw};
}

test('static adapter derives two builds from actual observations and preserves its real proof marker',()=>{
  for(const inline of [false,true]) {
    const f=staticFixture(inline),report=auditReport(f.input,f.reference,f.report,f.observations,compiler,'static-super');
    assert.equal(report.repeatBuilds,2);assert.equal(report.verified[0].allCharactersClassified,undefined);
    assert.equal(inventory(f.input,f.reference,[report],compiler).coveredJavascriptMaps,1);
    assert.throws(()=>inventory(f.input,f.reference,[{...f.report,repeatBuilds:2}],compiler),/whole-map proof/);
  }
});

test('static adapter rejects missing builds, false proofs, summaries, forged bytes and dropped runtime',()=>{
  for(const alter of[
    f=>f.observations[0].repeatBuilds=1,
    f=>f.observations[0].compilerSha256='b'.repeat(64),
    f=>f.report.verified[0].completeModuleRoles=false,
    f=>f.report.probeOnly=true,
    f=>f.report.completeFamily=false,
    f=>f.report.verified=1,
    f=>f.report.verified[0].actualMapSha256=sha256(JSON.stringify(JSON.parse(f.raw))),
    f=>f.report.executions=[],
    f=>f.observations.push({...f.observations[0]}),
  ]){const f=staticFixture(true);alter(f);assert.throws(()=>auditReport(f.input,f.reference,f.report,f.observations,compiler,'static-super'));}
});

test('whole writer id joins require real byte evidence for missing Native hashes',()=>{
  const f=staticFixture(),original={compilerSha256:compiler,repeatBuilds:2,failures:[],verified:f.report.verified.map(({completeModuleRoles,...row})=>({...row,allCharactersClassified:true}))};
  for(const row of original.verified){delete row.index;delete row.nativeJavaScriptSha256;delete row.nativeMapSha256;}
  assert.throws(()=>auditReport(f.input,f.reference,original,null,compiler),/actual byte evidence/);
  const joined=auditReport(f.input,f.reference,original,f.observations,compiler);
  assert.equal(inventory(f.input,f.reference,[joined],compiler).coveredJavascriptMaps,1);
  const full=fixture(),nested={...full.report,verified:full.report.verified.map(row=>({index:row.index,id:row.id,maps:[row],runtime:{executions:2}}))};
  assert.equal(inventory(full.input,full.reference,[auditReport(full.input,full.reference,nested,null,compiler)],compiler).coveredJavascriptMaps,3);
  assert.throws(()=>auditReport(full.input,full.reference,{...full.report,verified:4},null,compiler));
});

test('UTF16 source-specific proof does not require invented flags or observations',()=>{
  const f=fixture(),report={...f.report,family:'ge2_module_helpers/private-accessor',completeFamily:true,
    verified:f.report.verified.map(({allCharactersClassified,...row})=>({...row,completeUTF16ModuleRoles:true,controls:[{rejected:true}],runtime:{executions:2}}))};
  const accepted=auditReport(f.input,f.reference,report,null,compiler,'utf16-module');
  assert.equal(accepted.verified[0].allCharactersClassified,undefined);
  assert.equal(inventory(f.input,f.reference,[accepted],compiler).coveredJavascriptMaps,3);
  assert.throws(()=>auditReport(f.input,f.reference,{...report,completeFamily:false},null,compiler,'utf16-module'));
});

test('metadata evidence needs its exact complete proof, syntax, runtime and actual-byte joins',()=>{
  const f=staticFixture(),map=f.report.verified[0].map,hashes=f.report.verified[0];
  const report={compilerSha256:compiler,repeatBuilds:2,failures:[],scope:'complete Native-derived consumer/provider module roles, artifacts and execution',
    metadata:[hashes],verified:[{index:0,map,module:'main.ts',classes:1,metadataConstructors:1}],
    syntax:['Native','Blue'].map(producer=>({index:0,map,producer,codeSha256:hashes.actualJavaScriptSha256})),runtime:[{index:0,executions:2}]};
  const accepted=auditReport(f.input,f.reference,report,f.observations,compiler,'metadata-module');
  assert.equal(inventory(f.input,f.reference,[accepted],compiler).coveredJavascriptMaps,1);
  for(const alter of[r=>r.verified=[],r=>r.syntax.pop(),r=>r.runtime=[],r=>r.metadata[0].actualMapSha256='c'.repeat(64)]){
    const changed=structuredClone(report);alter(changed);assert.throws(()=>auditReport(f.input,f.reference,changed,f.observations,compiler,'metadata-module'));
  }
});

test('target-only subsets cannot promote a partial semantic contract',()=>{
  const f=fixture(),report={...f.report,completeFamilies:false,scope:'ES2015 and later native generator syntax'};
  assert.throws(()=>inventory(f.input,f.reference,[report],compiler));
  const accepted=auditReport(f.input,f.reference,report,null,compiler);
  assert.equal(accepted.completeFamilies,false);
  assert.equal(inventory(f.input,f.reference,[accepted],compiler).coveredJavascriptMaps,3);
  assert.throws(()=>auditReport(f.input,f.reference,{...report,scope:'only sampled source endpoints'},null,compiler));
});
