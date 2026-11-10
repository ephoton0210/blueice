// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independent pinned Native observations, never derived from BlueTS output.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
const ts = require(implementation); assert.equal(ts.version, '5.9.3');
const sha = value => createHash('sha256').update(value).digest('hex');
const directory = path.join(repo, 'backend/bluets/tests/fixtures/callable_utilities');
const bytes = fs.readFileSync(path.join(directory, 'cases.json')), input = JSON.parse(bytes);
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-utility-native-'));
const cache = new Map(), libraries = {};
const capture = process.env.BLUEICE_CALLABLE_UTILITIES_OBSERVATIONS ? path.join(path.resolve(process.env.BLUEICE_CALLABLE_UTILITIES_OBSERVATIONS), 'native-' + process.pid) : null;
function diagnostic(d, root) {
  let position = null, file = null;
  if (d.file && d.start !== undefined) {
    const loc = d.file.getLineAndCharacterOfPosition(d.start);
    file = path.relative(root, d.file.fileName).replaceAll('\\', '/');
    position = {line: loc.line + 1, column: loc.character + 1, length: d.length};
  }
  return {code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'), file, position,
    relatedInformation: (d.relatedInformation || []).map(row => diagnostic(row, root))};
}
function compile(root, files, options, emit) {
  const host = ts.createCompilerHost(options), get = host.getSourceFile;
  host.getSourceFile = (file, languageVersion, ...rest) => {
    const relative = path.relative(path.dirname(implementation), file);
    if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, languageVersion, ...rest);
    const key = file + ':' + languageVersion;
    if (!cache.has(key)) cache.set(key, get(file, languageVersion, ...rest));
    if (fs.existsSync(file)) libraries[path.basename(file)] = sha(fs.readFileSync(file));
    return cache.get(key);
  };
  const output = {};
  host.writeFile = (file, text) => { output[path.relative(root, file).replaceAll('\\', '/')] = text; };
  const program = ts.createProgram(files.map(file => path.join(root, file)), options, host);
  const errors = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program));
  if (emit && errors.length === 0) program.emit();
  return {accepts: errors.length === 0, diagnostics: errors.map(d => diagnostic(d, root)), output,
    selectedInputs: program.getSourceFiles().map(file => path.relative(root, file.fileName).replaceAll('\\', '/')).filter(name => !name.startsWith('../')).sort()};
}
try {
  const cases = [];
  for (const profile of input.profiles) for (const entry of input.cases) {
    const id = profile.id + '/' + entry.id, root = path.join(temporary, profile.id, entry.id);
    fs.mkdirSync(root, {recursive: true}); fs.writeFileSync(path.join(root, 'main.ts'), entry.source);
    const converted = ts.convertCompilerOptionsFromJson({...input.options, ...profile.compilerOptions}, root);
    assert.deepEqual(converted.errors, []);
    const native = compile(root, ['main.ts'], converted.options, true);
    assert.equal(native.accepts, entry.accepts, id + ': invalid authored source');
    const row = {id, form: entry.id, profile: profile.id, accepts: native.accepts, diagnostics: native.diagnostics,
      declarations: Object.fromEntries(Object.entries(native.output).filter(([name]) => name.endsWith('.d.ts')))};
    if (entry.consumer) {
      assert(native.accepts);
      const consumerRoot = path.join(root, 'consumer'); fs.mkdirSync(consumerRoot);
      for (const [name, text] of Object.entries(row.declarations)) fs.writeFileSync(path.join(consumerRoot, name), text);
      row.consumers = {};
      for (const kind of ['positive', 'negative']) {
        fs.writeFileSync(path.join(consumerRoot, 'consumer.ts'), entry.consumer[kind]);
        const consumer = compile(consumerRoot, ['consumer.ts'], {...converted.options, noEmit: true}, false);
        assert.deepEqual(consumer.selectedInputs, ['consumer.ts', 'main.d.ts'], id + ': consumer selected source instead of declarations');
        assert.equal(consumer.accepts, kind === 'positive', id + ': invalid ' + kind + ' consumer');
        row.consumers[kind] = {accepts: consumer.accepts, diagnostics: consumer.diagnostics};
      }
    }
    if (entry.runtime) {
      const javascript = native.output['main.js']; assert.equal(typeof javascript, 'string');
      const run = spawnSync(process.execPath, ['--input-type=' + (profile.compilerOptions.module === 'CommonJS' ? 'commonjs' : 'module')],
        {input: javascript, encoding: 'utf8', timeout: 120000});
      if (run.error) throw run.error; assert.equal(run.status, 0, run.stderr);
      row.runtime = {exit: run.status, stdout: run.stdout, stderr: run.stderr};
    }
    if (capture) {
      const captured = path.join(capture, profile.id, entry.id); fs.mkdirSync(captured, {recursive: true});
      fs.writeFileSync(path.join(captured, 'main.ts'), entry.source);
      fs.writeFileSync(path.join(captured, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
      for (const [name, text] of Object.entries(native.output)) fs.writeFileSync(path.join(captured, name), text);
    }
    cases.push(row);
  }
  assert.equal(cases.length, 156);
  const value = {schema: 1, typescript: ts.version, implementationSha256: sha(fs.readFileSync(implementation)),
    libraries, inputSha256: sha(bytes), cases};
  const text = JSON.stringify(value, null, 2) + '\n', output = path.join(directory, 'reference.json');
  if (process.env.BLUEICE_WRITE_CALLABLE_UTILITIES === '1') fs.writeFileSync(output, text);
  else assert.equal(fs.readFileSync(output, 'utf8'), text, 'Native utility observations changed');
  console.log(`Verified ${cases.length} Native utility observations, complete diagnostics, declarations, consumers and six executions`);
} finally { fs.rmSync(temporary, {recursive: true, force: true}); }
