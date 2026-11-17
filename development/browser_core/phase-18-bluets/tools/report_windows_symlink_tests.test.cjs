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

test('discovers the explicit Windows symlink feature without treating it as a pass', () => {
  const source = [
    '#[cfg(any(unix, all(windows, feature = "windows-symlink-tests")))]',
    '#[test]',
    'fn portable_symlink_boundary() {}',
  ].join('\n');
  assert.deepEqual(
    unixOnlyTests(source, 'backend/bluets/tests/example.rs').map(entry => entry.name),
    ['example::portable_symlink_boundary'],
  );
});

test('the Windows report names every currently Unix-gated symlink case and why it is skipped', () => {
  const cases = skippedSymlinkCases();
  assert.equal(cases.length, 28);
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

test('keeps the frozen Windows matrix separate from additional Unix-only boundaries', () => {
  const frozen = JSON.parse(fs.readFileSync(path.join(__dirname, '../GM3_FROZEN_MATRIX.json'), 'utf8'));
  const cases = skippedSymlinkCases();
  const frozenNames = new Set(frozen.cases.map(({ name }) => name));
  assert.equal(frozenNames.size, 25);
  assert.deepEqual(new Set(cases.filter(({ name }) => frozenNames.has(name)).map(({ name }) => name)), frozenNames);
  const added = cases.filter(({ name }) => !frozenNames.has(name));
  assert.deepEqual(added.map(({ name, source }) => ({ name, source })), [
    {
      name: 'gm2_incremental_options::state_and_parent_symlinks_cannot_publish_outside_owner',
      source: 'backend/bluets/tests/gm2_incremental_options.rs',
    },
    {
      name: 'program_root_publication::symlinked_retirement_targets_refuse_without_following_or_removing_them',
      source: 'backend/bluets/tests/program_root_publication.rs',
    },
    {
      name: 'program_roots_private_state::a_private_state_symlink_never_reaches_an_outside_directory',
      source: 'backend/bluets/tests/program_roots_private_state.rs',
    },
  ]);
  for (const entry of added) {
    assert(entry.reason.includes('Unix-only symlink APIs'));
    assert(!entry.reason.includes('windows-symlink-tests'));
  }
  assert(renderReport(cases).includes('Rows using Unix-only APIs remain unavailable on Windows.'));
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
    assert(result.stdout.includes('SKIPPED | Excluded from the default Windows test binary'));
    assert(result.stdout.includes('windows-symlink-tests'));
    assert(result.stdout.includes('This report is not an execution result.'));
    assert.equal(fs.readFileSync(summary, 'utf8'), result.stdout);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
