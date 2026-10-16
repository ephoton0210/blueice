// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// K.13.1.3 slice S2: split the pinned-oracle `cargo test -- --ignored` run into
// shards so a Windows-specific failure is isolated and re-run in minutes
// instead of the whole multi-hour suite. Every integration-test file belongs to
// exactly one shard by rule (never by a hand-kept list), so a new test file is
// picked up automatically and cannot be silently skipped.
//
//   node oracle_shards.cjs --list            print shard names
//   node oracle_shards.cjs --plan <shard>    print the cargo commands
//   node oracle_shards.cjs <shard>           run them, run all, fail if any failed
'use strict';
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const SHARDS = ['core', 'checker', 'project', 'other'];
const PACKAGES = [
  { name: 'blueice-bluets', dir: 'backend/bluets/tests' },
  { name: 'blueice-bluets-bluejs', dir: 'backend/bluets-bluejs/tests' },
];

function shardOf(packageName, file) {
  if (packageName === 'blueice-bluets') {
    if (file === 'typescript_oracle.rs') return 'core';
    if (file.endsWith('_checker_matrix.rs')) return 'checker';
    if (file.startsWith('project_')) return 'project';
  }
  return 'other';
}

function testFiles(root, dir) {
  return fs
    .readdirSync(path.join(root, dir), { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.rs'))
    .map((entry) => entry.name)
    .sort();
}

// Map of shard -> [{ package, tests: [name...] }] for the repository at `root`.
function plan(root) {
  const result = Object.fromEntries(SHARDS.map((shard) => [shard, []]));
  for (const pkg of PACKAGES) {
    const byShard = Object.fromEntries(SHARDS.map((shard) => [shard, []]));
    for (const file of testFiles(root, pkg.dir)) {
      byShard[shardOf(pkg.name, file)].push(file.replace(/\.rs$/, ''));
    }
    for (const shard of SHARDS) {
      if (byShard[shard].length) result[shard].push({ package: pkg.name, tests: byShard[shard] });
    }
  }
  return result;
}

function commands(root, shard) {
  return plan(root)[shard].map(({ package: name, tests }) => [
    'cargo',
    'test',
    '-p',
    name,
    ...tests.flatMap((test) => ['--test', test]),
    '--no-fail-fast',
    '--',
    '--ignored',
  ]);
}

function main(argv) {
  const root = path.resolve(__dirname, '../../../..');
  if (argv[0] === '--list') return console.log(SHARDS.join('\n'));
  const planOnly = argv[0] === '--plan';
  const shard = planOnly ? argv[1] : argv[0];
  if (!SHARDS.includes(shard)) {
    console.error(`unknown shard ${JSON.stringify(shard)}; expected one of ${SHARDS.join(', ')}`);
    process.exit(2);
  }
  let failed = 0;
  for (const command of commands(root, shard)) {
    console.log(`::group::${command.join(' ')}`);
    if (planOnly) {
      console.log('::endgroup::');
      continue;
    }
    const started = Date.now();
    const status = spawnSync(command[0], command.slice(1), { cwd: root, stdio: 'inherit' }).status;
    console.log('::endgroup::');
    console.log(`shard ${shard}: ${command[3]} exit ${status} in ${Math.round((Date.now() - started) / 1000)}s`);
    if (status !== 0) failed += 1;
  }
  process.exit(failed ? 1 : 0);
}

if (require.main === module) main(process.argv.slice(2));
module.exports = { SHARDS, shardOf, plan, commands };
