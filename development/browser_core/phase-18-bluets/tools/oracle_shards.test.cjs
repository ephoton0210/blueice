// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const { SHARDS, shardOf, plan, commands } = require('./oracle_shards.cjs');

const root = path.resolve(__dirname, '../../../..');

test('rules place files in the intended shard', () => {
  assert.equal(shardOf('blueice-bluets', 'typescript_oracle.rs'), 'core');
  assert.equal(shardOf('blueice-bluets', 'narrowing_checker_matrix.rs'), 'checker');
  assert.equal(shardOf('blueice-bluets', 'project_build_order.rs'), 'project');
  assert.equal(shardOf('blueice-bluets', 'module_systems.rs'), 'other');
  assert.equal(shardOf('blueice-bluets-bluejs', 'project_x.rs'), 'other');
});

test('every integration-test file is in exactly one shard', () => {
  const planned = plan(root);
  for (const [name, dir] of [
    ['blueice-bluets', 'backend/bluets/tests'],
    ['blueice-bluets-bluejs', 'backend/bluets-bluejs/tests'],
  ]) {
    const expected = fs
      .readdirSync(path.join(root, dir))
      .filter((file) => file.endsWith('.rs'))
      .map((file) => file.replace(/\.rs$/, ''))
      .sort();
    const seen = SHARDS.flatMap((shard) =>
      planned[shard].filter((entry) => entry.package === name).flatMap((entry) => entry.tests),
    ).sort();
    assert.deepEqual(seen, expected);
    assert.equal(new Set(seen).size, seen.length, 'a test file appears in two shards');
  }
});

test('commands run only ignored oracles and keep going after a failure', () => {
  for (const shard of SHARDS) {
    for (const command of commands(root, shard)) {
      assert.ok(command.includes('--no-fail-fast'));
      assert.deepEqual(command.slice(-2), ['--', '--ignored']);
    }
  }
});
