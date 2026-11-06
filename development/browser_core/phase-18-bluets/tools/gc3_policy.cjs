// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Acceptance consumes complete observations; policy denials are separate controls.
const assert = require('node:assert/strict');
const {syntaxProjection, preservedProjection, diagnosticProjection, outcome} = require('./gc3_protocol.cjs');

function ownerDenial(row, role) {
  if (row.verdict !== 'reject' || row.compiler.exit !== 1 || row.files.length ||
      row.compiler.signal || row.compiler.error || row.unstructuredDiagnosticLines.length ||
      row.authoredInputMutations.length || !row.rawDiagnostics.length) return false;
  if (role.reason === 'Host libraries remain owner supplied') {
    return row.rawDiagnostics.every(diagnostic => diagnostic.btsCode === 'BTS1001' &&
      diagnostic.rawMessage === 'unsupported compiler option `lib` value `DOM`' &&
      row.options.lib.includes('DOM'));
  }
  if (role.reason === 'Project declaration does not grant executable runtime value') {
    return row.rawDiagnostics.some(diagnostic => diagnostic.btsCode === 'BTS2002' &&
      diagnostic.rawMessage === 'value re-export resolves to a declaration module; declaration modules are type-only');
  }
  return false;
}

function verify(report, policy, native) {
  const expected = new Map(native.cases.map(row => [row.id, row]));
  assert.equal(expected.size, native.cases.length, 'unique Native cases');
  assert.equal(report.cases.length, expected.size, 'every frozen row observed');
  assert.equal(Object.keys(policy.caseRoles).length, expected.size);
  const seen = new Set(), direct = [], controls = [], failures = [];
  for (const row of report.cases) {
    assert(expected.has(row.id) && !seen.has(row.id), 'exact unique frozen case');
    seen.add(row.id);
    const role = policy.caseRoles[row.id];
    assert(role && ['direct', 'owner-policy-control'].includes(role.comparison));
    if (role.comparison === 'direct') {
      direct.push(row.id);
      const reference = expected.get(row.id);
      assert.equal(row.verdict, reference.verdict, row.id + ': verdict');
      assert.deepEqual(row.diagnostics.map(value => diagnosticProjection(value)),
        reference.diagnostics.map(value => diagnosticProjection(value)), row.id + ': full diagnostics');
      assert.deepEqual(row.selectedInputs, reference.selectedInputs, row.id + ': selected inputs');
      assert.deepEqual(row.files, reference.files, row.id + ': full artifact inventory');
      for (const file of reference.files.filter(file => /\.d\.[cm]?ts$/.test(file))) {
        assert.equal(row.outputs[file], reference.outputs[file], row.id + ': raw declaration ' + file);
      }
      assert.deepEqual(syntaxProjection(row.syntax), syntaxProjection(reference.syntax), row.id + ': syntax');
      assert.deepEqual(outcome(row.execution), outcome(reference.execution), row.id + ': physical execution');
      assert.deepEqual(preservedProjection(row.preservedSyntax), preservedProjection(reference.preservedSyntax), row.id + ': preserved JSX');
      for (const name of ['originProblems', 'positionProblems', 'unstructuredDiagnosticLines', 'authoredInputMutations']) {
        assert.deepEqual(row[name], [], row.id + ': ' + name);
      }
      assert(!row.compiler.signal && !row.compiler.error, row.id + ': compiler completed normally');
      if (row.differences.length) failures.push({id: row.id, differences: row.differences});
    } else {
      controls.push({id: row.id, reason: role.reason});
      if (!ownerDenial(row, role)) failures.push({id: row.id, differences: ['required-owner-policy-denial']});
    }
  }
  assert.equal(direct.length, 220);
  assert.equal(controls.length, 76);
  assert.deepEqual(failures, [], 'complete direct comparisons and separate owner controls');
  const runtimeFailures = report.cases.filter(row => direct.includes(row.id) && row.execution &&
    (row.execution.exit !== 0 || row.execution.error)).map(row => row.id).sort();
  const nativeRuntimeFailures = native.cases.filter(row => direct.includes(row.id) && row.execution &&
    (row.execution.exit !== 0 || row.execution.error)).map(row => row.id).sort();
  assert.deepEqual(runtimeFailures, nativeRuntimeFailures, 'actual Native runtime failures retained');
  assert.equal(runtimeFailures.length, 12);
  return {conclusion: 'success', configurations: report.cases.length, directCompared: direct.length,
    ownerControls: controls, nativeRuntimeFailures: runtimeFailures,
    compilerSha256: report.compiler.sha256, fullGC3Accepted: false};
}

module.exports = {ownerDenial, verify};
