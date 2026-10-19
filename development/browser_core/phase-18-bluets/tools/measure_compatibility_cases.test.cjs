// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { executables, testNames, summary, toolHashes } = require('./measure_compatibility_cases.cjs');

test('Cargo artifact matching uses the full source path, not duplicate target basenames', () => {
  const inventory = { suites: [{ harness: 'backend/bluets/tests/example.rs' },
    { harness: 'backend/bluets-bluejs/tests/example.rs' }] };
  const artifact = (harness, executable) => ({ reason: 'compiler-artifact', profile: { test: true },
    target: { kind: ['test'], src_path: '/repo/' + harness }, executable });
  const rows = inventory.suites.map((suite, i) => artifact(suite.harness, '/bin/test-' + i));
  assert.deepEqual(executables(rows, inventory).map(row => row.executable), ['/bin/test-0', '/bin/test-1']);
  assert.throws(() => executables(rows.slice(1), inventory), /missing/);
  assert.throws(() => executables([...rows, rows[0]], inventory), /duplicate/);
  rows[0].target.src_path = 'C:\\repo\\backend\\bluets\\tests\\example.rs';
  assert.equal(executables(rows, inventory)[0].executable, '/bin/test-0');
});

test('libtest names and final counts cannot silently accept an empty or partial run', () => {
  assert.deepEqual(testNames('first: test\nnested::second: test\n\n2 tests, 0 benchmarks\n'), ['first', 'nested::second']);
  assert.deepEqual(summary('test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n', 2),
    { passed: 2, failed: 0, ignored: 0, filtered: 0 });
  for (const text of ['', 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out;',
    'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;']) {
    assert.throws(() => summary(text, 2));
  }
});

test('staged-source CLI is explicit and retains the pinned-oracle requirement', () => {
  const script = path.join(__dirname, 'measure_compatibility_cases.cjs');
  const env = { ...process.env };
  delete env.BLUEICE_BLUETSC_ORACLE;
  for (const name of Object.keys(env)) {
    if (name.startsWith('BLUEICE_WRITE_') || name.startsWith('BLUETS_')) delete env[name];
  }
  const valid = spawnSync(process.execPath, [script, '--output-dir', 'unused-gc1-output', '--staged-source'], { env, encoding: 'utf8' });
  assert.equal(valid.status, 1);
  assert.match(valid.stderr, /set BLUEICE_BLUETSC_ORACLE/);
  const invalid = spawnSync(process.execPath, [script, '--output-dir', 'unused-gc1-output', '--skip-source-check'], { env, encoding: 'utf8' });
  assert.equal(invalid.status, 1);
  assert.match(invalid.stderr, /usage:/);
});

test('the actual oracle scripts are content-bound rather than attributed to an old HEAD', () => {
  const hashes = toolHashes();
  for (const name of ['measure_compatibility_cases.cjs', 'record_gc1_sources.cjs', 'record_gc1_explain.cjs', 'account_inventory_rows.py']) {
    assert.match(hashes[name], /^[a-f0-9]{64}$/);
  }
  assert.deepEqual(Object.keys(hashes), Object.keys(hashes).sort());
  assert.deepEqual(toolHashes(), hashes);
});
