// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The entire frozen family is compared; proposal execution uses the existing G-E3 adapter.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), {compare} = require('./ge5_decorator_preserved.cjs');
const root = path.resolve(__dirname, '../../../..'), directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), referenceBytes = fs.readFileSync(path.join(directory, 'reference.json'));
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const parentReference = JSON.parse(fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/ge3_decorator_compositions/reference.json')));
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const family = process.argv[2];
assert(['private-super-method', 'private-super-accessors', 'private-static-super', 'private-super-async-generator'].includes(family));
const observations = JSON.parse(fs.readFileSync(0, 'utf8'));
const selected = input.cases.flatMap((row, index) => row.form === 'ge3_decorator_compositions/' + family ? [index] : []);
assert.deepEqual(observations.map(row => row.index), selected); assert.equal(selected.length, 7);
const compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc'))), verified = [], failures = [], executions = [];
for (const row of observations) {
  const specification = input.cases[row.index], original = graph.resolve(input, specification, parents), expected = reference.cases[row.index];
  const parent = parents.ge3_decorator_compositions.byId.get(specification.origin.id);
  assert(parent.preservedSyntax && !parent.nativeRuntimeDefect);
  assert.equal(row.id, expected.id); assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  assert.deepEqual(row.options, original.options);
  for (const [name, map] of Object.entries(expected.maps)) {
    if (map.kind !== 'javascript') continue;
    try {
      const code = row.outputs[map.output], raw = name.endsWith('#inline')
        ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString('utf8')
        : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name)), actual = stream(ts, code, raw);
      const proof = compare(ts, native, actual);
      const controls = require('./ge5_decorator_preserved_mutations.cjs').run(ts, native, actual, original.files['main.ts'], proof);
      verified.push({index: row.index, id: row.id, map: name, tokenPairs: proof.tokenPairs, allCharactersClassified: proof.allCharactersClassified,
        controls, nativeJavaScriptSha256: expected.outputs[map.output], actualJavaScriptSha256: graph.sha256(code),
        nativeMapSha256: graph.sha256(authority.raw(reference, expected, name)), actualMapSha256: graph.sha256(raw)});
    } catch (error) { failures.push({id: row.id, map: name, error: error.message, stack: error.stack}); }
  }
  try {
    const nativeOutputs = Object.fromEntries(Object.entries(expected.outputs).map(([name, hash]) => [name, reference.blobs[hash]]));
    function adapted(outputs) {
      return Object.fromEntries(Object.entries(outputs).map(([name, text]) => [name, /\.(?:js|mjs|cjs)$/.test(name)
        ? ts.transpileModule(text, {fileName: name, compilerOptions: {target: ts.ScriptTarget.ES2022,
          module: parent.runtimeModule === 'CommonJS' ? ts.ModuleKind.CommonJS : ts.ModuleKind.ESNext,
          experimentalDecorators: false, newLine: ts.NewLineKind.LineFeed}}).outputText : text]));
    }
    const record = parentReference.cases.find(record => record.id === parent.id).proposalExecution;
    const runtime = {...parent, options: original.options, files: original.files};
    const wanted = require('./ge1_observations.cjs').execute(runtime, adapted(nativeOutputs));
    const actual = require('./ge1_observations.cjs').execute(runtime, adapted(row.outputs));
    assert.deepEqual(actual, wanted, 'actual emitted-JavaScript-only proposal adapter execution');
    const {adapter, ...recorded} = record; assert.deepEqual(wanted, recorded, 'unchanged exact G-E3 Native adapter authority');
    executions.push({id: row.id, adapter, native: wanted, blue: actual, executions: 2, directNodePreservedSyntaxClaim: false});
  } catch (error) { failures.push({id: row.id, runtime: true, error: error.message, stack: error.stack}); }
}
const report = {family: 'ge3_decorator_compositions/' + family, configurations: observations.length, maps: verified.length,
  compilerSha256, repeatBuilds: 2, completeFamily: !failures.length,
  contract: 'every retained module token/UTF-16 interior/source comment/generated comment/ordered gap/endpoint/metadata/publication',
  verified, executions, failures};
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'decorator-' + family + '.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify({family: report.family, configurations: report.configurations, maps: report.maps, completeFamily: report.completeFamily,
  rejectedControls: verified.reduce((sum, row) => sum + row.controls.length, 0), adapterExecutions: executions.reduce((sum, row) => sum + row.executions, 0),
  failures: failures.length, examples: failures.slice(0, 3)}));
if (failures.length) process.exitCode = 1;
