// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test'), assert = require('node:assert/strict');
const {ownerDenial, verify} = require('./gc3_policy.cjs');
const {executionScript} = require('./gc3_protocol.cjs');
const native = require('../../../../backend/bluets/tests/fixtures/gc3_compositions/reference.json');

test('physical framing equals every frozen Native execution command', () => {
  for (const row of native.cases.filter(row => row.execution)) {
    assert.equal(executionScript(row.execution.format === 'ESM'), row.execution.command[2]);
  }
});

test('owner denial cannot be satisfied by a generic compiler rejection', () => {
  const row = {verdict: 'reject', compiler: {exit: 1}, files: [], options: {lib: ['ES2022', 'DOM']},
    rawDiagnostics: [{btsCode: 'BTS1001', rawMessage: 'unsupported compiler option `lib` value `DOM`'}],
    unstructuredDiagnosticLines: [], authoredInputMutations: []};
  const role = {reason: 'Host libraries remain owner supplied'};
  assert(ownerDenial(row, role));
  for (const change of [{compiler: {exit: 2}}, {compiler: {exit: 1, signal: 'SIGABRT'}},
    {files: ['unexpected.js']}, {rawDiagnostics: [{btsCode: 'BTS1001', rawMessage: 'unrelated parse failure'}]},
    {unstructuredDiagnosticLines: ['panic']}, {authoredInputMutations: ['changed']}]) {
    assert.equal(ownerDenial({...row, ...change}, role), false);
  }
  assert.equal(ownerDenial(row, {reason: 'unknown policy'}), false);
});

test('partial or duplicated case reports never establish complete acceptance', () => {
  assert.throws(() => verify({cases: []}, {caseRoles: {}}, native), /every frozen row/);
  assert.throws(() => verify({cases: native.cases}, {caseRoles: {}}, native));
});
