// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const path = require('node:path');
const fs = require('node:fs'), os = require('node:os'), crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const { executables, compilerGuard, testNames, summary, toolHashes, suiteJobs, runSuites, parseArguments } = require('./measure_compatibility_cases.cjs');

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
  const parallel = spawnSync(process.execPath, [script, '--output-dir', 'unused-gc1-output', '--jobs', '2', '--staged-source'], {env, encoding: 'utf8'});
  assert.equal(parallel.status, 1);
  assert.match(parallel.stderr, /set BLUEICE_BLUETSC_ORACLE/);
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

test('nested oracle helper bytes are part of the measured tool identity', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-nested-oracle-'));
  try {
    fs.mkdirSync(path.join(directory, 'string_intrinsics'));
    fs.writeFileSync(path.join(directory, 'main.cjs'), 'main bytes');
    const helper = path.join(directory, 'string_intrinsics/consumers.cjs');
    fs.writeFileSync(helper, 'first consumer bytes');
    const before = toolHashes(directory);
    assert.deepEqual(Object.keys(before), ['main.cjs', 'string_intrinsics/consumers.cjs']);
    assert.equal(before['string_intrinsics/consumers.cjs'], crypto.createHash('sha256').update('first consumer bytes').digest('hex'));
    fs.writeFileSync(helper, 'changed consumer bytes');
    const after = toolHashes(directory);
    assert.equal(after['main.cjs'], before['main.cjs']);
    assert.notEqual(after['string_intrinsics/consumers.cjs'], before['string_intrinsics/consumers.cjs']);
  } finally { fs.rmSync(directory, {recursive: true, force: true}); }
});

function compilerFixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ge5-cli-guard-'));
  const normal = path.join(directory, 'bluetsc'), unit = path.join(directory, 'bluetsc-unit-test');
  fs.writeFileSync(normal, 'normal CLI bytes'); fs.writeFileSync(unit, 'unit-test harness bytes');
  const digest = filename => crypto.createHash('sha256').update(fs.readFileSync(filename)).digest('hex');
  const artifact = (executable, testProfile) => ({reason: 'compiler-artifact',
    target: {kind: ['bin'], name: 'bluetsc', test: true}, profile: {test: testProfile}, executable});
  return {directory, normal, unit, digest, artifact};
}

test('compiler guard selects profile.test=false normal bin and ignores the unit-test executable', () => {
  const f = compilerFixture();
  try {
    const normal = f.artifact(f.normal, false), unit = f.artifact(f.unit, true);
    const guard = compilerGuard([unit, normal, {...normal, fresh: true}], f.digest(f.normal));
    assert.equal(guard.executable, f.normal); assert.doesNotThrow(guard.check);
    assert.throws(() => compilerGuard([unit], f.digest(f.unit)), /exactly one normal/);
    assert.throws(() => compilerGuard([normal, f.artifact(f.unit, false)], f.digest(f.normal)), /exactly one normal/);
  } finally { fs.rmSync(f.directory, {recursive: true, force: true}); }
});

test('no-run compiler mismatch is rejected immediately before a guard can launch any suite', () => {
  const f = compilerFixture();
  try {
    let suiteLaunched = false;
    assert.throws(() => {
      compilerGuard([f.artifact(f.normal, false)], f.digest(f.unit));
      suiteLaunched = true;
    }, /compiler SHA-256 changed/);
    assert.equal(suiteLaunched, false);
  } finally { fs.rmSync(f.directory, {recursive: true, force: true}); }
});

test('matching CLI stays bound to the expected digest and later replacement or deletion fails', () => {
  const f = compilerFixture();
  try {
    const expected = f.digest(f.normal), guard = compilerGuard([f.artifact(f.normal, false)], expected);
    assert.doesNotThrow(guard.check); assert.equal(guard.sha256, expected);
    fs.copyFileSync(f.unit, f.normal);
    assert.throws(guard.check, /compiler SHA-256 changed/);
    assert.equal(guard.sha256, expected, 'a rebuild never updates the frozen expectation');
    fs.unlinkSync(f.normal); assert.throws(guard.check, /ENOENT/);
  } finally { fs.rmSync(f.directory, {recursive: true, force: true}); }
});

