// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Reproduce each stock defect, then compile independently with the local patch.
// No map is edited after emission, and reference.json is never written here.
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), maps = require('./ge5_maps.cjs');
const patch = require('./ge5_native_patch.cjs'), compiler = require('./ge5_native_compile.cjs');
const root = path.resolve(__dirname, '../../../..');
const directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json'));
const referenceBytes = fs.readFileSync(path.join(directory, 'reference.json'));
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes);
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const resolve = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const filename = resolve(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc', 'typescript', 'lib/typescript.js');
const stock = compiler(require(filename)), fixed = compiler(patch.load(filename));
const parents = graph.parents(root, input), all = process.argv.includes('--all');
const target = path.join(directory, 'native-map-defects.json');
const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge5-native-defects-')));
const defects = [], blobs = {}, counts = { configurations: 0, stockReplays: 0, maps: 0, changedMaps: 0 };
const started = Date.now();

function stockObservation(expected) {
  return { diagnostics: expected.diagnostics, selectedInputs: expected.selectedInputs,
    outputs: Object.fromEntries(Object.entries(expected.outputs).map(([name, hash]) => [name, reference.blobs[hash]])) };
}
function compare(expected, actual, label) {
  const { configuration, ...observation } = actual;
  assert.deepEqual(observation, stockObservation(expected), label);
  if (expected.configuration) assert.equal(configuration, reference.blobs[expected.configuration]);
}
function blob(text) { const hash = graph.sha256(text); blobs[hash] = text; return hash; }

try {
  for (const [index, specification] of input.cases.entries()) {
    const row = graph.resolve(input, specification, parents), system = row.options.module === 'System';
    if (!all && !system) continue;
    const expected = reference.cases[index]; assert.equal(specification.id, expected.id);
    const base = path.join(temporary, String(index)); fs.mkdirSync(base); graph.write(base, row.files);
    if (system) {
      const original = stock(base, row); assert.deepEqual(stock(base, row), original, expected.id + ': stock repeat');
      compare(expected, original, expected.id + ': fresh stock vs frozen authority'); counts.stockReplays++;
    }
    const actual = fixed(base, row);
    assert.deepEqual(fixed(base, row), actual, expected.id + ': patched repeat');
    assert.deepEqual(actual.diagnostics, expected.diagnostics, expected.id + ': diagnostics');
    assert.deepEqual(actual.selectedInputs, expected.selectedInputs, expected.id + ': inputs');
    assert.deepEqual(Object.keys(actual.outputs), Object.keys(expected.outputs), expected.id + ': artifact inventory');
    const observations = maps.artifacts(actual.outputs);
    assert.deepEqual(Object.keys(observations), Object.keys(expected.maps));
    for (const [name, hash] of Object.entries(expected.outputs)) {
      if (!expected.maps[name]?.coordinateIssues.length) assert.equal(actual.outputs[name], reference.blobs[hash], expected.id + ': unchanged artifact ' + name);
    }
    for (const [name, before] of Object.entries(expected.maps)) {
      counts.maps++;
      const after = observations[name];
      if (!before.coordinateIssues.length) { assert.equal(after.text, reference.blobs[before.raw]); continue; }
      assert(system && specification.profile === 'external' && before.kind === 'javascript');
      assert.equal(before.coordinateIssues.length, 1);
      const old = maps.decode(reference.blobs[before.raw], reference.blobs[expected.outputs[before.output]], { recordInvalidCoordinates: true });
      const issue = before.coordinateIssues[0], [line, column, source, sourceLine, sourceColumn] = issue.segment;
      assert.equal(issue.kind, 'UTF-16 source bounds'); assert.equal(sourceColumn, 1);
      const sourceText = old.metadata.sourcesContent[source], widths = sourceText.split('\n').map(line => line.length);
      assert.equal(sourceLine, widths.length - 1); assert.equal(widths.at(-1), 0);
      const generatedLine = reference.blobs[expected.outputs[before.output]].split('\n')[line];
      assert.equal(generatedLine.trim(), '}'); assert.equal(generatedLine[column - 1], '}');
      const removed = old.segments.filter(segment => segment[0] === line);
      assert.deepEqual(removed, [[line, column - 1, source, sourceLine, 0], issue.segment]);
      assert.deepEqual(after.decoded.metadata, old.metadata);
      assert.deepEqual(after.decoded.segments, old.segments.filter(segment => segment[0] !== line), expected.id + ': every real statement mapping');
      assert.equal(after.decoded.coordinateIssues.length, 0);
      // Public consumers use same-line GLB: no segment on this generated brace
      // line means neither the brace nor its trailing space has a source owner.
      assert(!after.decoded.segments.some(segment => segment[0] === line));
      defects.push({ id: 'NATIVE-MAP-' + String(defects.length + 1).padStart(4, '0'),
        caseIndex: index, caseId: expected.id, form: specification.form,
        target: row.options.target, variant: specification.variant, module: 'System',
        map: name, output: before.output, source: old.metadata.sources[source],
        sourceSha256: graph.sha256(sourceText), sourceUtf16Length: sourceText.length,
        stockRawSha256: before.raw, stockDecodedSha256: before.decodedSha256,
        issue, generatedLine, removedSegments: removed,
        patchedRawSha256: blob(after.text), patchedDecodedSha256: graph.sha256(JSON.stringify(after.decoded)),
        fix: patch.patchId, nativeVerification: { stockReproduced: true, patchedStrictBounds: true,
          generatedBraceUnmapped: true, realStatementMappingsUnchanged: true,
          nonMapArtifactsUnchanged: true, diagnosticsUnchanged: true, repeatBuilds: 2 } });
      counts.changedMaps++;
    }
    counts.configurations++; fs.rmSync(base, { recursive: true, force: true });
    if (counts.configurations % (all ? 250 : 50) === 0) console.log(JSON.stringify({ ...counts, elapsedSeconds: (Date.now() - started) / 1000 }));
  }
  assert.equal(defects.length, 572); assert.equal(new Set(defects.map(row => row.caseId)).size, 452);
  assert.equal(new Set(defects.map(row => row.stockRawSha256)).size, 138);
  assert.equal(counts.stockReplays, 516);
  const report = { schema: 1, typescript: '5.9.3', patchId: patch.patchId,
    upstreamStatus: 'not-submitted; local-experimental-patch-only',
    inputSha256: graph.sha256(inputBytes), referenceSha256: graph.sha256(referenceBytes),
    stockBundleSha256: patch.stockSha256,
    patchedBundleSha256: graph.sha256(patch.patchSource(fs.readFileSync(filename, 'utf8'))),
    counts: { defects: defects.length, configurations: 452, rawMaps: 138, sourceFamilies: 14,
      nativeSystemConfigurationsReplayedTwice: counts.stockReplays },
    defects, patchedBlobs: Object.fromEntries(Object.entries(blobs).sort(([a], [b]) => a.localeCompare(b))) };
  if (process.argv.includes('--write')) fs.writeFileSync(target, JSON.stringify(report, null, 2) + '\n');
  else assert.deepEqual(report, JSON.parse(fs.readFileSync(target)), 'Native fix ledger changed');
  assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'cases.json'))), report.inputSha256);
  assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'reference.json'))), report.referenceSha256);
  assert.equal(graph.sha256(fs.readFileSync(filename)), patch.stockSha256);
  console.log(JSON.stringify({ scope: all ? 'all-8407-configurations' : 'all-System-configurations',
    ...counts, defects: defects.length, strictFailures: 0, statementMappingDifferences: 0,
    nonMapArtifactDifferences: 0, repeatedBuildDifferences: 0,
    ledgerSha256: graph.sha256(fs.readFileSync(target)), elapsedSeconds: (Date.now() - started) / 1000 }));
} finally { fs.rmSync(temporary, { recursive: true, force: true }); }
