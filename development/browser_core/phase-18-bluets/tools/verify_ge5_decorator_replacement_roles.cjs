// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every frozen target/module/provider/coordinate/publication product remains required.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {compare} = require('./ge5_decorator_replacement_module.cjs');
const root = path.resolve(__dirname, '../../../..'), directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), referenceBytes = fs.readFileSync(path.join(directory, 'reference.json'));
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const parentReference=JSON.parse(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge3_decorator_compositions/reference.json')));
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'))();
const group='literal-replacement-order',counts=new Map([['literal-methods',67],['literal-accessors',67],['literal-auto-accessors',51],['literal-replacement-order',67]]);assert(counts.has(group));
const observations = JSON.parse(fs.readFileSync(0, 'utf8'));
const selected = input.cases.flatMap((row, index) => row.form === 'ge3_decorator_compositions/'+group ? [index] : []);
assert.deepEqual(observations.map(row => row.index), selected); assert.equal(selected.length, counts.get(group));
const compilerSha256 = graph.sha256(fs.readFileSync(process.env.BLUEICE_GE5_CLI_PATH||path.join(root, 'target/debug/bluetsc'))), verified = [], failures = [], executions = [];
const probeOnly = process.env.BLUEICE_GE5_ROLE_PROBE_ONLY === '1';
for (const row of observations) {
  const specification = input.cases[row.index], original = graph.resolve(input, specification, parents), expected = reference.cases[row.index];
  assert.equal(row.id, expected.id); assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2); assert.deepEqual(row.options, original.options);
  for (const [name, map] of Object.entries(expected.maps)) {
    if (map.kind !== 'javascript') continue;
    try {
      const code = row.outputs[map.output], raw = name.endsWith('#inline') ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString('utf8') : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name)), actual = stream(ts, code, raw);
      assert.equal(native.decoded.metadata.sources.length, 1, 'one exact original mapped entry');
      const sourceRoot = native.decoded.metadata.sourceRoot || '', sourceName = native.decoded.metadata.sources[0];
      const key = path.posix.normalize(sourceRoot ? sourceName : path.posix.join(path.posix.dirname(map.output), sourceName));
      assert.equal(key, original.entry); assert(Object.hasOwn(original.files, key));
      const proof = compare(ts, native, actual, original.options, original.files[key]);
      const endpoints=require('./ge5_decorator_replacement_endpoints.cjs').verify(ts,native,actual,original.options,original.files[key]);
      const controls = probeOnly ? [] : [...require('./ge5_decorator_replacement_mutations.cjs').run(ts, native, actual, original.options, original.files[key], proof),...endpoints.controls];
      verified.push({index: row.index, id: row.id, map: name, allCharactersClassified: true, logicalEndpoints:endpoints.logicalEndpoints, controls,
        nativeJavaScriptSha256: expected.outputs[map.output], actualJavaScriptSha256: graph.sha256(code),
        nativeMapSha256: graph.sha256(authority.raw(reference, expected, name)), actualMapSha256: graph.sha256(raw)});
    } catch (error) { failures.push({index: row.index, id: row.id, map: name, error: error.message, stack: error.stack}); }
  }
  if(process.env.BLUEICE_GE5_REPLACEMENT_PROGRESS)fs.appendFileSync(process.env.BLUEICE_GE5_REPLACEMENT_PROGRESS,JSON.stringify({index:row.index,verified:verified.length,failures:failures.length})+'\n');
  if (!probeOnly || process.env.BLUEICE_GE5_ROLE_PROBE_EXECUTIONS==='1') try {
    const outputs = Object.fromEntries(Object.entries(expected.outputs).map(([name, hash]) => [name, reference.blobs[hash]]));
    const parent = parents.ge3_decorator_compositions.byId.get(specification.origin.id);
    executions.push({index: row.index, id: row.id, ...require('./ge5_decorator_literal_execution.cjs').run(ts,acorn,parent,original,outputs,row.outputs,parentReference.cases.find(record=>record.id===parent.id))});
  } catch (error) { failures.push({index: row.index, id: row.id, runtime: true, error: error.message, stack: error.stack}); }
}
const noMap=probeOnly?null:require('./verify_ge5_replacement_nomap.cjs').verify(process.env.BLUEICE_GE5_CLI_PATH||path.join(root,'target/debug/bluetsc'),observations);if(noMap?.failures.length)failures.push(...noMap.failures.map(f=>({...f,noMap:true})));
const report = {noMap,family: 'ge3_decorator_compositions/'+group, configurations: observations.length,
  maps: verified.length + failures.filter(row => row.map).length, verified, failures, executions, compilerSha256, repeatBuilds: 2,
  completeFamily: !probeOnly && !failures.length, probeOnly,
  contract: 'every source token/UTF-16 interior/class and decorator phase/complete helper/module ABI/generated comment/ordered physical gap/endpoint/metadata/publication'};
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) { fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true}); fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'decorator-'+group+'.json'), JSON.stringify(report, null, 2) + '\n'); }
console.log(JSON.stringify({family: report.family, configurations: report.configurations, maps: report.maps, logicalEndpoints:verified.reduce((n,r)=>n+r.logicalEndpoints,0),mapNoMapBytePairs:noMap?.mapNoMapBytePairs,completeFamily: report.completeFamily, verified: verified.length,
  rejectedControls: verified.reduce((sum, row) => sum + row.controls.length, 0), executions: executions.reduce((sum, row) => sum + row.executions, 0), failures: failures.length, examples: failures.slice(0, 3)}));
if (failures.length) process.exitCode = 1;
