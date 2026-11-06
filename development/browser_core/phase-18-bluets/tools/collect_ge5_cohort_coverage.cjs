// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Read successful per-map evidence; never infer semantic success from suite counters.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const {sha256} = require('./ge5_inputs.cjs'), {inventory, auditReport} = require('./ge5_cohort_coverage.cjs');
const fileHash = filename => sha256(fs.readFileSync(filename));
function collect({compiler, sourceFreeze, reports, output, measurement}, root = path.resolve(__dirname, '../../../..')) {
  for (const value of [compiler, sourceFreeze, reports, output]) assert(value, 'explicit compiler/freeze/reports/output paths');
  const compilerSha256 = fileHash(compiler), freezeBytes = fs.readFileSync(sourceFreeze), freeze = JSON.parse(freezeBytes);
  assert.equal(freeze.compilerSha256 || freeze.compiler?.sha256, compilerSha256, 'frozen compiler identity');
  assert(Object.keys(freeze.sourceFiles).length, 'nonempty frozen production source inventory');
  const checkSource = () => {
    assert.equal(fileHash(compiler), compilerSha256, 'unchanged compiler');
    for (const [name, hash] of Object.entries(freeze.sourceFiles)) assert.equal(fileHash(path.join(root, name)), hash, 'frozen source ' + name);
  };
  checkSource();
  const measurementErrors = [];
  let measurementBinding = null;
  try {
    assert(measurement, 'successful complete measurement is required');
    const bytes = fs.readFileSync(measurement), measured = JSON.parse(bytes);
    measurementBinding = {path: path.resolve(measurement), sha256: sha256(bytes)};
    const catalog = JSON.parse(fs.readFileSync(path.join(root, 'development/browser_core/phase-18-bluets/COMPATIBILITY_SUITES.json')));
    assert.equal(measured.conclusion, 'success', 'complete measurement succeeded');
    assert.equal(measured.compilerSha256, compilerSha256, 'measured compiler identity');
    assert.equal(measured.source_revision, catalog.source_revision);
    assert.equal(measured.source_graph_sha256, catalog.source_graph_sha256);
    assert.deepEqual(measured.suites.map(row => row.harness).sort(), catalog.suites.map(row => row.harness).sort(), 'every measured suite');
    assert(measured.suites.every(row => row.conclusion === 'success' && !row.failed && !row.ignored && !row.filtered), 'no partial measured suite');
    assert(Object.keys(measured.oracle_tools_sha256).length, 'actual measured tool hashes');
    for (const [name, hash] of Object.entries(measured.oracle_tools_sha256)) assert.equal(fileHash(path.join(__dirname, name)), hash, 'measured tool ' + name);
    for (const name of ['collect_ge5_cohort_coverage.cjs', 'ge5_cohort_coverage.cjs']) assert.equal(measured.oracle_tools_sha256[name], fileHash(path.join(__dirname, name)), 'collector was frozen before measurement');
  } catch (error) { measurementErrors.push(error.message); }
  const directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
  const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), referenceBytes = fs.readFileSync(path.join(directory, 'reference.json'));
  assert.equal(sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
  assert.equal(sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
  const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), manifestBytes = fs.readFileSync(reports), entries = JSON.parse(manifestBytes);
  assert(Array.isArray(entries), 'explicit report manifest array');
  const accepted = [], rejectedReports = [], reportBindings = [];
  const resolve = filename => path.resolve(path.dirname(path.resolve(reports)), filename);
  for (const entry of entries) {
    const binding = {path: entry.path, kind: entry.kind || 'whole-map'};
    try {
      assert.equal(typeof entry.path, 'string');
      const filename = resolve(entry.path), bytes = fs.readFileSync(filename); binding.path = filename; binding.sha256 = sha256(bytes);
      if (entry.sha256) assert.equal(binding.sha256, entry.sha256, 'report digest');
      const report = JSON.parse(bytes); let observations;
      if (entry.observations) {
        const filename = resolve(entry.observations), bytes = fs.readFileSync(filename);
        binding.observations = filename; binding.observationsSha256 = sha256(bytes);
        if (entry.observationsSha256) assert.equal(binding.observationsSha256, entry.observationsSha256);
        if (report.existingObservationsSha256) assert.equal(binding.observationsSha256, report.existingObservationsSha256);
        observations = JSON.parse(bytes);
      }
      const audited = auditReport(input, reference, report, observations, compilerSha256, binding.kind);
      inventory(input, reference, [...accepted, audited], compilerSha256);
      accepted.push(audited); reportBindings.push(binding);
    } catch (error) { rejectedReports.push({...binding, error: error.message}); }
  }
  const coverage = inventory(input, reference, accepted, compilerSha256);
  assert.equal(coverage.configurations, 8407); assert.equal(coverage.families.length, 137);
  assert.equal(coverage.expectedJavascriptMaps, 10193);
  const declarationMaps = reference.cases.reduce((sum, row) => sum + Object.values(row.maps).filter(map => map.kind === 'declaration').length, 0);
  assert.equal(declarationMaps, 10397);
  checkSource();
  const result = {...coverage, declarationMaps, allMaps: declarationMaps + coverage.expectedJavascriptMaps,
    evidenceAccepted: coverage.completeJavascriptMapInventory && rejectedReports.length === 0 && measurementErrors.length === 0,
    rejectedReports, reportBindings, measurementBinding, measurementErrors, sourceFreezeSha256: sha256(freezeBytes), reportManifestSha256: sha256(manifestBytes),
    inputSha256: sha256(inputBytes), referenceSha256: sha256(referenceBytes),
    collectorSources: Object.fromEntries(['collect_ge5_cohort_coverage.cjs', 'ge5_cohort_coverage.cjs', 'ge5_native_authority.cjs'].map(name => [name, fileHash(path.join(__dirname, name))])),
    fullCohortAccepted: false};
  fs.writeFileSync(output, JSON.stringify(result, null, 2) + '\n');
  return result;
}
module.exports = {collect};
if (require.main === module) {
  try {
    const names = {'--compiler':'compiler', '--source-freeze':'sourceFreeze', '--reports':'reports', '--output':'output', '--measurement':'measurement'}, options = {};
    for (let at = 2; at < process.argv.length; at += 2) {
      const name = names[process.argv[at]]; assert(name && process.argv[at + 1] && !options[name], 'known unique option and value');
      options[name] = process.argv[at + 1];
    }
    const result = collect(options);
    console.log(JSON.stringify({configurations: result.configurations, families: result.families.length,
      expectedJavascriptMaps: result.expectedJavascriptMaps, coveredJavascriptMaps: result.coveredJavascriptMaps,
      missingMaps: result.missingMaps.length, rejectedReports: result.rejectedReports.length, measurementErrors: result.measurementErrors.length,
      completeJavascriptMapInventory: result.completeJavascriptMapInventory, fullCohortAccepted: false}));
    if (!result.evidenceAccepted) process.exitCode = 1;
  } catch (error) { console.error(error.stack); process.exitCode = 1; }
}
