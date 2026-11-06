// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Connect the complete repeated CLI collector to the complete whole-module gate.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const {spawnSync} = require('node:child_process'), {sha256} = require('./ge5_inputs.cjs');
const compiler = path.resolve(process.argv[2]), expected = process.argv[3];
assert.equal(sha256(fs.readFileSync(compiler)), expected, 'explicit immutable compiler capability');
const observations = require('./ge5_static_super_observe.cjs').collect(compiler, expected);
assert.equal(observations.length, 282);
const observationDirectory = process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY;
if (observationDirectory) {
  fs.mkdirSync(observationDirectory, {recursive: true});
  fs.writeFileSync(path.join(observationDirectory, 'static-super-complete.json'), JSON.stringify(observations) + '\n');
}
const result = spawnSync(process.execPath,
  ['--experimental-vm-modules', path.join(__dirname, 'verify_ge5_static_super_roles.cjs')],
  {input: JSON.stringify(observations), encoding: 'utf8', maxBuffer: 128 * 1024 * 1024,
    env: {...process.env, BLUEICE_GE5_CLI_PATH: compiler}});
const reportDirectory = process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY;
if (reportDirectory) {
  fs.mkdirSync(reportDirectory, {recursive: true});
  fs.writeFileSync(path.join(reportDirectory, 'static-super-complete.raw.log'),
    (result.stdout || '') + (result.stderr || ''));
}
assert(!result.error, result.error?.message);
assert.equal(result.status, 0, result.stderr || result.stdout);
const report = JSON.parse(result.stdout);
assert.equal(report.compilerSha256, expected);
assert.equal(report.completeFamily, true);
assert.equal(report.verified.length, 282);
assert.deepEqual(report.failures, []);
assert(report.verified.every(row => row.completeModuleRoles === true));
assert.equal(sha256(fs.readFileSync(compiler)), expected, 'compiler unchanged through complete gate');
if (reportDirectory) fs.writeFileSync(path.join(reportDirectory, 'static-super-complete.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({compilerSha256: expected, configurations: 282,
  maps: report.verified.length, executions: report.totalExecutions,
  controls: report.effectiveRejectedControls, failures: 0, completeFamily: true}));
