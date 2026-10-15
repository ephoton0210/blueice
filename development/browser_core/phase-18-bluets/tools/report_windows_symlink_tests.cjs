// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Make Unix-gated symlink coverage visible in Windows CI instead of silently
// omitting those test functions from the Windows test binaries.

const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '../../../..');
const ATTRIBUTE_BLOCK = /((?:^[ \t]*#\[[^\r\n]*\][ \t]*\r?\n)+)[ \t]*fn[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*\(/gm;
const UNIX_ONLY_REASON = 'Excluded from the Windows test binary by `#[cfg(unix)]`; the fixture uses Unix-only symlink APIs.';
const MATRIX_REASON = 'The pinned matrix filters this symlink case behind `cfg!(unix)` on Windows.';

function unixOnlyTests(source, relativePath) {
  const tests = [];
  for (const match of source.matchAll(ATTRIBUTE_BLOCK)) {
    if (match[1].includes('#[cfg(unix)]') && match[1].includes('#[test]')) {
      tests.push({ name: `${path.basename(relativePath, '.rs')}::${match[2]}`, source: relativePath });
    }
  }
  return tests;
}

function rustTestFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) return rustTestFiles(fullPath);
    return entry.isFile() && entry.name.endsWith('.rs') ? [fullPath] : [];
  });
}

function skippedSymlinkCases(root = ROOT) {
  const testRoots = ['backend/bluets/tests', 'backend/bluets-bluejs/tests'];
  const tests = testRoots.flatMap((relativeRoot) => {
    const directory = path.join(root, relativeRoot);
    return rustTestFiles(directory).flatMap((fullPath) => {
      const relativePath = path.relative(root, fullPath).replaceAll(path.sep, '/');
      return unixOnlyTests(fs.readFileSync(fullPath, 'utf8'), relativePath)
        .map((test) => ({ ...test, reason: UNIX_ONLY_REASON }));
    });
  });
  const matrixPath = 'backend/bluets/tests/project_build_invalidation.rs';
  const matrix = fs.readFileSync(path.join(root, matrixPath), 'utf8');
  if (!matrix.includes('cfg!(unix) || **case != "repointed-symlink"')) {
    throw new Error('The repointed-symlink matrix filter changed; review the Windows report.');
  }
  tests.push({
    name: 'project_build_invalidation::repointed-symlink pinned oracle row',
    source: matrixPath,
    reason: MATRIX_REASON,
  });
  if (tests.length < 20) throw new Error(`Expected the Unix-gated symlink inventory, found ${tests.length}.`);
  return tests.sort((left, right) => left.name.localeCompare(right.name));
}

function renderReport(tests) {
  const rows = tests.map(({ name, source, reason }) =>
    `| \`${name}\` | SKIPPED | ${reason} Source: \`${source}\`. |`);
  return [
    '### Windows symlink test report',
    '',
    'These cases are omitted by existing Unix-only test fixtures, not counted as Windows passes.',
    'The Windows workspace suite runs all other compiled tests; each omitted case is listed below.',
    '',
    '| Test or oracle row | Status | Printed reason |',
    '| --- | --- | --- |',
    ...rows,
    '',
  ].join('\n');
}

function main() {
  const report = renderReport(skippedSymlinkCases());
  process.stdout.write(`${report}\n`);
  if (process.env.GITHUB_STEP_SUMMARY) {
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${report}\n`);
  }
}

if (require.main === module) main();
module.exports = { unixOnlyTests, skippedSymlinkCases, renderReport };
