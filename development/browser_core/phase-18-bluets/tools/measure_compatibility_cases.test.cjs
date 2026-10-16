// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const { executables, testNames, summary } = require('./measure_compatibility_cases.cjs');

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
