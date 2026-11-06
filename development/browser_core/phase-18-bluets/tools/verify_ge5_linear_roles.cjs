// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native-derived continuation data-flow contracts; immutable input selection.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), { stream } = require('./ge5_role_stream.cjs'), { compare } = require('./ge5_linear_roles.cjs');
const root = path.resolve(__dirname, '../../../..'), directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'))), reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json')));
assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'cases.json'))), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'reference.json'))), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const observations = JSON.parse(fs.readFileSync(0, 'utf8')), parents = graph.parents(root, input);
const logical = process.argv.includes('--logical'), asynchronous = process.argv.includes('--named-async');
const nested = process.argv.includes('--nested-await');
const conditional = process.argv.includes('--conditional-await');
const statement = process.argv.includes('--conditional-statement');
const template = process.argv.includes('--template');
const computed = process.argv.includes('--computed');
const switchProfile = process.argv.includes('--switch');
const finallyProfile = process.argv.includes('--finally-return');
const catchProfile = process.argv.includes('--catch-finally');
const loopProfile = process.argv.includes('--loop-finally');
const delegateProfile = process.argv.includes('--delegate-finally');
const form = delegateProfile ? 'delegate-external-return-finally' : loopProfile ? 'loop-abrupt-finally' : catchProfile ? 'switch-exception-finally' : finallyProfile ? 'finally-overrides-return' : switchProfile ? 'switch-suspension-fallthrough' : computed ? 'computed-assignment-yield' : template ? asynchronous ? 'template-await-order' : 'template-yield-order' : statement ? 'conditional-await' : conditional ? 'conditional-expression-await' : nested ? 'nested-await-call' : asynchronous ? 'helper-name-collisions' : logical ? 'logical-yield-order' : 'binary-yield-order';
const selected = input.cases.flatMap((row, index) => row.form === 'ge1_downlevel/' + form
  && graph.resolve(input, row, parents).options.target === 'ES5' ? [index] : []);
assert.deepEqual(observations.map(row => row.index), selected); assert.equal(selected.length, 7);
const compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc'))), verified = [], failures = [], mutations = [];
(async()=>{
for (const row of observations) {
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  assert.deepEqual(row.files, graph.resolve(input, input.cases[row.index], parents).files);
  const expected = reference.cases[row.index]; assert.equal(row.id, expected.id);
  for (const [name, artifact] of Object.entries(expected.maps)) {
    if (artifact.kind !== 'javascript') continue;
    try {
      const code = row.outputs[artifact.output], raw = name.endsWith('#inline')
        ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString('utf8') : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[artifact.output]], reference.blobs[artifact.raw]), actual = stream(ts, code, raw);
      const proof = compare(ts, native, actual, Object.values(row.files), { asynchronous, computed, switch: switchProfile || catchProfile, delegation: delegateProfile, exceptions: delegateProfile ? 'delegate-finally' : loopProfile ? 'loop-finally' : catchProfile ? 'catch-finally' : finallyProfile, loop: loopProfile });
      const controls = delegateProfile ? require('./ge5_delegate_finally_mutations.cjs').run(ts,native,actual,Object.values(row.files),proof) : loopProfile ? require('./ge5_loop_finally_mutations.cjs').run(ts,native,actual,Object.values(row.files),proof) : catchProfile ? require('./ge5_catch_finally_mutations.cjs').run(ts,native,actual,Object.values(row.files),proof) : finallyProfile ? require('./ge5_finally_mutations.cjs').run(ts,native,actual,Object.values(row.files),proof) : switchProfile ? require('./ge5_switch_mutations.cjs').run(ts,native,actual,Object.values(row.files),proof) : computed ? require('./ge5_computed_mutations.cjs').run(ts,native,actual,Object.values(row.files),proof) : template ? require('./ge5_template_mutations.cjs').run(ts, native, actual, Object.values(row.files), proof, asynchronous)
        : asynchronous ? require('./ge5_named_async_mutations.cjs').run(ts, native, actual, Object.values(row.files), proof, form)
        : require(logical ? './ge5_branch_mutations.cjs' : './ge5_linear_mutations.cjs').run(ts, native, actual, Object.values(row.files), proof);
      const runtime=delegateProfile?await require('./ge5_delegate_finally_execution.cjs').run(ts,native,actual,graph.resolve(input,input.cases[row.index],parents).options):loopProfile?await require('./ge5_loop_finally_execution.cjs').run(ts,native,actual,graph.resolve(input,input.cases[row.index],parents).options,proof):catchProfile?await require('./ge5_catch_finally_execution.cjs').run(ts,native,actual,graph.resolve(input,input.cases[row.index],parents).options,proof):finallyProfile?await require('./ge5_finally_execution.cjs').run(ts,native,actual,graph.resolve(input,input.cases[row.index],parents).options):switchProfile?await require('./ge5_switch_execution.cjs').run(ts,native,actual,graph.resolve(input,input.cases[row.index],parents).options):computed?await require('./ge5_computed_execution.cjs').run(ts,native,actual,graph.resolve(input,input.cases[row.index],parents).options,proof):null;
      mutations.push({ id: row.id, controls });
      verified.push({ index: row.index, id: row.id, map: name, actualJavaScriptSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw),
        nativeJavaScriptSha256: expected.outputs[artifact.output], nativeMapSha256: artifact.raw,
        abstractPaths: proof.actualPaths.length, allCharactersClassified: true, ...(computed||switchProfile||finallyProfile||catchProfile||loopProfile||delegateProfile?{runtime}:{}) });
    } catch (error) { failures.push({ id: row.id, map: name, error: error.message, stack: error.stack }); }
  }
}
const report = { configurations: observations.length, maps: verified.length + failures.length, compilerSha256, repeatBuilds: 2, form, verified, mutations, failures };
if (!failures.length) assert.equal(mutations.reduce((sum, row) => sum + row.controls.length, 0),
  delegateProfile ? 553 : loopProfile ? 364 : catchProfile ? 434 : finallyProfile ? 294 : switchProfile ? 301 : computed ? 238 : template ? asynchronous ? 147 : 140 : statement ? 126 : conditional ? 91 : nested ? 98 : asynchronous ? 84 : logical ? 77 : 98);
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, { recursive: true });
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, delegateProfile ? 'delegate-finally.json' : loopProfile ? 'loop-finally.json' : catchProfile ? 'catch-finally.json' : finallyProfile ? 'finally.json' : switchProfile ? 'switch.json' : computed ? 'computed.json' : template ? asynchronous ? 'template-async.json' : 'template-generator.json' : statement ? 'conditional-statement-async.json' : conditional ? 'conditional-async.json' : nested ? 'nested-async.json' : asynchronous ? 'named-async.json' : logical ? 'branching.json' : 'linear.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify(report, null, 2)); if (failures.length) process.exitCode = 1;


})().catch(error=>{console.error(error);process.exitCode=1;});
