// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Freeze native build/CLI/state/watch authority before changing production code.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(support, 'load_acorn.cjs'))();
const native = require('./gm2_native.cjs');
const observe = require('./gm2_observations.cjs');
const watch = require('./gm2_watch.cjs');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gm2_build_interop');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json')));
assert.equal(ts.version, '5.9.3');
assert.equal(acorn.version, '8.15.0');

function selected(base) {
  const inputs = new Set();
  function visit(directory) {
    for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(file);
      else if (entry.name.endsWith('.tsbuildinfo')) {
        const info = JSON.parse(fs.readFileSync(file, 'utf8'));
        for (const name of info.fileNames || []) {
          const absolute = path.resolve(directory, name);
          if (absolute.startsWith(base + '/')) inputs.add(path.relative(base, absolute).replaceAll(path.sep, '/'));
        }
      }
    }
  }
  visit(base);
  return [...inputs].sort();
}
function snapshot(base, row, inputs, compilation) {
  const outputs = observe.collect(base, inputs);
  return {...compilation, selectedInputs: compilation.selectedInputs || selected(base), outputs,
    ...observe.artifacts(ts, acorn, row, outputs, inputs)};
}
function publish(group, value) {
  const file = path.join(directory, group + '.reference.json');
  if (process.env.BLUEICE_WRITE_GM2 === '1') fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
  else if (!process.argv.includes('--probe')) assert.deepEqual(value, JSON.parse(fs.readFileSync(file)));
  console.log(JSON.stringify({group, observations: value.cases.length,
    declarations: value.cases.reduce((n, row) => n + Object.keys(row.outputs).filter(file => /\.d\.(?:ts|mts|cts)$/.test(file)).length, 0),
    executions: value.cases.reduce((n, row) => n + Object.keys(row.executions).length, 0)}));
}
function aggregate() {
  const groups=['build','state','cli','watch'];
  const records=Object.fromEntries(groups.map(group=>[group,JSON.parse(fs.readFileSync(path.join(directory,group+'.reference.json')))]));
  const merged={schema:1,typescript:ts.version,cases:groups.flatMap(kind=>records[kind].cases.map(row=>({kind,...row}))),
    seeds:records.state.seeds,ownerControls:input.ownerControls};
  const file=path.join(directory,'reference.json');
  if(process.env.BLUEICE_WRITE_GM2==='1')fs.writeFileSync(file,JSON.stringify(merged,null,2)+'\n');
  else assert.deepEqual(merged,JSON.parse(fs.readFileSync(file)));
  console.log(JSON.stringify({total:merged.cases.length,ownerControls:merged.ownerControls.length}));
}
async function main() {
  const groups = process.argv.includes('--group') ? [process.argv[process.argv.indexOf('--group') + 1]] : ['build', 'state', 'cli', 'watch'];
  for (const group of groups) {
    const cases = [], seeds = {};
    for (const workflow of input[group]) {
      if (group === 'watch') {
        cases.push(...await watch.run(ts, workflow, snapshot,
          row => console.log('WATCH ' + row.id + ' ' + JSON.stringify(row.events))));
        continue;
      }
      const base = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-gm2-native-')));
      const inputs = {...workflow.files};
      observe.write(base, inputs);
      try {
        if (group === 'cli') {
          const compilation = native.cli(ts, base, workflow.args);
          const row = {id: workflow.id, ...snapshot(base, workflow, inputs, compilation)};
          cases.push(row);
          if (process.argv.includes('--probe')) console.log('CLI ' + row.id + ' ' + JSON.stringify({exit: row.exit, diagnostics: row.diagnostics, shown: row.shown}));
          continue;
        }
        if (group === 'state') {
          const seeded = native.build(ts, base, false);
          assert.equal(seeded.exit, 0, workflow.id + ': native seed');
          seeds[workflow.id] = {info: native.normalizeState(ts, base, workflow), outputs: observe.collect(base, inputs)};
        }
        for (const step of workflow.steps) {
          if (Object.keys(step.changes).length) await new Promise(resolve => setTimeout(resolve, 25));
          observe.changes(base, step.changes);
          Object.assign(inputs, step.changes);
          const compilation = native.build(ts, base, workflow.stop);
          const row = {id: step.id, step: step.step, ...snapshot(base, workflow, inputs, compilation)};
          if (group === 'state') {
            assert.equal(row.exit, 0, step.id);
            row.programState = native.programState(ts, base, workflow);
          }
          cases.push(row);
          if (process.argv.includes('--probe')) console.log(group.toUpperCase() + ' ' + row.id + ' ' + JSON.stringify({exit: row.exit, events: row.events, diagnostics: row.diagnostics}));
        }
      } finally { fs.rmSync(base, {recursive: true, force: true}); }
    }
    publish(group, {schema: 1, typescript: ts.version, cases, ...(group === 'state' ? {seeds} : {}), ownerControls: input.ownerControls});
  }
  const lib = path.dirname(ts.getDefaultLibFilePath({}));
  const hashes = observe.sorted(Object.fromEntries(fs.readdirSync(lib).filter(name => /^lib\..+\.d\.ts$/.test(name))
    .map(name => [name, observe.hash(fs.readFileSync(path.join(lib, name)))])));
  const file = path.join(directory, 'library_hashes.json');
  if (process.env.BLUEICE_WRITE_GM2 === '1') fs.writeFileSync(file, JSON.stringify({typescript: ts.version, files: hashes}, null, 2) + '\n');
  else if (!process.argv.includes('--probe')) assert.deepEqual({typescript: ts.version, files: hashes}, JSON.parse(fs.readFileSync(file)));
  if (groups.length === 4) {
    if(!process.argv.includes('--probe'))aggregate();
  }
}
async function validate() {
  const actual = JSON.parse(fs.readFileSync(0, 'utf8'));
  const expected = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'))).cases;
  assert.equal(actual.length, expected.length);
  let executions = 0, consumers = 0;
  for (const [index, row] of actual.entries()) {
    const reference = expected[index];
    assert.equal(row.id, reference.id);
    const workflow = input[reference.kind].find(workflow =>
      workflow.id === row.id || workflow.steps?.some(step => step.id === row.id));
    assert(workflow);
    assert.deepEqual(Object.keys(row.outputs).sort(), Object.keys(reference.outputs).sort(), row.id + ': artifacts');
    const declarations = outputs => Object.fromEntries(Object.entries(outputs).filter(([name]) => /\.d\.(?:ts|mts|cts)$/.test(name)));
    assert.deepEqual(declarations(row.outputs), declarations(reference.outputs), row.id + ': declaration bytes');
    const observed = observe.artifacts(ts, acorn, workflow, row.outputs, row.inputs);
    for (const key of ['imports', 'consumers', 'executions']) assert.deepEqual(observed[key], reference[key], row.id + ': ' + key);
    executions += Object.keys(observed.executions).length;
    if (observed.consumers.accept) consumers += 2;
  }
  console.log(JSON.stringify({observations: actual.length, consumers, executions}));
}
function nativeStep() {
  const request = JSON.parse(fs.readFileSync(0, 'utf8'));
  const workflow = input.state.find(row => row.id === request.workflow);
  assert(workflow);
  const base = fs.realpathSync(request.root);
  assert(base.startsWith(fs.realpathSync(os.tmpdir()) + path.sep), 'native consumer must use a temporary project');
  const consumedState = native.programState(ts, base, workflow);
  const result = native.build(ts, base, false);
  console.log(JSON.stringify({...result, consumedState, programState: native.programState(ts, base, workflow)}));
}
const action = process.argv.includes('--outputs') ? validate : process.argv.includes('--native-step') ? nativeStep : process.argv.includes('--aggregate') ? aggregate : main;
Promise.resolve().then(action).catch(error => {console.error(error); process.exitCode = 1;});
