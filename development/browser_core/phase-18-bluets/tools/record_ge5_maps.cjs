// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native-only authority. Recording is explicit; ordinary invocations only check.
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), maps = require('./ge5_maps.cjs');
const root = path.resolve(__dirname, '../../../..');
const directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), input = JSON.parse(inputBytes);
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
assert.equal(ts.version, '5.9.3'); assert.equal(input.repeatBuilds, 2);
const origins = graph.parents(root, input), libraries = new Map();
const libRoot = path.dirname(ts.getDefaultLibFilePath({}));
const sorted = object => Object.fromEntries(Object.entries(object).sort(([a], [b]) => a.localeCompare(b)));

function compile(base, row) {
  const config = { compilerOptions: row.options, files: [row.entry] };
  const filename = path.join(base, 'tsconfig.json'), text = JSON.stringify(config, null, 2) + '\n';
  fs.writeFileSync(filename, text);
  const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(filename, text), ts.sys, base, {}, filename);
  const host = ts.createCompilerHost(parsed.options), read = host.getSourceFile, outputs = {};
  host.getSourceFile = (filename, ...args) => {
    if (path.dirname(filename) !== libRoot) return read(filename, ...args);
    const key = filename + JSON.stringify(args[0]);
    if (!libraries.has(key)) libraries.set(key, read(filename, ...args)); return libraries.get(key);
  };
  host.writeFile = (filename, text, bom) => {
    const relative = path.relative(base, filename).replaceAll(path.sep, '/');
    assert(!relative.startsWith('../') && !path.isAbsolute(relative), 'Native output escaped input root');
    outputs[relative] = (bom ? '\ufeff' : '') + text;
  };
  const program = ts.createProgram(parsed.fileNames, parsed.options, host);
  const describe = diagnostic => ({ code: diagnostic.code,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n').replaceAll(base, '<root>'),
    module: diagnostic.file ? path.relative(base, diagnostic.file.fileName).replaceAll(path.sep, '/') : null,
    start: diagnostic.start ?? null, length: diagnostic.length ?? null,
    related: (diagnostic.relatedInformation || []).map(describe) });
  let diagnostics = ts.sortAndDeduplicateDiagnostics([...parsed.errors, ...ts.getPreEmitDiagnostics(program)]);
  if (!diagnostics.length) diagnostics = ts.sortAndDeduplicateDiagnostics([...diagnostics, ...program.emit().diagnostics]);
  const selectedInputs = program.getSourceFiles().map(source => path.relative(base, source.fileName).replaceAll(path.sep, '/'))
    .filter(name => !name.startsWith('../')).sort();
  return { configuration: text, diagnostics: diagnostics.map(describe), selectedInputs, outputs: sorted(outputs) };
}

function main() {
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge5-native-')));
  const blobs = {}, cases = []; let accepted = 0, mapCount = 0, declarationCount = 0, coordinateIssueMaps = 0;
  const blob = text => { const hash = graph.sha256(text); if (!Object.hasOwn(blobs, hash)) blobs[hash] = text; return hash; };
  try {
    for (const [index, specification] of input.cases.entries()) {
      const base = path.join(temporary, String(index)); fs.mkdirSync(base);
      const row = graph.resolve(input, specification, origins); graph.write(base, row.files);
      const result = compile(base, row);
      assert.deepEqual(compile(base, row), result, specification.id + ': repeated Native build');
      if (specification.profile === 'external') assert.deepEqual(result.diagnostics, [], specification.id);
      let mapped;
      try { mapped = maps.artifacts(result.outputs, { recordInvalidCoordinates: true }); }
      catch (error) {
        const diagnostic = path.join(os.tmpdir(), 'bluets-ge5-native-map-failure.json');
        fs.writeFileSync(diagnostic, JSON.stringify({ id: specification.id, result }, null, 2) + '\n');
        throw new Error(specification.id + ': ' + error.message + '; capture: ' + diagnostic, { cause: error });
      }
      if (!result.diagnostics.length) { accepted++; assert(Object.keys(mapped).length > 0, specification.id); }
      else assert.deepEqual(result.outputs, {}, specification.id + ': failure must not publish');
      const observations = Object.fromEntries(Object.entries(mapped).map(([name, map]) => {
        mapCount++; if (map.kind === 'declaration') declarationCount++;
        if (map.decoded.coordinateIssues.length) coordinateIssueMaps++;
        return [name, { output: map.output, kind: map.kind, raw: blob(map.text),
          decodedSha256: graph.sha256(JSON.stringify(map.decoded)), coordinateIssues: map.decoded.coordinateIssues }];
      }));
      cases.push({ id: specification.id,
        ...(result.diagnostics.length ? { configuration: blob(result.configuration) } : {}),
        diagnostics: result.diagnostics, selectedInputs: result.selectedInputs,
        outputs: Object.fromEntries(Object.entries(result.outputs).map(([name, text]) => [name, blob(text)])), maps: observations });
      fs.rmSync(base, { recursive: true, force: true });
      if ((index + 1) % 250 === 0) console.log(JSON.stringify({ completed: index + 1, total: input.cases.length, accepted, maps: mapCount }));
    }
    const reference = { schema: 1, typescript: ts.version, inputSha256: graph.sha256(inputBytes), repeatBuilds: 2,
      counts: { configurations: cases.length, accepted, rejected: cases.length - accepted,
        maps: mapCount, declarationMaps: declarationCount, javascriptMaps: mapCount - declarationCount, coordinateIssueMaps,
        uniqueBlobs: Object.keys(blobs).length }, cases, blobs: sorted(blobs), ownerControls: input.ownerControls };
    const target = path.join(directory, 'reference.json');
    if (process.env.BLUEICE_WRITE_GE5 === '1') fs.writeFileSync(target, JSON.stringify(reference, null, 2) + '\n');
    else assert.deepEqual(reference, JSON.parse(fs.readFileSync(target)), 'complete Native G-E5 authority changed');
    console.log(JSON.stringify({ ...reference.counts, inputSha256: reference.inputSha256,
      referenceSha256: graph.sha256(fs.readFileSync(target)) }));
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
}
try { main(); } catch (error) { console.error(error); process.exitCode = 1; }