test('unset expected digest preserves generic measurement and malformed set values fail closed', () => {
  assert.equal(compilerGuard([], undefined), null);
  for (const expected of ['', 'old-compiler', 'a'.repeat(63), 'A'.repeat(64)]) {
    assert.throws(() => compilerGuard([], expected), /invalid expected compiler/);
  }
});

test('suite jobs default to one and only the bounded explicit CLI is admitted', () => {
  assert.equal(suiteJobs(), 1);
  assert.deepEqual(parseArguments(['--output-dir', 'out']), {output: 'out', staged: false, jobs: 1});
  for (const jobs of [1, 2, 3, 4]) {
    assert.equal(suiteJobs(jobs), jobs);
    assert.deepEqual(parseArguments(['--output-dir', 'out', '--jobs', String(jobs), '--staged-source']),
      {output: 'out', staged: true, jobs});
  }
  for (const value of [0, 5, -1, 1.5, NaN, '2']) assert.throws(() => suiteJobs(value), /1 to 4/);
  for (const args of [['--jobs'], ['--jobs', '0'], ['--jobs', '5'], ['--jobs', '2.0'],
    ['--jobs', '2', '--jobs', '3'], ['--staged-source', '--staged-source'], ['--filter', 'first']])
    assert.throws(() => parseArguments(['--output-dir', 'out', ...args]), /usage:/);
});

// Real child processes emulate the libtest public protocol, without Cargo,
// native oracles, fixture reuse, or a call to the repository's compiler.
const fakeChild = `#!/usr/bin/env node
const fs = require('node:fs'), path = require('node:path');
const c = JSON.parse(fs.readFileSync(__filename + '.json'));
const event = kind => fs.appendFileSync(process.env.GE5_FAKE_TRACE,
  JSON.stringify({kind, index: c.index, args: process.argv.slice(2)}) + '\\n');
if (process.argv.includes('--list')) {
  event('list');
  process.stdout.write(process.argv.includes('--ignored') ? (c.ignored || 'ignored') + ': test\\n'
    : 'first: test\\n' + (c.ignored || 'ignored') + ': test\\n');
} else {
  event('start');
  setTimeout(() => {
    const keys = ['BLUEICE_GC3_REPORT_DIRECTORY', 'BLUEICE_GE5_ROLE_REPORT_DIRECTORY', 'BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY',
      'BLUEICE_GE5_ADJACENT_OBSERVATION_DIRECTORY', 'BLUEICE_GE5_NODE_ROLE_REPORT',
      'BLUEICE_GE5_ASYNC_ROLE_REPORT', 'BLUEICE_GE5_CONTINUATION_ROLE_REPORT', 'BLUEICE_GE5_NATIVE_FIX_REPORT',
      'BLUEICE_GE5_SYSTEM_PHASE_REPORT', 'BLUEICE_GE5_REPORT'];
    const locations = {};
    for (const key of keys) if (process.env[key]) {
      const file = key.endsWith('_DIRECTORY') ? path.join(process.env[key], 'shared.json') : process.env[key];
      fs.mkdirSync(path.dirname(file), {recursive: true}); fs.writeFileSync(file, JSON.stringify({index: c.index}));
      locations[key] = process.env[key];
    }
    fs.appendFileSync(process.env.GE5_FAKE_LOCATIONS, JSON.stringify({index: c.index, locations}) + '\\n');
    if (c.mutate) fs.writeFileSync(c.compiler, 'changed CLI bytes');
    event('end');
    process.stdout.write(c.fail
      ? 'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;\\n'
      : c.partial ? 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\\n'
      : 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\\n');
    process.exitCode = c.fail ? 1 : 0;
  }, c.delay || 0);
}
`;

