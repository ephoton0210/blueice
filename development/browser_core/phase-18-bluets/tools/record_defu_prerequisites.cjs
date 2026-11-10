// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const cp = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
assert.equal(require(implementation).version, '5.9.3');
const tsc = path.resolve(path.dirname(implementation), '../bin/tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/defu_prerequisites');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
const original = require(path.join(root, 'differential-testing/bluets-projects/defu-reproductions.native.json'));
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-defu-native-'));
const capture = process.env.BLUEICE_DEFU_NATIVE_OBSERVATIONS || (process.platform === 'win32' ?
  fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-defu-native-observations-')) : undefined);
try {
  const cases = input.cases.map((entry, index) => {
    const directory = path.join(temporary, entry.id); fs.mkdirSync(path.join(directory, 'src'), {recursive: true});
    fs.writeFileSync(path.join(directory, 'src/main.ts'), entry.source);
    const {id, ...config} = input.profile;
    fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify(config, null, 2) + '\n');
    const result = cp.spawnSync(process.execPath, [tsc, '-p', 'tsconfig.json', '--pretty', 'false'],
      {cwd: directory, encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024});
    if (result.error || result.status === null) throw result.error || Error('terminated Native');
    assert.equal(result.status === 0, entry.nativeExpectedAccept, entry.id);
    const dist = path.join(directory, 'dist');
    const native = {exit: result.status, stdout: result.stdout, stderr: result.stderr,
      declarations: fs.existsSync(dist) ? Object.fromEntries(fs.readdirSync(dist).filter(name => name.endsWith('.d.ts')).sort()
        .map(name => [name, fs.readFileSync(path.join(dist, name), 'utf8')])) : {}};
    if (entry.runtimeProbe) {
      assert.equal(native.exit, 0, 'runtime observation requires successful Native emit');
      const execution = cp.spawnSync(process.execPath, ['-e', entry.runtimeProbe],
        {cwd: directory, encoding: 'utf8', timeout: 120000});
      assert.equal(execution.status, 0, execution.stderr);
      native.runtime = {exit: execution.status, stdout: execution.stdout, stderr: execution.stderr};
    }
    if (capture) {
      const raw = path.join(capture, entry.id); fs.mkdirSync(raw, {recursive: true});
      fs.writeFileSync(path.join(raw, 'observation.json'), JSON.stringify(native, null, 2) + '\n');
      fs.writeFileSync(path.join(raw, 'stdout.log'), native.stdout);
      fs.writeFileSync(path.join(raw, 'stderr.log'), native.stderr);
    }
    // Diagnostic console terminators follow the host OS. Keep their raw bytes
    // above; compare messages using LF. Declaration and runtime bytes retain
    // their exact original comparison, including every line terminator.
    native.stdout = native.stdout.replace(/\r\n/g, '\n');
    native.stderr = native.stderr.replace(/\r\n/g, '\n');
    if (index < original.cases.length) {
      assert.equal(entry.id, original.cases[index].id);
      assert.deepEqual(native, original.cases[index].native, 'original Native controls changed');
    }
    return {...entry, native};
  });
  const value = {schema: 1, typescript: '5.9.3', scope: input.scope, profile: input.profile, cases};
  const text = JSON.stringify(value, null, 2) + '\n', file = path.join(directory, 'reference.json');
  if (process.env.BLUEICE_WRITE_DEFU_PREREQUISITES === '1') fs.writeFileSync(file, text);
  else assert.equal(fs.readFileSync(file, 'utf8'), text, 'changed Native prerequisite observations');
  console.log(`Verified ${cases.length} Native prerequisite controls; original eight observations unchanged`);
  if (capture) console.log(`Raw Native diagnostic observations: ${path.resolve(capture)}`);
} finally { fs.rmSync(temporary, {recursive: true, force: true}); }
