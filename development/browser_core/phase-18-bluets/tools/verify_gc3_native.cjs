// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Reproduce the frozen references; path normalization only removes capture roots.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const {createHash} = require('node:crypto');
const {record} = require('./gc3_native_observer.cjs');
const {outcome} = require('./gc3_protocol.cjs');
const repo = path.resolve(__dirname, '../../../..');
const corpus = path.join(repo, 'backend/bluets/tests/fixtures/gc3_compositions');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function verifyNative(output) {
  const registry = path.join(corpus, 'registry.json'), reference = path.join(corpus, 'reference.json');
  const policy = JSON.parse(fs.readFileSync(path.join(corpus, 'policy.json')));
  const original = JSON.parse(fs.readFileSync(reference));
  assert.equal(hash(fs.readFileSync(registry)), policy.registrySha256);
  assert.equal(hash(fs.readFileSync(reference)), policy.nativeReferenceSha256);
  assert.equal(hash(fs.readFileSync(path.join(repo, policy.frozenMatrixPath))), policy.frozenMatrixSha256);
  const observed = record(registry, output);
  assert.deepEqual(observed.counts, original.counts);
  assert.equal(observed.cases.length, original.cases.length);
  assert.deepEqual(observed.duplicateConfigurations, original.duplicateConfigurations);
  const originalOutput = path.join(path.dirname(original.registry.path), 'native');
  const fields = ['id','form','family','configuration','identity','entry','resolvedEntry','roots','options',
    'emitMode','sources','verdict','diagnostics','emitSkipped','selectedInputs','libraryInputs','files','syntax','preservedSyntax'];
  for (let index = 0; index < observed.cases.length; index++) {
    const actual = observed.cases[index], expected = original.cases[index];
    for (const field of fields) assert.deepEqual(actual[field], expected[field], expected.id + ': ' + field);
    const actualRoot = path.join(output, actual.directory), expectedRoot = path.join(originalOutput, expected.directory);
    for (const file of expected.files) {
      const dts = /\.d\.[cm]?ts$/.test(file);
      const normalize = (text, root) => dts ? text : text.replaceAll(root.replaceAll('\\','/'), '<root>').replaceAll(root, '<root>');
      assert.equal(normalize(actual.outputs[file], actualRoot), normalize(expected.outputs[file], expectedRoot), expected.id + ': Native artifact ' + file);
    }
    assert.deepEqual(outcome(actual.execution), outcome(expected.execution), expected.id + ': Native execution');
  }
  const result = {conclusion: 'success', configurations: observed.cases.length, counts: observed.counts,
    registrySha256: policy.registrySha256, nativeReferenceSha256: policy.nativeReferenceSha256,
    fullGC3Accepted: false};
  fs.writeFileSync(path.join(output, 'verification.json'), JSON.stringify(result, null, 2) + '\n');
  return result;
}

module.exports = {verifyNative};
if (require.main === module) {
  try {
    assert.equal(process.argv.length, 3, 'fresh Native output directory');
    console.log(JSON.stringify(verifyNative(path.resolve(process.argv[2]))));
  } catch (error) { console.error(error.stack); process.exitCode = 1; }
}
