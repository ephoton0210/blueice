// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independent workers retain every frozen product and run the whole contract.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const {Worker, isMainThread, parentPort, workerData} = require('node:worker_threads');
// Source selection identity is checked before choosing any output path.
function reportNames(indices, selected) {
  assert.equal(selected.length, 363);
  assert.deepEqual(indices, indices.length === 3 ? [4892, 4933, 5013] : selected);
  const full = indices.length === selected.length;
  return {
    report: full ? 'interop-complete-module-acceptance.json' : 'interop-complete-module-acceptance-partial.json',
    progress: full ? 'interop-module-acceptance-progress.jsonl' : 'interop-module-acceptance-partial-progress.jsonl',
  };
}
module.exports = {reportNames};

if (require.main === module || !isMainThread) {
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs'), {stream} = require('./ge5_role_stream.cjs');
const modules = require('./ge5_interop_module.cjs'), mutations = require('./ge5_interop_mutations.cjs'), execution = require('./ge5_interop_execution.cjs');
const root = path.resolve(__dirname, '../../../..'), directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const bytes = fs.readFileSync(path.join(directory, 'cases.json')), referenceBytes = fs.readFileSync(path.join(directory, 'reference.json'));
assert.equal(graph.sha256(bytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(bytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'))();

async function worker() {
  const products = [], failures = [], executions = []; let syntaxPrograms = 0;
  for (const row of workerData.rows) {
    const original = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
    assert.equal(row.compilerSha256, workerData.compilerSha256); assert.equal(row.repeatBuilds, 2);
    assert.equal(row.id, expected.id); assert.deepEqual(row.options, original.options);
    for (const [name, map] of Object.entries(expected.maps)) {
      if (map.kind !== 'javascript') continue;
      try {
        const filename = path.posix.relative(original.options.outDir, map.output).replace(/\.js$/, '.ts');
        const code = row.outputs[map.output], raw = name.endsWith('#inline')
          ? Buffer.from(/sourceMappingURL=data:[^,]+,([^\s]+)/.exec(code)[1], 'base64').toString('utf8') : row.outputs[name];
        const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name)), actual = stream(ts, code, raw);
        const proof = modules.compare(ts, native, actual, original.options, original.files, original.entry, filename);
        for (const view of [native, actual]) {
          const esm = view.source.statements.some(node => ts.isImportDeclaration(node) || ts.isExportDeclaration(node)
            || node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword));
          acorn.parse(view.code, {ecmaVersion: original.options.target === 'ESNext' ? 'latest'
            : original.options.target === 'ES5' ? 5 : Number(original.options.target.slice(2)), sourceType: esm ? 'module' : 'script'});
          syntaxPrograms++;
        }
        const controls = mutations.run(ts, native, actual, original.options, original.files, original.entry, filename, proof);
        products.push({index: row.index, map: name, actualCodeSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw), controls});
        parentPort.postMessage({kind: 'product', index: row.index, map: name, controls: controls.length});
      } catch (error) {
        failures.push({index: row.index, map: name, error: error.message, stack: error.stack});
        parentPort.postMessage({kind: 'failure', index: row.index, map: name, error: error.message});
      }
    }
    try {
      const native = Object.fromEntries(Object.entries(expected.outputs).filter(([name]) => name.endsWith('.js'))
        .map(([name, hash]) => [name, reference.blobs[hash]]));
      const result = await execution.run(ts, native, row.outputs, original.files, original.options);
      executions.push({index: row.index, ...result});
    } catch (error) { failures.push({index: row.index, phase: 'live-cycle execution', error: error.message, stack: error.stack}); }
  }
  parentPort.postMessage({kind: 'result', products, failures, executions, syntaxPrograms});
}

async function main() {
  const rows = JSON.parse(fs.readFileSync(0, 'utf8'));
  const selected = input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/interop-exportstar-cycle' ? [index] : []);
  const names = reportNames(rows.map(row => row.index), selected);
  const compilerSha256 = rows[0].compilerSha256, reports = process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY;
  if (reports) fs.mkdirSync(reports, {recursive: true});
  const progress = reports && path.join(reports, names.progress); if (progress) fs.writeFileSync(progress, '');
  const results = await Promise.all(Array.from({length: Math.min(4, rows.length)}, (_, worker) => new Promise((resolve, reject) => {
    const thread = new Worker(__filename, {workerData: {rows: rows.filter((_, index) => index % Math.min(4, rows.length) === worker), compilerSha256}});
    let result;
    thread.on('message', message => { if (message.kind === 'result') result = message; else if (progress) fs.appendFileSync(progress, JSON.stringify(message) + '\n'); });
    thread.on('error', reject); thread.on('exit', code => code ? reject(new Error('interop worker exit ' + code)) : resolve(result));
  })));
  const products = results.flatMap(result => result.products), failures = results.flatMap(result => result.failures), executions = results.flatMap(result => result.executions);
  const report = {form: 'ge2_module_helpers/interop-exportstar-cycle', compilerSha256, configurations: rows.length,
    maps: rows.length * 4, completeFamily: rows.length === 363 && failures.length === 0 && products.length === 1452 && executions.length === 363,
    effectiveControls: products.reduce((sum, row) => sum + row.controls.length, 0),
    selectedEditionSyntaxPrograms: results.reduce((sum, row) => sum + row.syntaxPrograms, 0),
    totalExecutions: executions.reduce((sum, row) => sum + row.executions, 0), products, executions, failures};
  if (reports) fs.writeFileSync(path.join(reports, names.report), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({...report, products: products.length, executions: executions.length, failures: failures.slice(0, 4), failureCount: failures.length}));
  assert.equal(failures.length, 0); assert.equal(products.length, rows.length * 4); assert.equal(executions.length, rows.length);
  assert.equal(report.selectedEditionSyntaxPrograms, rows.length * 8);
}

(isMainThread ? main() : worker()).catch(error => { console.error(error.stack); process.exitCode = 1; });
}
