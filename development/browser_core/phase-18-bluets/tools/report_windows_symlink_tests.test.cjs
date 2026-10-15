// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');
const { unixOnlyTests, skippedSymlinkCases, renderReport } =
  require('./report_windows_symlink_tests.cjs');

test('discovers Unix-only tests in either attribute order', () => {
  const source = [
    '#[cfg(unix)]',
    '#[test]',
    'fn first() {}',
    '#[test]',
    '#[cfg(unix)]',
    'fn second() {}',
    '#[cfg(unix)]',
    '#[test]',
    '#[ignore = "pinned oracle"]',
    'fn ignored() {}',
    '#[test]',
    'fn portable() {}',
  ].join('\n');
  assert.deepEqual(
    unixOnlyTests(source, 'backend/bluets/tests/example.rs').map((entry) => entry.name),
    ['example::first', 'example::second', 'example::ignored'],
  );
});

test('the Windows report names every currently Unix-gated symlink case and why it is skipped', () => {
  const cases = skippedSymlinkCases();
  assert.equal(cases.length, 25);
  assert.equal(new Set(cases.map(({ name }) => name)).size, cases.length);
  assert(cases.some(({ name }) => name ===
    'project_build_invalidation::repointed-symlink pinned oracle row'));
  assert(cases.some(({ name }) => name ===
    'project_reference_owner_gate::pinned_build_records_owner_gate_difference'));
  const report = renderReport(cases);
  for (const { name, source, reason } of cases) {
    assert(report.includes(`\`${name}\` | SKIPPED | ${reason} Source: \`${source}\``));
  }
});

test('prints reasons and appends the same report to the CI job summary', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-symlink-report-'));
  const summary = path.join(directory, 'summary.md');
  try {
    const result = spawnSync(process.execPath, [
      path.join(__dirname, 'report_windows_symlink_tests.cjs'),
    ], {
      env: { ...process.env, GITHUB_STEP_SUMMARY: summary },
      encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    assert(result.stdout.includes('project_reference_owner_gate::pinned_build_records_owner_gate_difference'));
    assert(result.stdout.includes('SKIPPED | Excluded from the Windows test binary'));
    assert.equal(fs.readFileSync(summary, 'utf8'), result.stdout);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
