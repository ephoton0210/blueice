// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const {test} = require('node:test');
const {complete} = require('./defu-interchange.cjs');
const matrix = require('./defu-interchange.json');
function successful() {
  return {workflows: matrix.profiles.flatMap(profile => matrix.directions.map(direction =>
    ({id: profile.id + '-' + direction, checks: Object.fromEntries(matrix.requiredChecks.map(key => [key, true]))}))),
  ownerChecks: Object.fromEntries(matrix.ownerChecks.map(key => [key, true]))};
}
test('state evaluation requires both directions of both source-discovery profiles', () => {
  assert.equal(complete(successful()), true);
  const report = successful(); report.workflows[3] = report.workflows[0];
  assert.equal(complete(report), false);
  report.workflows.pop(); assert.equal(complete(report), false);
});
test('every frozen recovery, artifact, runtime and dependent-consumer check is required', () => {
  for (const key of matrix.requiredChecks) {
    const report = successful(); delete report.workflows[0].checks[key];
    assert.equal(complete(report), false, key);
    report.workflows[0].checks[key] = false; assert.equal(complete(report), false, key);
  }
});
test('cross-use cannot substitute success for an untested owner-path boundary', () => {
  for (const key of matrix.ownerChecks) {
    const report = successful(); report.ownerChecks[key] = false;
    assert.equal(complete(report), false, key);
    delete report.ownerChecks[key]; assert.equal(complete(report), false, key);
  }
});
