// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync, execFileSync } = require('node:child_process');

test('container jobs trust only the checked-out workspace before inventory tests', () => {
  const root = path.resolve(__dirname, '../../../..');
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8')
    .replaceAll('\r\n', '\n');
  const step = "- name: Trust checkout for container Git\n        if: matrix.container != ''\n        run: git config --global --add safe.directory \"$GITHUB_WORKSPACE\"";
  assert(workflow.includes(step), 'container Git trust must persist beyond an action temporary HOME');
  assert(workflow.indexOf(step) > workflow.indexOf('- uses: actions/checkout@v4'));
  assert(workflow.indexOf(step) < workflow.indexOf('- name: Test\n'));
  assert(!/safe\.directory\s+["']?\*/.test(workflow), 'never disable ownership checks for all paths');
  assert(workflow.includes('node --test development/browser_core/phase-18-bluets/tools/ci_inventory_checkout.test.cjs'));
});

test('exact-path checkout trust permits that repository but still rejects an untrusted sibling', t => {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'blueice-ci-ownership-'));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const base = fs.realpathSync(temporary);
  const env = { ...process.env, GIT_CONFIG_GLOBAL: path.join(base, 'isolated.gitconfig'),
    GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_COUNT: '0', GIT_DEFAULT_HASH: 'sha1' };
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR',
    'GIT_TEST_ASSUME_DIFFERENT_OWNER']) delete env[key];
  const git = (args, options = {}) => spawnSync('git', args, {
    cwd: base, env, encoding: 'utf8', ...options
  });
  const success = result => {
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  const repositories = ['owned checkout $literal', 'untrusted sibling'].map(name => path.join(base, name));
  for (const repository of repositories) {
    success(git(['init', '--quiet', repository]));
    success(git(['-C', repository, '-c', 'user.name=CI fixture', '-c', 'user.email=ci@example.invalid',
      'commit', '--quiet', '--allow-empty', '-m', 'Create ownership fixture']));
  }
  const foreign = { ...env, GIT_TEST_ASSUME_DIFFERENT_OWNER: '1' };
  const read = repository => git(['-C', repository, 'rev-parse', '--verify', 'HEAD'], { env: foreign });
  const denied = result => {
    assert.ifError(result.error);
    assert.equal(result.status, 128, result.stderr);
    assert.match(result.stderr, /detected dubious ownership/);
  };
  repositories.forEach(repository => denied(read(repository)));
  const workspace = repositories[0].replaceAll('\\', '/');
  // The test uses its own global-config file, never the user's Git settings.
  success(git(['config', '--global', '--add', 'safe.directory', workspace], {
    cwd: repositories[0], env: foreign
  }));
  assert.match(success(read(repositories[0])), /^[a-f0-9]{40}$/);
  denied(read(repositories[1]));
  assert.equal(success(git(['config', '--global', '--get-all', 'safe.directory'])), workspace);
});

test('canonical inventory artifacts remain byte-identical with core.autocrlf=true', t => {
  const root = path.resolve(__dirname, '../../../..');
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'blueice-ci-checkout-'));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const base = fs.realpathSync(temporary);
  const repository = path.join(base, 'source');
  const checkout = path.join(base, 'checkout');
  fs.mkdirSync(repository);
  const env = { ...process.env, GIT_CONFIG_GLOBAL: path.join(base, 'isolated.gitconfig'),
    GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_COUNT: '0' };
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR',
    'GIT_TEST_ASSUME_DIFFERENT_OWNER']) delete env[key];
  const git = args => {
    const result = spawnSync('git', ['-c', 'core.autocrlf=true', ...args], {
      cwd: repository, env, encoding: 'utf8'
    });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
  };
  git(['init', '--quiet']);
  fs.copyFileSync(path.join(root, '.gitattributes'), path.join(repository, '.gitattributes'));
  const files = ['COMPATIBILITY_SUITES.json', 'COMPATIBILITY_CASES.json',
    'COMPATIBILITY_MEASUREMENT.json', 'COMPATIBILITY_INVENTORY.md']
    .map(file => 'development/browser_core/phase-18-bluets/' + file);
  const canonical = new Map();
  for (const file of files) {
    // Read the immutable blob, never a possibly CRLF-converted working copy.
    const bytes = execFileSync('git', ['show', 'HEAD:' + file], {
      cwd: root, maxBuffer: 32 * 1024 * 1024
    });
    canonical.set(file, bytes);
    fs.mkdirSync(path.dirname(path.join(repository, file)), { recursive: true });
    fs.writeFileSync(path.join(repository, file), bytes);
  }
  git(['add', '--', '.gitattributes', ...files]);
  git(['checkout-index', '--all', '--prefix=' + checkout.replaceAll('\\', '/') + '/']);
  for (const file of files) {
    assert(fs.readFileSync(path.join(checkout, file)).equals(canonical.get(file)),
      'checkout changed the canonical artifact bytes: ' + file);
  }
});
