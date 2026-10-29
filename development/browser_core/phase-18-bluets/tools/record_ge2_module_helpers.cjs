// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native-only frozen module/helper ABI authority. Normal runs never record.
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const assert = require('node:assert/strict'), crypto = require('node:crypto');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const directory = path.join(root, 'backend/bluets/tests/fixtures/ge2_module_helpers');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(support, 'load_acorn.cjs'))();
const observe = require('./ge2_observations.cjs');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json')));
assert.equal(ts.version, '5.9.3'); assert.equal(acorn.version, '8.15.0');
for (const [name, hash] of Object.entries(input.helperProvider.sha256)) assert.equal(
  crypto.createHash('sha256').update(input.sharedFiles['node_modules/tslib/' + name]).digest('hex'), hash);
const helperNames = new Set([...input.sharedFiles['node_modules/tslib/tslib.d.ts'].matchAll(/export declare function (__\w+)/g)].map(m => m[1]));
const libraries = new Map(), libRoot = path.dirname(ts.getDefaultLibFilePath({}));
const sorted = value => Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)));
const declarations = outputs => sorted(Object.fromEntries(Object.entries(outputs).filter(([n]) => /\.d\.(?:ts|mts|cts)$/.test(n))));
function compile(base, row, consumer) {
  if (consumer) observe.write(base, { [row.consumerEntry]: row[consumer] });
  const config = { compilerOptions: { ...row.options, ...(consumer ? { noEmit: true, rootDir: '.' } : {}) }, files: [consumer ? row.consumerEntry : row.entry] };
  const file = path.join(base, 'tsconfig.json'); observe.write(base, { 'tsconfig.json': JSON.stringify(config, null, 2) + '\n' });
  const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(file, fs.readFileSync(file, 'utf8')), ts.sys, base, {}, file);
  const host = ts.createCompilerHost(parsed.options), read = host.getSourceFile, outputs = {};
  host.getSourceFile = (file, ...args) => {
    if (path.dirname(file) !== libRoot) return read(file, ...args);
    const key = file + JSON.stringify(args[0]); if (!libraries.has(key)) libraries.set(key, read(file, ...args)); return libraries.get(key);
  };
  host.writeFile = (file, text, bom) => { outputs[path.relative(base, file).replaceAll(path.sep, '/')] = (bom ? '\ufeff' : '') + text; };
  const program = ts.createProgram(parsed.fileNames, parsed.options, host);
  const describe = d => ({ code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n').replaceAll(base, '<root>'),
    module: d.file ? path.relative(base, d.file.fileName).replaceAll(path.sep, '/') : null, start: d.start ?? null, length: d.length ?? null,
    related: (d.relatedInformation || []).map(describe) });
  let diagnostics = ts.sortAndDeduplicateDiagnostics([...parsed.errors, ...ts.getPreEmitDiagnostics(program)]);
  if (!diagnostics.length && !consumer) { const emitted = program.emit(); diagnostics = ts.sortAndDeduplicateDiagnostics([...diagnostics, ...emitted.diagnostics]); }
  const selectedInputs = program.getSourceFiles().map(s => path.relative(base, s.fileName).replaceAll(path.sep, '/')).filter(s => !s.startsWith('../')).sort();
  return { diagnostics: diagnostics.map(describe), selectedInputs, outputs: sorted(outputs) };
}
function syntax(row, outputs) {
  const edition = row.options.target === 'ES5' ? 5 : Number(row.options.target.slice(2));
  for (const [name, source] of Object.entries(outputs).filter(([n]) => /\.(?:js|mjs|cjs)$/.test(n)))
    acorn.parse(source, { ecmaVersion: edition, sourceType: name.endsWith('.mjs') || !name.endsWith('.cjs') && row.runtimeModule === 'ESNext' ? 'module' : 'script' });
}
async function main() {
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge2-native-')));
  const recording = process.env.BLUEICE_WRITE_GE2 === '1';
  if (recording) { assert(!process.env.GE2_PROBE_FORM); assert(!process.argv.includes('--probe')); }
  try {
    const cases = [];
    for (const [index, row] of input.cases.entries()) {
      if (process.env.GE2_PROBE_FORM && row.form !== process.env.GE2_PROBE_FORM) continue;
      const base = path.join(temporary, String(index)); fs.mkdirSync(base); observe.write(base, observe.sourceFiles(input, row));
      const result = compile(base, row), accepts = !result.diagnostics.length;
      if (!process.argv.includes('--probe') && !(recording && row.accepts === null)) assert.equal(accepts, row.accepts, row.id + ': ' + JSON.stringify(result.diagnostics));
      if (recording) row.accepts = accepts;
      const actual = { id: row.id, nativeRuntimeDefect: !!row.nativeRuntimeDefect,
        nativeMissingHelper: row.nativeMissingHelper || null, nativeRuntimeDefectReason: row.nativeRuntimeDefectReason || null,
        ...result, imports: observe.inspect(ts, result.outputs, helperNames), consumers: null, execution: null };
      if (accepts) {
        syntax(row, result.outputs); observe.write(base, result.outputs);
        actual.consumers = { accept: compile(base, row, 'consumerAccept').diagnostics, reject: compile(base, row, 'consumerReject').diagnostics };
        assert.deepEqual(actual.consumers.accept, [], row.id + ': valid consumer'); assert(actual.consumers.reject.length, row.id + ': invalid consumer');
        actual.execution = observe.execute(input, row, result.outputs);
        if (!process.argv.includes('--probe')) {
          if (row.nativeRuntimeDefect) {
            assert.deepEqual(actual.execution.configured, { exit: 1, stdout: '', stderr: 'ReferenceError: ' + row.nativeMissingHelper + ' is not defined' }, row.id);
          } else assert.equal(actual.execution.configured.exit, 0, row.id + ': ' + JSON.stringify(actual.execution));
          if (row.runtimeModule === 'UMD') assert.equal(actual.execution.amdBranch.exit, 0, row.id);
        }
      }
      cases.push(actual);
      if (process.argv.includes('--probe') || (index + 1) % 100 === 0) console.log(row.id + ' ' + JSON.stringify({ index: index + 1, codes: result.diagnostics.map(d => d.code), execution: actual.execution }));
    }
    const reference = { schema: 1, typescript: ts.version, tslib: input.helperProvider.version, cases, ownerControls: input.ownerControls };
    if (recording) {
      assert.equal(cases.length, input.cases.length);
      fs.writeFileSync(path.join(directory, 'cases.json'), JSON.stringify(input, null, 2) + '\n');
      fs.writeFileSync(path.join(directory, 'reference.json'), JSON.stringify(reference, null, 2) + '\n');
    } else if (!process.argv.includes('--probe')) assert.deepEqual(reference, JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'))));
    console.log(JSON.stringify({ observations: cases.length, accepted: cases.filter(c => !c.diagnostics.length).length,
      declarationFiles: cases.reduce((n, c) => n + Object.keys(declarations(c.outputs)).length, 0), executions: cases.filter(c => c.execution?.configured.exit === 0).length }));
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
}
if (process.argv.includes('--outputs')) {
  (async () => {
    const actual = JSON.parse(fs.readFileSync(0, 'utf8')), reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'))).cases.filter(c => !c.diagnostics.length && !c.nativeRuntimeDefect);
    assert.equal(actual.length, reference.length);
    const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge2-consumers-')));
    const failures = [];
    try {
      for (let i = 0; i < actual.length; i++) {
        try {
        const a = actual[i], r = reference[i], row = input.cases.find(row => row.id === r.id); assert.equal(a.id, r.id);
        assert.deepEqual(Object.keys(a.outputs).sort(), Object.keys(r.outputs).sort(), r.id + ': artifacts');
        assert.deepEqual(declarations(a.outputs), declarations(r.outputs), r.id + ': declaration bytes');
        assert.deepEqual(observe.inspect(ts, a.outputs, helperNames), r.imports, r.id + ': module dependencies and imported ABI'); syntax(row, a.outputs);
        const base = path.join(temporary, String(i)); fs.mkdirSync(base); observe.write(base, observe.sourceFiles(input, row)); observe.write(base, a.outputs);
        assert.deepEqual({ accept: compile(base, row, 'consumerAccept').diagnostics, reject: compile(base, row, 'consumerReject').diagnostics }, r.consumers, r.id + ': consumers');
        assert.deepEqual(observe.execute(input, row, a.outputs), r.execution, r.id + ': real loaders, live cycles and helper globals');
        } catch (error) {
          failures.push(actual[i].id + ': ' + error.message);
          console.error(failures[failures.length - 1]);
        }
      }
      assert.deepEqual(failures, [], 'every frozen configuration must pass');
      console.log(JSON.stringify({ observations: actual.length, consumers: actual.length * 2, executions: actual.length }));
    } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
  })().catch(error => { console.error(error); process.exitCode = 1; });
} else main().catch(error => { console.error(error); process.exitCode = 1; });
