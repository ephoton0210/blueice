// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Public gate over the frozen Native rows and their separate owner policy.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const {createHash} = require('node:crypto');
const {run} = require('./gc3_blue_observer.cjs'), {verify} = require('./gc3_policy.cjs');
const repo = path.resolve(__dirname, '../../../..');
const corpus = path.join(repo, 'backend/bluets/tests/fixtures/gc3_compositions');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function gate(compiler, output) {
  const policy = JSON.parse(fs.readFileSync(path.join(corpus, 'policy.json')));
  assert.equal(hash(fs.readFileSync(path.join(repo, policy.frozenMatrixPath))), policy.frozenMatrixSha256);
  const compilerSha256 = hash(fs.readFileSync(compiler));
  const report = run({compiler, compilerSha256, output, registry: path.join(corpus, 'registry.json'),
    native: path.join(corpus, 'reference.json')});
  const result = verify(report, policy, JSON.parse(fs.readFileSync(path.join(corpus, 'reference.json'))));
  fs.writeFileSync(path.join(output, 'verification.json'), JSON.stringify(result, null, 2) + '\n');
  return result;
}

module.exports = {gate};
if (require.main === module) {
  try {
    assert.equal(process.argv.length, 4, 'compiler and fresh output directory');
    console.log(JSON.stringify(gate(path.resolve(process.argv[2]), path.resolve(process.argv[3]))));
  } catch (error) { console.error(error.stack); process.exitCode = 1; }
}