function scheduleFixture(configurations) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ge5-suite-jobs-'));
  const output = path.join(directory, 'measurement'); fs.mkdirSync(output);
  const trace = path.join(directory, 'trace.jsonl'), locations = path.join(directory, 'locations.jsonl');
  fs.writeFileSync(trace, ''); fs.writeFileSync(locations, '');
  const compiler = path.join(directory, 'bluetsc'); fs.writeFileSync(compiler, 'frozen CLI');
  const expected = crypto.createHash('sha256').update(fs.readFileSync(compiler)).digest('hex');
  const guard = compilerGuard([{reason: 'compiler-artifact', target: {name: 'bluetsc', kind: ['bin']},
    profile: {test: false}, executable: compiler}], expected);
  const binaries = configurations.map((c, index) => {
    const executable = path.join(directory, 'suite-' + index);
    fs.writeFileSync(executable, fakeChild, {mode: 0o755});
    fs.writeFileSync(executable + '.json', JSON.stringify({...c, index, compiler}));
    return {executable, harness: 'backend/bluets/tests/fake_' + index + '.rs'};
  });
  const inventory = {suites: binaries.map(b => ({harness: b.harness, ignored_tests: ['ignored']}))};
  const report = {conclusion: 'in_progress', suites: []};
  const environment = {...process.env, GE5_FAKE_TRACE: trace, GE5_FAKE_LOCATIONS: locations};
  for (const key of ['BLUEICE_GC3_REPORT_DIRECTORY', 'BLUEICE_GE5_ROLE_REPORT_DIRECTORY', 'BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY',
    'BLUEICE_GE5_ADJACENT_OBSERVATION_DIRECTORY']) environment[key] = path.join(directory, key);
  for (const key of ['BLUEICE_GE5_NODE_ROLE_REPORT', 'BLUEICE_GE5_ASYNC_ROLE_REPORT',
    'BLUEICE_GE5_CONTINUATION_ROLE_REPORT', 'BLUEICE_GE5_NATIVE_FIX_REPORT', 'BLUEICE_GE5_SYSTEM_PHASE_REPORT',
    'BLUEICE_GE5_REPORT']) environment[key] = path.join(directory, key + '.json');
  const rows = file => fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).map(JSON.parse);
  return {directory, output, binaries, inventory, report, environment, compiler,
    events: () => rows(trace), locations: () => rows(locations),
    run: jobs => runSuites({binaries, inventory, output, root: directory, report,
      save: () => fs.writeFileSync(path.join(output, 'measurement.json'), JSON.stringify(report)),
      checkCompiler: guard.check, jobs, environment}),
    cleanup: () => fs.rmSync(directory, {recursive: true, force: true})};
}

test('bounded real children finish out of order but retain every catalog index and independent artifacts', async () => {
  const f = scheduleFixture([{delay: 600}, {delay: 30}, {delay: 70}, {delay: 30}]);
  try {
    await f.run(2);
    let active = 0, maximum = 0;
    const runtime = f.events().filter(e => e.kind !== 'list');
    for (const e of runtime) { active += e.kind === 'start' ? 1 : -1; maximum = Math.max(maximum, active); }
    assert.equal(active, 0); assert.equal(maximum, 2);
    assert.equal(runtime.find(e => e.kind === 'end').index, 1);
    assert.deepEqual(f.report.suites.map(s => s.harness), f.inventory.suites.map(s => s.harness));
    assert(f.report.suites.every(s => s.conclusion === 'success' && s.passed === 2 && !s.failed && !s.ignored && !s.filtered));
    assert(runtime.filter(e => e.kind === 'start').every(e => JSON.stringify(e.args) === '["--include-ignored"]'));
    for (const [index, row] of f.report.suites.entries()) {
      assert.deepEqual(row.test_names, ['first', 'ignored']); assert.deepEqual(row.pinned_tests, ['ignored']);
      assert.equal(row.log_sha256, crypto.createHash('sha256').update(fs.readFileSync(path.join(f.output, row.log))).digest('hex'));
      for (const [key, directory] of Object.entries(row.artifact_paths)) {
        assert.notEqual(directory, f.environment[key]);
        const file = key.endsWith('_DIRECTORY') ? path.join(directory, 'shared.json') : directory;
        assert.deepEqual(JSON.parse(fs.readFileSync(file)), {index});
      }
    }
    assert.equal(new Set(f.locations().map(row => row.locations.BLUEICE_GE5_ROLE_REPORT_DIRECTORY)).size, 4);
    assert.equal(new Set(f.locations().map(row => row.locations.BLUEICE_GC3_REPORT_DIRECTORY)).size, 4);
  } finally { f.cleanup(); }
});

