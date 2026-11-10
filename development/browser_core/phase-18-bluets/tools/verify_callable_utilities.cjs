// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exact declarations, isolated Native consumers, emitted syntax and execution.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
const ts = require(implementation), acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
assert.equal(ts.version, '5.9.3'); assert.equal(acorn.version, '8.15.0');
const sha = value => createHash('sha256').update(value).digest('hex');
const directory = path.join(repo, 'backend/bluets/tests/fixtures/callable_utilities');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), input = JSON.parse(inputBytes);
const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'), 'utf8'));
assert.equal(sha(inputBytes), reference.inputSha256);
assert.equal(sha(fs.readFileSync(implementation)), reference.implementationSha256);
for (const [name, hash] of Object.entries(reference.libraries)) assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), hash);
const compiler = fs.realpathSync(process.env.BLUEICE_CALLABLE_UTILITIES_COMPILER);
const compilerHash = sha(fs.readFileSync(compiler));
if (process.env.BLUEICE_GE5_EXPECTED_COMPILER_SHA256) assert.equal(compilerHash, process.env.BLUEICE_GE5_EXPECTED_COMPILER_SHA256);
const capture = process.env.BLUEICE_CALLABLE_UTILITIES_OBSERVATIONS;
const root = capture ? path.join(path.resolve(capture), 'blue-' + process.pid) : fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-utility-blue-'));
fs.mkdirSync(root, {recursive: true});
const report = {schema: 1, sourceRevision: process.env.BLUEICE_CALLABLE_SOURCE_REVISION || null,
  compilerSha256: compilerHash, inputSha256: reference.inputSha256,
  implementationSha256: reference.implementationSha256, libraries: reference.libraries,
  cases: [], consumers: 0, syntax: 0, runtime: 0, conclusion: 'in_progress'};
const save = () => fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
const libraryCache = new Map();
function diagnostic(d, cwd) {
  let position = null, file = null;
  if (d.file && d.start !== undefined) {
    const loc = d.file.getLineAndCharacterOfPosition(d.start);
    file = path.relative(cwd, d.file.fileName).replaceAll('\\', '/');
    position = {line: loc.line + 1, column: loc.character + 1, length: d.length};
  }
  return {code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'), file, position,
    relatedInformation: (d.relatedInformation || []).map(row => diagnostic(row, cwd))};
}
function consumer(cwd, options, source) {
  fs.writeFileSync(path.join(cwd, 'consumer.ts'), source);
  const host = ts.createCompilerHost(options), get = host.getSourceFile;
  host.getSourceFile = (file, version, ...rest) => {
    const relative = path.relative(path.dirname(implementation), file);
    if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, version, ...rest);
    const key = file + ':' + version;
    if (!libraryCache.has(key)) libraryCache.set(key, get(file, version, ...rest));
    return libraryCache.get(key);
  };
  const program = ts.createProgram([path.join(cwd, 'consumer.ts')], {...options, noEmit: true}, host);
  const files = program.getSourceFiles().map(file => path.relative(cwd, file.fileName).replaceAll('\\', '/')).filter(name => !name.startsWith('../')).sort();
  assert.deepEqual(files, ['consumer.ts', 'main.d.ts'], 'consumer did not select only the emitted declarations');
  const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(d => diagnostic(d, cwd));
  return {accepts: diagnostics.length === 0, diagnostics};
}
try {
  save();
  for (const expected of reference.cases.filter(row => row.accepts)) {
    const entry = input.cases.find(row => row.id === expected.form), profile = input.profiles.find(row => row.id === expected.profile);
    assert(entry && profile); const cwd = path.join(root, profile.id, entry.id); fs.mkdirSync(cwd, {recursive: true});
    fs.writeFileSync(path.join(cwd, 'main.ts'), entry.source);
    const options = {...input.options, ...profile.compilerOptions, outDir: 'dist'};
    fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify({compilerOptions: options, files: ['main.ts']}, null, 2) + '\n');
    const built = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], {cwd, encoding: 'utf8', timeout: 120000});
    fs.writeFileSync(path.join(cwd, 'build.stdout'), built.stdout || ''); fs.writeFileSync(path.join(cwd, 'build.stderr'), built.stderr || '');
    if (built.error) throw built.error; assert.equal(built.status, 0, expected.id + ': ' + built.stderr); assert.equal(built.stderr, '');
    const dist = path.join(cwd, 'dist');
    const declarations = Object.fromEntries(fs.readdirSync(dist).filter(name => name.endsWith('.d.ts')).sort().map(name => [name, fs.readFileSync(path.join(dist, name), 'utf8')]));
    assert.deepEqual(declarations, expected.declarations, expected.id + ': declaration bytes differ');
    const row = {id: expected.id, declarations};
    if (entry.consumer) {
      const consumerRoot = path.join(cwd, 'consumers'); fs.mkdirSync(consumerRoot);
      for (const [name, text] of Object.entries(declarations)) fs.writeFileSync(path.join(consumerRoot, name), text);
      const converted = ts.convertCompilerOptionsFromJson(options, consumerRoot); assert.deepEqual(converted.errors, []);
      row.consumers = {};
      for (const kind of ['positive', 'negative']) {
        const observed = consumer(consumerRoot, converted.options, entry.consumer[kind]);
        assert.deepEqual(observed, expected.consumers[kind], expected.id + ': ' + kind + ' consumer differs');
        row.consumers[kind] = observed; report.consumers++;
      }
    }
    const javascript = fs.readFileSync(path.join(dist, 'main.js'), 'utf8');
    acorn.parse(javascript, {ecmaVersion: profile.compilerOptions.target === 'ES5' ? 5 : 2022,
      sourceType: profile.compilerOptions.module === 'CommonJS' ? 'script' : 'module'});
    row.javascriptSha256 = sha(javascript); report.syntax++;
    if (entry.runtime) {
      const run = spawnSync(process.execPath, ['--input-type=' + (profile.compilerOptions.module === 'CommonJS' ? 'commonjs' : 'module')],
        {input: javascript, encoding: 'utf8', timeout: 120000});
      if (run.error) throw run.error;
      row.runtime = {exit: run.status, stdout: run.stdout, stderr: run.stderr};
      assert.deepEqual(row.runtime, expected.runtime, expected.id + ': runtime bytes differ'); report.runtime++;
    }
    report.cases.push(row); save();
  }
  assert.equal(report.cases.length, 75); assert.equal(report.consumers, 150); assert.equal(report.syntax, 75); assert.equal(report.runtime, 6);
  assert.equal(sha(fs.readFileSync(compiler)), compilerHash);
  assert.equal(sha(fs.readFileSync(path.join(directory, 'cases.json'))), reference.inputSha256);
  assert.equal(sha(fs.readFileSync(implementation)), reference.implementationSha256);
  for (const [name, hash] of Object.entries(reference.libraries)) assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), hash);
  report.conclusion = 'pass'; save();
  console.log('Verified 75 exact declaration inventories, 150 isolated Native consumers, 75 selected-edition parses and six executions');
} catch (error) { report.conclusion = 'failure'; report.error = error.stack; save(); throw error; }
finally { if (!capture) fs.rmSync(root, {recursive: true, force: true}); }
