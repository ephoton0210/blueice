// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exact inventory accounting for independently executed, source-bound contracts.
// This ledger does not establish their semantics or certify G-E5 completion.
const assert = require('node:assert/strict');
const {sha256} = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const auditedReports = new WeakSet();
const hash = value => assert.match(value, /^[0-9a-f]{64}$/, 'bound artifact SHA-256');

function inventory(input, reference, reports, compilerSha256) {
  hash(compilerSha256);
  assert.equal(input.cases.length, reference.cases.length, 'complete authority case identities');
  const ids = new Set(), expected = new Map(), families = new Map();
  let configurationsWithoutJavascriptMaps = 0;
  for (const [index, row] of input.cases.entries()) {
    assert.equal(typeof row.id, 'string'); assert.equal(typeof row.form, 'string');
    assert(!ids.has(row.id), 'unique frozen case identity'); ids.add(row.id);
    const native = reference.cases[index]; assert.equal(native.id, row.id);
    const family = families.get(row.form) || {form: row.form, configurations: 0, expectedMaps: 0, coveredMaps: 0};
    family.configurations++; families.set(row.form, family);
    let maps = 0;
    for (const [name, map] of Object.entries(native.maps)) {
      if (map.kind !== 'javascript') continue;
      assert(Object.hasOwn(native.outputs, map.output), 'map has its exact authority artifact');
      const nativeJavaScriptSha256 = native.outputs[map.output]; hash(nativeJavaScriptSha256);
      const nativeMapSha256 = sha256(authority.raw(reference, native, name));
      expected.set(JSON.stringify([index, name]), {index, id: row.id, form: row.form, map: name,
        nativeJavaScriptSha256, nativeMapSha256});
      family.expectedMaps++; maps++;
    }
    configurationsWithoutJavascriptMaps += Number(maps === 0);
  }
  const covered = new Map(); let duplicateConsistentRows = 0;
  for (const report of reports) {
    assert.equal(report.compilerSha256, compilerSha256, 'one actual compiler');
    assert.equal(report.repeatBuilds, 2, 'two complete builds');
    assert.deepEqual(report.failures, [], 'failed reports cannot enter successful coverage');
    assert(!report.probeOnly && !report.positiveOnly, 'full contract execution');
    assert.notEqual(report.completeFamily, false, 'incomplete family report');
    assert.notEqual(report.complete, false, 'incomplete contract report');
    assert(report.completeFamilies !== false || auditedReports.has(report), 'unaudited target subset');
    assert(Array.isArray(report.verified), 'explicit per-map records required');
    for (const row of report.verified) {
      assert(Number.isInteger(row.index) && row.index >= 0, 'original frozen case index');
      const key = JSON.stringify([row.index, row.map]), wanted = expected.get(key);
      assert(wanted, 'exact frozen JavaScript map identity');
      assert.equal(row.id, wanted.id, 'index and source identity agree');
      assert(row.allCharactersClassified === true || auditedReports.has(report), 'whole-map proof record required');
      for (const field of ['nativeJavaScriptSha256', 'nativeMapSha256']) {
        assert.equal(row[field], wanted[field], 'exact separately corrected Native authority');
      }
      hash(row.actualJavaScriptSha256); hash(row.actualMapSha256);
      const identity = {...wanted, actualJavaScriptSha256: row.actualJavaScriptSha256,
        actualMapSha256: row.actualMapSha256};
      if (covered.has(key)) {
        assert.deepEqual(covered.get(key), identity, 'overlap must preserve identical actual artifacts');
        duplicateConsistentRows++;
      } else {
        covered.set(key, identity); families.get(wanted.form).coveredMaps++;
      }
    }
  }
  const missingMaps = [...expected].filter(([key]) => !covered.has(key)).map(([, row]) => row);
  return {compilerSha256, configurations: input.cases.length, families: [...families.values()],
    configurationsWithoutJavascriptMaps, expectedJavascriptMaps: expected.size,
    coveredJavascriptMaps: covered.size, coveredMaps: [...covered.values()], duplicateConsistentRows, missingMaps,
    completeJavascriptMapInventory: missingMaps.length === 0,
    fullCohortAccepted: false,
    scope: 'Inventory only. Actual contract semantics, endpoints, controls, declarations, runtime and process gates require their separate source-bound evidence.'};
}
// An adapter preserves its writer's real proof marker; it never invents a whole-map flag.
function auditReport(input, reference, report, observations, compilerSha256, kind = 'whole-map') {
  assert(['whole-map', 'utf16-module', 'static-super', 'metadata-module', 'class-expressions', 'interop-module'].includes(kind), 'unknown report adapter');
  assert.equal(report.compilerSha256, compilerSha256, 'one actual compiler');
  assert.deepEqual(report.failures, [], 'failed report');
  assert(!report.probeOnly && !report.positiveOnly && !report.shard, 'partial/probe report');
  for (const flag of ['completeFamily', 'complete']) assert.notEqual(report[flag], false, 'incomplete report ' + flag);
  if (report.completeFamilies === false) assert(kind === 'whole-map' && ['ES2015 and later native generator syntax', 'ES2015 and later named async functions'].includes(report.scope), 'unknown partial contract');
  if (kind === 'whole-map' && Array.isArray(report.verified) && report.verified.some(row => Array.isArray(row.maps))) {
    report = {...report, verified: report.verified.flatMap(row => {
      assert(Array.isArray(row.maps) && row.runtime, 'complete nested class map/runtime proof');
      return row.maps.map(map => ({...map, index: row.index, id: row.id}));
    })};
  }
  if (kind === 'interop-module') {
    assert.equal(report.completeFamily, true); assert.equal(report.form, 'ge2_module_helpers/interop-exportstar-cycle');
    assert(Array.isArray(report.products) && Array.isArray(report.executions), 'full interop report, not stdout summary');
    assert.equal(report.products.length, report.maps);
    assert.equal(report.selectedEditionSyntaxPrograms, report.configurations * 8);
    report = {...report, verified: report.products.map(row => ({...row, actualJavaScriptSha256: row.actualCodeSha256}))};
  }
  if (kind === 'metadata-module' || kind === 'class-expressions') {
    const metadata = kind === 'metadata-module' ? report.metadata : report.prelude;
    assert(Array.isArray(metadata) && Array.isArray(report.verified) && Array.isArray(report.syntax) && Array.isArray(report.runtime), 'complete joined module report');
    if (kind === 'metadata-module') assert.equal(report.scope, 'complete Native-derived consumer/provider module roles, artifacts and execution');
    const key = row => JSON.stringify([row.index, row.map]), hashes = new Map(metadata.map(row => [key(row), row]));
    assert.equal(hashes.size, metadata.length); assert.equal(metadata.length, report.verified.length);
    const used = new Set();
    report = {...report, verified: report.verified.map(proof => {
      const identity = key(proof), data = hashes.get(identity); assert(data && !used.has(identity), 'one complete proof per metadata/prelude map'); used.add(identity);
      if (kind === 'metadata-module') {
        assert.equal(typeof proof.module, 'string'); assert(Number.isInteger(proof.classes) && proof.classes >= 0);
        assert(Number.isInteger(proof.metadataConstructors) && proof.metadataConstructors >= 0);
      } else {
        for (const field of ['completeSourceCallables', 'completeClassValues', 'deferredDecoratedCallables']) assert(Number.isInteger(data[field]) && data[field] >= 0);
        const controls = report.controls?.filter(row => key(row) === identity); assert.equal(controls?.length, 1);
        assert(controls[0].controls.length && controls[0].controls.every(row => row.rejected));
      }
      for (const producer of ['Native', 'Blue']) {
        const syntax = report.syntax.filter(row => key(row) === identity && row.producer === producer); assert.equal(syntax.length, 1);
        assert.equal(syntax[0].codeSha256, data[producer === 'Native' ? 'nativeJavaScriptSha256' : 'actualJavaScriptSha256']);
      }
      const runtime = report.runtime.filter(row => row.index === proof.index); assert.equal(runtime.length, 1); assert(runtime[0].executions >= 2);
      return {...proof, ...data};
    })};
  }
  assert(Array.isArray(report.verified) && report.verified.length, 'explicit whole-map records');
  if (Number.isInteger(report.maps)) assert.equal(report.maps, report.verified.length, 'complete per-map inventory');
  if (Number.isInteger(report.javascriptMaps)) assert.equal(report.javascriptMaps, report.verified.length, 'complete Node per-map inventory');
  const ids = new Map(input.cases.map((row, index) => [row.id, index]));
  assert.equal(ids.size, input.cases.length, 'unique canonical source identity');
  const observed = new Map();
  if (observations) {
    assert(Array.isArray(observations), 'actual observation array');
    for (const row of observations) {
      assert(Number.isInteger(row.index) && input.cases[row.index]?.id === row.id, 'actual original observation identity');
      assert(!observed.has(row.index), 'duplicate actual observation');
      assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2, 'actual two-build observation');
      observed.set(row.index, row);
    }
  }
  if (report.repeatBuilds !== undefined) assert.equal(report.repeatBuilds, 2, 'two actual builds');
  else assert(['static-super', 'interop-module'].includes(kind) && observed.size, 'missing real two-build evidence');
  if (kind === 'utf16-module') {
    assert.equal(report.completeFamily, true, 'complete UTF-16 module family');
    assert(['ge2_module_helpers/standard-decorator', 'ge2_module_helpers/private-accessor'].includes(report.family), 'unknown UTF-16 writer');
  }
  if (kind === 'interop-module') {
    const selected = input.cases.flatMap((row, index) => row.form === report.form ? [index] : []);
    assert.deepEqual([...observed.keys()].sort((a,b) => a-b), selected, 'complete actual interop cohort');
    assert.equal(report.configurations, selected.length);
    assert.deepEqual(report.executions.map(row => row.index).sort((a,b) => a-b), selected);
  }
  if (kind === 'static-super') {
    assert.equal(report.completeFamily, true); assert.equal(report.probeOnly, false);
    const forms = new Set([...require('./ge5_static_super_observe.cjs').families.keys()].map(name => 'ge3_decorator_compositions/static-super-' + name));
    const selected = input.cases.flatMap((row, index) => forms.has(row.form) ? [index] : []);
    assert.equal(report.frozenConfigurations, selected.length);
    assert.deepEqual([...observed.keys()].sort((a,b) => a-b), selected, 'complete actual static two-build cohort');
    assert.equal(report.verified.length, selected.length);
    assert(Array.isArray(report.executions));
    assert.deepEqual(report.executions.map(row => row.index).sort((a,b) => a-b), selected, 'complete static runtime cohort');
    assert(report.executions.every(row => row.executions === 2));
    assert.equal(report.totalExecutions, selected.length * 2);
  }
  const verified = report.verified.map(row => {
    const index = row.index === undefined ? ids.get(row.id) : row.index;
    assert(Number.isInteger(index) && (row.id === undefined || input.cases[index]?.id === row.id), 'canonical original index/id');
    const native = reference.cases[index], map = native.maps[row.map];
    assert(map?.kind === 'javascript', 'canonical original JavaScript map');
    const field = kind === 'static-super' ? 'completeModuleRoles' : kind === 'utf16-module' ? 'completeUTF16ModuleRoles' : 'allCharactersClassified';
    if (!['metadata-module', 'class-expressions', 'interop-module'].includes(kind)) assert.equal(row[field], true, 'existing complete proof marker ' + field);
    if (['static-super', 'utf16-module', 'interop-module'].includes(kind)) {
      assert(Array.isArray(row.controls) && row.controls.length && row.controls.every(control => control.rejected), 'effective complete controls');
      if (kind === 'utf16-module') assert(row.runtime, 'complete module runtime');
    }
    const nativeJavaScriptSha256 = native.outputs[map.output], nativeMapSha256 = sha256(authority.raw(reference, native, row.map));
    if (row.nativeJavaScriptSha256 !== undefined) assert.equal(row.nativeJavaScriptSha256, nativeJavaScriptSha256);
    if (row.nativeMapSha256 !== undefined) assert.equal(row.nativeMapSha256, nativeMapSha256);
    let actualJavaScriptSha256 = row.actualJavaScriptSha256, actualMapSha256 = row.actualMapSha256;
    const observation = observed.get(index);
    if (observation) {
      const artifact = require('./ge5_maps.cjs').artifacts(observation.outputs)[row.map];
      assert(artifact, 'actual map bytes');
      const javascript = observation.outputs[artifact.output];
      const jsHash = sha256(javascript), mapHash = sha256(artifact.text);
      if (actualJavaScriptSha256 !== undefined) assert.equal(actualJavaScriptSha256, jsHash, 'actual JS bytes');
      if (actualMapSha256 !== undefined) assert.equal(actualMapSha256, mapHash, 'actual raw/decoded-inline map bytes');
      actualJavaScriptSha256 = jsHash; actualMapSha256 = mapHash;
    }
    hash(actualJavaScriptSha256); hash(actualMapSha256);
    // Missing original Native fields may only be derived with real actual byte evidence.
    if (row.nativeJavaScriptSha256 === undefined || row.nativeMapSha256 === undefined) assert(observation, 'missing Native hashes require actual byte evidence');
    return Object.freeze({...row, index, id: input.cases[index].id, nativeJavaScriptSha256, nativeMapSha256, actualJavaScriptSha256, actualMapSha256});
  });
  const result = Object.freeze({...report, repeatBuilds: report.repeatBuilds ?? observations[0].repeatBuilds, verified: Object.freeze(verified)});
  auditedReports.add(result); return result;
}
module.exports = {inventory, auditReport};
