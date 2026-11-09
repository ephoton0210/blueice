// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');

const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function files(root) {
  if (!fs.existsSync(root)) return [];
  return fs.readdirSync(root, {withFileTypes: true}).flatMap(entry => {
    const name = path.join(root, entry.name);
    return entry.isDirectory() ? files(name) : [name];
  }).sort();
}
function inventory(root, predicate) {
  return Object.fromEntries(files(root).filter(predicate).map(name =>
    [path.relative(root, name).replaceAll(path.sep, '/'), hash(fs.readFileSync(name))]));
}
function declarations(a, b) {
  const native = inventory(a, name => name.endsWith('.d.ts'));
  const bluets = inventory(b, name => name.endsWith('.d.ts'));
  return {native, bluets, equal: Object.keys(native).length > 0 && JSON.stringify(native) === JSON.stringify(bluets)};
}
function compatible(native, bluets, declaration) {
  return declaration.equal && [native, bluets].every(result => result.cold.exit === 0 &&
    result.runtime?.passed === 8 && result.positiveConsumer?.exit === 0 &&
    result.negativeConsumer?.expectedRejection === true && result.incremental?.passed === true);
}
function unpackFixture(destination, manifest) {
  const archive = path.join(__dirname, manifest.archive);
  assert.equal(hash(fs.readFileSync(archive)), manifest.archiveSha256, 'upstream archive hash mismatch');
  const listing = cp.execFileSync('tar', ['-tzf', archive], {encoding: 'utf8'}).trim().split(/\r?\n/).sort();
  assert.deepEqual(listing, Object.keys(manifest.files).sort(), 'unexpected archive members');
  assert(listing.every(name => !path.isAbsolute(name) && !name.split('/').includes('..')));
  cp.execFileSync('tar', ['-xzf', archive, '-C', destination]);
  for (const [name, expected] of Object.entries(manifest.files)) {
    assert.equal(hash(fs.readFileSync(path.join(destination, name))), expected, 'upstream file hash mismatch: ' + name);
  }
}
function sourceSnapshot(repo, sourceTree) {
  const git = args => cp.execFileSync('git', args, {cwd: repo, encoding: 'utf8'}).trim();
  const tree = sourceTree || git(['rev-parse', 'HEAD:backend']);
  assert(/^[a-f0-9]{40}$/.test(tree) && git(['cat-file', '-t', tree]) === 'tree', 'expected an immutable backend tree');
  const entries = cp.execFileSync('git', ['ls-tree', '-r', '-z', tree], {cwd: repo, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024}).split('\0').filter(Boolean);
  const expectedPaths = [];
  for (const entry of entries) {
    const match = /^(\d+) blob ([a-f0-9]{40})\t(.+)$/.exec(entry);
    assert(match, 'unsupported source tree entry');
    const [, mode, digest, relative] = match, name = 'backend/' + relative;
    const data = mode === '120000' ? Buffer.from(fs.readlinkSync(path.join(repo, name))) : fs.readFileSync(path.join(repo, name));
    const actual = crypto.createHash('sha1').update('blob ' + data.length + '\0').update(data).digest('hex');
    assert.equal(actual, digest, 'compiler source differs from frozen tree: ' + name);
    expectedPaths.push(name);
  }
  const tracked = cp.execFileSync('git', ['ls-files', '-z', '--', 'backend'], {cwd: repo, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024}).split('\0').filter(Boolean).sort();
  assert.deepEqual(tracked, expectedPaths.sort(), 'tracked compiler source selection differs from frozen tree');
  assert.equal(git(['ls-files', '--others', '--exclude-standard', '--', 'backend']), '', 'untracked compiler source');
  return {sourceRevision: 'backend-tree:' + tree, baseCommit: git(['rev-parse', 'HEAD']), verifiedFiles: entries.length,
    mode: sourceTree ? 'explicit-immutable-backend-tree' : 'committed-backend-tree'};
}
function audit({compiler, typescript, output, sourceTree}) {
  compiler = fs.realpathSync(compiler); typescript = fs.realpathSync(typescript);
  output = path.resolve(output);
  // Exclusive creation keeps stale outputs from a previous attempt out of this result.
  fs.mkdirSync(output);
  const repo = path.resolve(__dirname, '../..');
  const source = sourceSnapshot(repo, sourceTree);
  const tsLibrary = path.resolve(path.dirname(typescript), '../lib/typescript.js');
  const ts = require(tsLibrary);
  assert.equal(ts.version, '5.9.3', 'the reference compiler must be pinned TypeScript 5.9.3');
  const manifest = require('./mitt-3.0.1.json');
  const report = {schema: 1, scope: 'Complete unchanged mitt 3.0.1 source under one frozen CommonJS/ES2022 profile; eight authored runtime scenarios, not the upstream full test suite or the global compatibility denominator.',
    startedAt: new Date().toISOString(), project: manifest, sourceRevision: source.sourceRevision,
    sourceSnapshot: source, node: process.version, compilerSha256: hash(fs.readFileSync(compiler)),
    typescript: {version: ts.version, executableSha256: hash(fs.readFileSync(typescript)), implementationSha256: hash(fs.readFileSync(tsLibrary)),
      libraries: inventory(path.dirname(tsLibrary), name => /[/\\]lib\..*\.d\.ts$/.test(name))},
    tools: inventory(__dirname, name => name.endsWith('.cjs')), commands: []};
  const save = () => fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  function run(label, program, args, cwd) {
    const result = cp.spawnSync(program, args, {cwd, encoding: 'utf8', timeout: 240000, maxBuffer: 16 * 1024 * 1024,
      env: {...process.env, FORCE_COLOR: '0'}});
    const record = {label, command: [program, ...args], cwd, exit: result.status,
      stdout: label + '.stdout', stderr: label + '.stderr', error: result.error?.message};
    fs.writeFileSync(path.join(output, record.stdout), result.stdout || '');
    fs.writeFileSync(path.join(output, record.stderr), result.stderr || '');
    record.stdoutSha256 = hash(result.stdout || ''); record.stderrSha256 = hash(result.stderr || '');
    report.commands.push(record); save();
    if (result.error || result.status === null) throw result.error || Error('process terminated: ' + label);
    return record;
  }
  const artifacts = directory => inventory(path.join(directory, 'dist'), name => !name.endsWith('.tsbuildinfo'));
  function probe(name, program, buildArgs) {
    const directory = path.join(output, name); fs.mkdirSync(directory); unpackFixture(directory, manifest);
    fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify(manifest.profile, null, 2) + '\n');
    const result = {cold: run(name + '-cold', program, buildArgs, directory)};
    if (result.cold.exit !== 0) {
      result.downstream = 'Blocked by the cold compilation failure: no declaration, runtime, consumer or incremental success is credited.';
      return result;
    }
    const dts = path.join(directory, 'dist/index.d.ts');
    assert(fs.existsSync(dts), name + ' succeeded without publishing its declaration');
    fs.cpSync(path.join(directory, 'dist'), path.join(output, name + '-cold-artifacts'), {recursive: true});
    result.coldArtifacts = artifacts(directory);
    const smoke = run(name + '-runtime', process.execPath, [path.join(__dirname, 'smoke.cjs'), path.join(directory, 'dist/index.js')], directory);
    const smokeData = smoke.exit === 0 ? JSON.parse(fs.readFileSync(path.join(output, smoke.stdout), 'utf8')) : null;
    result.runtime = {exit: smoke.exit, passed: smokeData?.count || 0, scenarios: smokeData?.passed || []};
    const positive = "import mitt from './dist/index';\nconst emitter = mitt<{count: number; ready: undefined}>();\nemitter.on('count', value => { const count: number = value; });\nemitter.emit('count', 42); emitter.emit('ready');\n";
    const negative = "import mitt from './dist/index';\nconst emitter = mitt<{count: number}>();\nemitter.emit('count', 'wrong');\n";
    const consumerArgs = ['--noEmit', '--strict', '--target', 'ES2022', '--module', 'CommonJS', '--lib', 'ES2022', '--pretty', 'false'];
    for (const [kind, source] of [['positive', positive], ['negative', negative]]) {
      fs.writeFileSync(path.join(directory, kind + '.ts'), source);
      const record = run(name + '-' + kind + '-consumer', process.execPath, [typescript, kind + '.ts', ...consumerArgs], directory);
      if (kind === 'negative') record.expectedRejection = record.exit !== 0 && /error TS2345:/.test(fs.readFileSync(path.join(output, record.stdout), 'utf8'));
      result[kind + 'Consumer'] = record;
    }
    const cold = artifacts(directory);
    fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify({...manifest.profile,
      compilerOptions: {...manifest.profile.compilerOptions, ...manifest.incrementalOptions}}, null, 2) + '\n');
    const incremental = run(name + '-incremental-initial', program, buildArgs, directory);
    if (incremental.exit !== 0) {
      result.incremental = {passed: false, initialExit: incremental.exit, scope: 'The explicit incremental configuration is refused; no warm/edit/clean agreement is credited.'};
      return result;
    }
    const warm = run(name + '-warm', program, buildArgs, directory);
    const warmUnchanged = JSON.stringify(cold) === JSON.stringify(artifacts(directory));
    fs.appendFileSync(path.join(directory, 'src/index.ts'), '\n// BlueIce incremental edit probe.\n');
    const edited = run(name + '-edited', program, buildArgs, directory), changed = artifacts(directory);
    const clean = path.join(output, name + '-clean'); fs.mkdirSync(clean); unpackFixture(clean, manifest);
    fs.copyFileSync(path.join(directory, 'src/index.ts'), path.join(clean, 'src/index.ts'));
    fs.copyFileSync(path.join(directory, 'tsconfig.json'), path.join(clean, 'tsconfig.json'));
    const rebuilt = run(name + '-clean-rebuild', program, buildArgs, clean);
    const editedSmoke = run(name + '-edited-runtime', process.execPath, [path.join(__dirname, 'smoke.cjs'), path.join(directory, 'dist/index.js')], directory);
    result.incremental = {warmExit: warm.exit, editedExit: edited.exit, cleanExit: rebuilt.exit, editedRuntimeExit: editedSmoke.exit,
      warmUnchanged, editedChanged: JSON.stringify(cold) !== JSON.stringify(changed),
      matchesClean: JSON.stringify(changed) === JSON.stringify(artifacts(clean))};
    result.incremental.passed = [warm, edited, rebuilt, editedSmoke].every(r => r.exit === 0) &&
      result.incremental.warmUnchanged && result.incremental.editedChanged && result.incremental.matchesClean;
    return result;
  }
  try {
    save();
    report.native = probe('native', process.execPath, [typescript, '-p', 'tsconfig.json', '--pretty', 'false']);
    assert.equal(report.native.cold.exit, 0, 'invalid Native baseline');
    assert(compatible(report.native, report.native, {equal: true}), 'invalid Native runtime/consumer/rebuild baseline');
    report.bluets = probe('bluets', compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json']);
    report.declarations = declarations(path.join(output, 'native-cold-artifacts'), path.join(output, 'bluets-cold-artifacts'));
    report.reproductions = [];
    const reproductions = path.join(output, 'reproductions'); fs.mkdirSync(reproductions);
    for (const [name, expectation] of Object.entries(manifest.reproductions)) {
      const source = fs.readFileSync(path.join(__dirname, name));
      assert.equal(hash(source), expectation.sha256, 'reproduction source changed: ' + name);
      const entry = path.basename(name); fs.writeFileSync(path.join(reproductions, entry), source);
      const native = run(entry + '-native', process.execPath,
        [typescript, entry, '--noEmit', '--strict', '--target', 'ES2022', '--module', 'CommonJS', '--lib', 'ES2022', '--pretty', 'false'], reproductions);
      assert.equal(native.exit === 0, expectation.nativeExpectedAccept, 'reproduction Native expectation changed');
      const bluets = run(entry + '-bluets', compiler,
        ['check', entry, '--project-root', reproductions, '--target', 'es2022', '--module', 'commonjs', '--diagnostics-json'], reproductions);
      report.reproductions.push({source: name, sourceSha256: expectation.sha256, nativeExit: native.exit, bluetsExit: bluets.exit,
        nativeAccepted: native.exit === 0, bluetsAccepted: bluets.exit === 0});
    }
    const config = {compilerOptions: {...manifest.profile.compilerOptions, rootDir: '.', outDir: 'config-dist'}, files: ['incremental-config-control.ts']};
    const configFile = path.join(reproductions, 'tsconfig.json');
    fs.writeFileSync(configFile, JSON.stringify(config, null, 2) + '\n');
    const configNative = run('config-control-native', process.execPath, [typescript, '-p', 'tsconfig.json', '--pretty', 'false'], reproductions);
    const configBluets = run('config-control-bluets', compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], reproductions);
    assert(configNative.exit === 0 && configBluets.exit === 0, 'invalid ordinary configuration control');
    fs.writeFileSync(configFile, JSON.stringify({...config, compilerOptions: {...config.compilerOptions, ...manifest.incrementalOptions}}, null, 2) + '\n');
    const incrementalNative = run('incremental-config-native', process.execPath, [typescript, '-p', 'tsconfig.json', '--pretty', 'false'], reproductions);
    const incrementalBluets = run('incremental-config-bluets', compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], reproductions);
    assert.equal(incrementalNative.exit, 0, 'invalid Native incremental configuration control');
    report.incrementalConfiguration = {control: {nativeExit: configNative.exit, bluetsExit: configBluets.exit},
      enabled: {nativeExit: incrementalNative.exit, bluetsExit: incrementalBluets.exit}};
    assert.equal(hash(fs.readFileSync(compiler)), report.compilerSha256, 'compiler changed during project audit');
    assert.equal(hash(fs.readFileSync(tsLibrary)), report.typescript.implementationSha256, 'TypeScript changed during project audit');
    assert.equal(sourceSnapshot(repo, sourceTree).sourceRevision, report.sourceRevision);
    report.compatible = compatible(report.native, report.bluets, report.declarations) &&
      report.reproductions.every(row => row.bluetsAccepted) && incrementalBluets.exit === 0;
    report.conclusion = report.compatible ? 'compatible-with-frozen-profile' : 'blocked';
    report.finishedAt = new Date().toISOString(); save();
    return report;
  } catch (error) {
    report.conclusion = 'audit-error'; report.error = error.stack; report.finishedAt = new Date().toISOString(); save(); throw error;
  }
}

if (require.main === module) {
  try {
    const args = process.argv.slice(2);
    assert([6, 8].includes(args.length) && args[0] === '--compiler' && args[2] === '--typescript' && args[4] === '--output' &&
      (args.length === 6 || args[6] === '--source-tree'),
      'usage: node audit.cjs --compiler <bluetsc> --typescript <typescript/bin/tsc> --output <new-directory> [--source-tree <backend-tree-sha>]');
    const report = audit({compiler: args[1], typescript: args[3], output: args[5], sourceTree: args[7]});
    console.log(JSON.stringify({conclusion: report.conclusion, compatible: report.compatible, output: path.resolve(args[5])}));
    process.exitCode = report.compatible ? 0 : 2;
  } catch (error) { console.error(error); process.exitCode = 1; }
}
module.exports = {declarations, compatible, unpackFixture, audit};
