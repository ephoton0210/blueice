// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const {spawnSync} = require('child_process');
const root = path.resolve(__dirname, '../../../..');
const fixture = path.join(root, 'backend/bluets/tests/fixtures/target_protocols');
const ts = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs')();
if (ts.version !== '5.9.3' || acorn.version !== '8.15.0') throw new Error('Unpinned tools');
function hash(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex'); }
const nativePath = path.join(fixture, 'native.mjs');
const nativeSource = fs.readFileSync(nativePath, 'utf8').replaceAll('\r\n', '\n');
const typedSource = nativeSource
  .replace('\n\nfunction snapshot', '\n\ndeclare const console: {log(value: unknown): void};\n\nfunction snapshot')
  .replace('function snapshot(completion, closing)', 'function snapshot(completion: string, closing: string)')
  .replace('async function asyncSnapshot(completion, closing)', 'async function asyncSnapshot(completion: string, closing: string)')
  .replaceAll('const iterator =', 'const iterator: any =')
  .replace('let iterator;', 'let iterator: any;');
const work = fs.mkdtempSync(path.join(require('os').tmpdir(), 'bluets-target-protocol-'));
const native = spawnSync(process.execPath, [nativePath], {encoding: 'utf8', timeout: 30000});
if (native.status !== 0 || native.error) throw new Error(native.stderr || String(native.error));
const nativeLines = native.stdout.replaceAll('\r\n', '\n').trim().split('\n');
if (nativeLines.length !== 37) throw new Error('Native observation count');
for (const line of nativeLines.slice(0, 15)) {
  const [completion, closing, outcome, gets, calls] = line.split('|');
  const expected = completion === 'throw' ? 'body'
    : closing === 'getter-throw' ? 'get-close'
    : closing === 'call-throw' ? 'call-close'
    : closing === 'primitive' ? 'type'
    : completion === 'return' ? 'returned' : 'break';
  if (outcome !== expected || gets !== '1'
      || calls !== (['getter-throw', 'absent'].includes(closing) ? '0' : '1')) throw new Error(line);
}
for (const line of nativeLines.slice(19)) {
  const [mode, completion, closing, outcome, gets, calls, settled] = line.split('|');
  const expected = completion === 'throw' ? 'body'
    : closing === 'getter-throw' ? 'get-close'
    : closing === 'call-throw' ? 'call-close'
    : closing === 'reject' ? 'await-close'
    : closing === 'primitive' ? 'type'
    : completion === 'return' ? 'returned' : 'break';
  if (mode !== 'async' || outcome !== expected || gets !== '1'
      || calls !== (['getter-throw', 'absent'].includes(closing) ? '0' : '1')
      || settled !== (['reject', 'primitive', 'object'].includes(closing) ? '1' : '0')) throw new Error(line);
}
const generatorExpected = ['start-return|0|42|true||true', 'start-throw|0|body||true', 'reentrant|type|false||true', 'completed|20|42|true|body||true'];
if (JSON.stringify(nativeLines.slice(15, 19)) !== JSON.stringify(generatorExpected)) throw new Error('Generator completion semantics changed');
const targets = ['ES5','ES2015','ES2016','ES2017','ES2018','ES2019','ES2020','ES2021','ES2022','ES2023','ESNext'];
const library = path.dirname(ts.getDefaultLibFilePath({}));
const parsedLibraries = new Map();
function host(options) {
  const h = ts.createCompilerHost(options);
  const read = h.getSourceFile;
  h.getSourceFile = (file, version, error, fresh) => {
    if (path.dirname(file) !== library) return read(file, version, error, fresh);
    const key = file + ':' + JSON.stringify(version);
    if (!parsedLibraries.has(key)) parsedLibraries.set(key, read(file, version, error, fresh));
    return parsedLibraries.get(key);
  };
  return h;
}
const results = [];
for (const target of targets) for (const module of ['commonjs','ES2022']) {
  const directory = path.join(work, target + '-' + module);
  fs.mkdirSync(directory);
  const source = path.join(directory, 'main.ts');
  fs.writeFileSync(source, typedSource);
  fs.writeFileSync(path.join(directory, 'package.json'), JSON.stringify({type: module === 'commonjs' ? 'commonjs' : 'module'}));
  const flags = ['--target', target, '--module', module, '--lib', 'ES2020', '--strict', '--skipLibCheck', '--downlevelIteration', 'true'];
  const parsed = ts.parseCommandLine(flags);
  if (parsed.errors.length) throw new Error('Flag parsing failed');
  const options = {...parsed.options, declaration: true, noEmitOnError: true, outDir: path.join(directory, 'out'), newLine: ts.NewLineKind.LineFeed};
  const program = ts.createProgram([source], options, host(options));
  const errors = ts.getPreEmitDiagnostics(program).filter(d => d.category === ts.DiagnosticCategory.Error);
  if (errors.length) throw new Error(JSON.stringify(errors.map(d => ({code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n')}))));
  const emitted = program.emit();
  if (emitted.emitSkipped || emitted.diagnostics.length) throw new Error('Emit failed');
  const javascript = path.join(options.outDir, 'main.js');
  const js = fs.readFileSync(javascript, 'utf8');
  acorn.parse(js, {ecmaVersion: target === 'ES5' ? 5 : target === 'ESNext' ? 'latest' : Number(target.slice(2)), sourceType: module === 'commonjs' ? 'script' : 'module'});
  const run = spawnSync(process.execPath, [javascript], {encoding: 'utf8', timeout: 30000});
  if (run.status !== 0 || run.error) throw new Error(run.stderr || String(run.error));
  const lines = run.stdout.replaceAll('\r\n', '\n').trim().split('\n');
  if (lines.length !== nativeLines.length) throw new Error('Observation count differs');
  const differences = lines.map((line, i) => line === nativeLines[i] ? null : {index: i, native: nativeLines[i], emitted: line}).filter(Boolean);
  results.push({target, module, flags, accepts: true, javascript_sha256: hash(js), declaration_sha256: hash(fs.readFileSync(path.join(options.outDir, 'main.d.ts'))), stdout: run.stdout.replaceAll('\r\n', '\n'), declaration_text: fs.readFileSync(path.join(options.outDir, 'main.d.ts'), 'utf8'), differences});
}
const record = {typescript: ts.version, acorn: acorn.version, native_source_sha256: hash(nativeSource), typed_source_sha256: hash(typedSource), native_stdout: native.stdout.replaceAll('\r\n', '\n'), native_observations: nativeLines.length, cases: results};
const text = JSON.stringify(record, null, 2) + '\n';
const destination = path.join(fixture, 'reference.json');
if (process.env.BLUEICE_WRITE_TARGET_PROTOCOLS === '1') {
  fs.writeFileSync(destination, text);
  fs.writeFileSync(path.join(fixture, 'input.ts'), typedSource);
} else if (fs.readFileSync(destination, 'utf8').replaceAll('\r\n', '\n') !== text
    || fs.readFileSync(path.join(fixture, 'input.ts'), 'utf8').replaceAll('\r\n', '\n') !== typedSource) {
  throw new Error('Protocol observations changed');
}
fs.rmSync(work, {recursive: true, force: true});
console.log(JSON.stringify({cases: results.length, native_observations: nativeLines.length, upstream_cases_with_differences: results.filter(r => r.differences.length).length, upstream_differences: results.reduce((n, r) => n + r.differences.length, 0), node: process.version}));