test('default single worker preserves configured artifact paths and exact suite order', async () => {
  const f = scheduleFixture([{}, {}]);
  try {
    await f.run();
    assert.deepEqual(f.events().filter(e => e.kind !== 'list').map(e => [e.kind, e.index]),
      [['start', 0], ['end', 0], ['start', 1], ['end', 1]]);
    for (const row of f.locations()) for (const [key, value] of Object.entries(row.locations)) assert.equal(value, f.environment[key]);
  } finally { f.cleanup(); }
});

test('three and four workers also keep the configured live-child bound and complete inventory', async () => {
  for (const jobs of [3, 4]) {
    const f = scheduleFixture(Array.from({length: jobs + 1}, () => ({delay: 400})));
    try {
      await f.run(jobs);
      let active = 0, maximum = 0;
      for (const event of f.events().filter(e => e.kind !== 'list')) {
        active += event.kind === 'start' ? 1 : -1; maximum = Math.max(maximum, active);
        assert(active >= 0 && active <= jobs);
      }
      assert.equal(active, 0); assert(maximum > 1 && maximum <= jobs);
      assert.deepEqual(f.report.suites.map(row => row.harness), f.inventory.suites.map(row => row.harness));
      assert(f.report.suites.every(row => row.conclusion === 'success'));
    } finally { f.cleanup(); }
  }
});

test('failure stops new work, drains an active child, and retains both logs in catalog order', async () => {
  const f = scheduleFixture([{delay: 200, fail: true}, {delay: 600}, {}, {}]);
  try {
    await assert.rejects(f.run(2), /test process exit/);
    assert.deepEqual(f.events().filter(e => e.kind === 'start').map(e => e.index), [0, 1]);
    assert.deepEqual(f.events().filter(e => e.kind === 'end').map(e => e.index).sort(), [0, 1]);
    assert.equal(f.report.conclusion, 'failure'); assert.equal(f.report.suites.length, 2);
    assert.deepEqual(f.report.suites.map(s => s.conclusion), ['failure', 'success']);
    for (const row of f.report.suites) assert.match(row.log_sha256, /^[a-f0-9]{64}$/);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(f.output, 'current.json'))).active, []);
  } finally { f.cleanup(); }
});

test('a zero-exit partial libtest run cannot accept or schedule the remaining suites', async () => {
  const f = scheduleFixture([{delay: 200, partial: true}, {delay: 600}, {}]);
  try {
    await assert.rejects(f.run(2), /incomplete or failed libtest run/);
    assert.equal(f.report.conclusion, 'failure'); assert.equal(f.report.suites.length, 2);
    assert.equal(f.events().filter(e => e.kind === 'end').length, 2);
  } finally { f.cleanup(); }
});

test('compiled ignored-test drift and an early CLI mismatch launch no test children', async () => {
  for (const type of ['ignored', 'compiler']) {
    const f = scheduleFixture([{ignored: type === 'ignored' ? 'different' : undefined}, {}]);
    try {
      if (type === 'compiler') fs.writeFileSync(f.compiler, 'unexpected CLI');
      await assert.rejects(f.run(2), type === 'compiler' ? /compiler SHA-256 changed/ : /ignored-test set differs/);
      assert.equal(f.events().filter(e => e.kind === 'start').length, 0);
      if (type === 'compiler') assert.equal(f.events().length, 0, 'reject before even listing a test binary');
      assert.equal(f.report.conclusion, 'failure');
    } finally { f.cleanup(); }
  }
});

test('later CLI mutation rejects both affected active suites and never rebinds or launches a third', async () => {
  const f = scheduleFixture([{delay: 200, mutate: true}, {delay: 600}, {}]);
  try {
    await assert.rejects(f.run(2), /compiler SHA-256 changed/);
    assert.deepEqual(f.events().filter(e => e.kind === 'start').map(e => e.index), [0, 1]);
    assert.equal(f.events().filter(e => e.kind === 'end').length, 2);
    assert.deepEqual(f.report.suites.map(s => s.conclusion), ['failure', 'failure']);
    assert.equal(f.report.conclusion, 'failure');
  } finally { f.cleanup(); }
});
