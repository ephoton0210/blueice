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
const directory = path.join(repo, 'backend/bluets/tests/fixtures/nonnullable_awaited');
const matrices = [['cases.json', 'reference.json'], ['operator-cases.json', 'operator-reference.json']].map(([name, expected]) => {
  const inputBytes = fs.readFileSync(path.join(directory, name));
  const input = JSON.parse(inputBytes), reference = JSON.parse(fs.readFileSync(path.join(directory, expected), 'utf8'));
  assert.equal(sha(inputBytes), reference.inputSha256);
  assert.equal(sha(fs.readFileSync(implementation)), reference.implementationSha256);
  for (const [name, hash] of Object.entries(reference.libraries)) assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), hash);
  return {name, inputBytes, input, reference};
});
const reference = matrices[0].reference;
const accepted = matrices.flatMap(({input, reference}) => reference.cases.filter(row => row.accepts).map(expected => ({input, expected})));
assert.equal(new Set(accepted.map(row => row.expected.id)).size, 315);
const compiler = fs.realpathSync(process.env.BLUEICE_NONNULLABLE_AWAITED_COMPILER);
const compilerHash = sha(fs.readFileSync(compiler));
if (process.env.BLUEICE_NONNULLABLE_AWAITED_EXPECTED_COMPILER_SHA256) assert.equal(compilerHash, process.env.BLUEICE_NONNULLABLE_AWAITED_EXPECTED_COMPILER_SHA256);
const capture = process.env.BLUEICE_NONNULLABLE_AWAITED_OBSERVATIONS;
const root = capture ? path.join(path.resolve(capture), 'blue-' + process.pid) : fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-nonnullable-awaited-blue-'));
fs.mkdirSync(root, {recursive: true});
const report = {schema: 1, sourceRevision: process.env.BLUEICE_NONNULLABLE_AWAITED_SOURCE_REVISION || null,
  compilerSha256: compilerHash, inputs: matrices.map(({name, reference}) => ({name, sha256: reference.inputSha256})),
  implementationSha256: reference.implementationSha256, libraries: reference.libraries,
  cases: [], declarationFiles: 0, consumers: 0, syntax: 0, runtime: 0, conclusion: 'in_progress'};
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
function consumer(cwd, options, source, declarations) {
  fs.writeFileSync(path.join(cwd, 'consumer.ts'), source);
  const host = ts.createCompilerHost(options), get = host.getSourceFile;
  host.getSourceFile = (file, version, ...rest) => {
    const relative = path.relative(path.dirname(implementation), file);
    if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, version, ...rest);
    const key = file + ':' + version;
    if (!libraryCache.has(key)) libraryCache.set(key, get(file, version, ...rest));
    return libraryCache.get(key);
  };
  const roots = ['consumer.ts', ...Object.keys(declarations)].sort();
  const program = ts.createProgram(roots.map(name => path.join(cwd, name)), {...options, noEmit: true}, host);
  const files = program.getSourceFiles().map(file => path.relative(cwd, file.fileName).replaceAll('\\', '/')).filter(name => !name.startsWith('../')).sort();
  assert.deepEqual(files, roots, 'consumer did not select only the emitted declarations');
  const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(d => diagnostic(d, cwd));
  return {accepts: diagnostics.length === 0, diagnostics};
}
function inventory(root, relative = '') {
  const result = {};
  for (const entry of fs.readdirSync(path.join(root, relative), {withFileTypes: true}).sort((a, b) => a.name.localeCompare(b.name))) {
    const name = relative ? relative + '/' + entry.name : entry.name;
    if (entry.isDirectory()) Object.assign(result, inventory(root, name));
    else if (entry.isFile()) result[name] = fs.readFileSync(path.join(root, name), 'utf8');
    else throw Error('unexpected non-regular output: ' + name);
  }
  return result;
}
try {
  save();
  for (const {input, expected} of accepted) {
    const entry = input.cases.find(row => row.id === expected.form), profile = input.profiles.find(row => row.id === expected.profile);
    assert(entry && profile); const cwd = path.join(root, profile.id, entry.id); fs.mkdirSync(cwd, {recursive: true});
    fs.writeFileSync(path.join(cwd, 'main.ts'), entry.source);
    for (const [name, text] of Object.entries(entry.files || {})) {
      assert(!path.isAbsolute(name) && !name.split(/[\\/]/).includes('..'));
      fs.mkdirSync(path.dirname(path.join(cwd, name)), {recursive: true}); fs.writeFileSync(path.join(cwd, name), text);
    }
    const options = {...input.options, ...profile.compilerOptions, outDir: 'dist'};
    fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify({compilerOptions: options, files: ['main.ts', ...Object.keys(entry.files || {})]}, null, 2) + '\n');
    const built = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], {cwd, encoding: 'utf8', timeout: 120000});
    fs.writeFileSync(path.join(cwd, 'build.stdout'), built.stdout || ''); fs.writeFileSync(path.join(cwd, 'build.stderr'), built.stderr || '');
    if (built.error) throw built.error; assert.equal(built.status, 0, expected.id + ': ' + built.stderr); assert.equal(built.stderr, '');
    const dist = path.join(cwd, 'dist');
    const files = inventory(dist);
    const declarations = Object.fromEntries(Object.entries(files).filter(([name]) => /\.d\.(?:ts|mts|cts)$/.test(name)));
    assert.deepEqual(declarations, expected.declarations, expected.id + ': declaration bytes differ');
    const row = {id: expected.id, declarations}; report.declarationFiles += Object.keys(declarations).length;
    if (entry.consumer) {
      const consumerRoot = path.join(cwd, 'consumers'); fs.mkdirSync(consumerRoot);
      for (const [name, text] of Object.entries(declarations)) {
        const file = path.join(consumerRoot, name); fs.mkdirSync(path.dirname(file), {recursive: true}); fs.writeFileSync(file, text);
      }
      const converted = ts.convertCompilerOptionsFromJson(options, consumerRoot); assert.deepEqual(converted.errors, []);
      row.consumers = {};
      for (const kind of ['positive', 'negative']) {
        const observed = consumer(consumerRoot, converted.options, entry.consumer[kind], declarations);
        assert.deepEqual(observed, expected.consumers[kind], expected.id + ': ' + kind + ' consumer differs');
        row.consumers[kind] = observed; report.consumers++;
      }
    }
    row.javascriptSha256 = {};
    for (const [name, javascript] of Object.entries(files).filter(([name]) => /\.(?:js|mjs|cjs)$/.test(name))) {
      acorn.parse(javascript, {ecmaVersion: profile.compilerOptions.target === 'ES5' ? 5 : 2022,
        sourceType: profile.compilerOptions.module === 'CommonJS' ? 'script' : 'module'});
      row.javascriptSha256[name] = sha(javascript); report.syntax++;
    }
    const javascript = fs.readFileSync(path.join(dist, 'main.js'), 'utf8');
    if (entry.runtime) {
      const run = spawnSync(process.execPath, ['--input-type=' + (profile.compilerOptions.module === 'CommonJS' ? 'commonjs' : 'module')],
        {input: javascript, encoding: 'utf8', timeout: 120000});
      if (run.error) throw run.error;
      row.runtime = {exit: run.status, stdout: run.stdout, stderr: run.stderr};
      assert.deepEqual(row.runtime, expected.runtime, expected.id + ': runtime bytes differ'); report.runtime++;
    }
    report.cases.push(row); save();
  }
  assert.equal(report.cases.length, 315); assert.equal(report.declarationFiles, 331); assert.equal(report.consumers, 630); assert.equal(report.syntax, 331); assert.equal(report.runtime, 12);
  assert.equal(sha(fs.readFileSync(compiler)), compilerHash);
  for (const {name, reference} of matrices) assert.equal(sha(fs.readFileSync(path.join(directory, name))), reference.inputSha256);
  assert.equal(sha(fs.readFileSync(implementation)), reference.implementationSha256);
  for (const [name, hash] of Object.entries(reference.libraries)) assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), hash);
  report.conclusion = 'pass'; save();
  console.log('Verified 315 exact declaration inventories / 331 files, 630 isolated Native consumers, 331 selected-edition parses and twelve executions');
} catch (error) { report.conclusion = 'failure'; report.error = error.stack; save(); throw error; }
finally { if (!capture) fs.rmSync(root, {recursive: true, force: true}); }
