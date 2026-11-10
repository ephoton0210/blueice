// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const {hash, inventory, sourceSnapshot, unpackFixture, declarations} = require('./audit.cjs');
const consumers = require('./defu-consumers.cjs');

function assess(native, bluets, declaration, manifest) {
  return declaration.equal && [native, bluets].every(result =>
    result.cold?.exit === 0 &&
    JSON.stringify(result.runtime?.passed) === JSON.stringify(manifest.runtimeScenarios) &&
    result.consumers?.positive === true && result.consumers?.negative === true &&
    result.consumers?.typeBefore === true && result.typeAfterRejected === true &&
    result.incremental?.passed === true);
}
function applyEdit(root, edit) {
  assert(!path.isAbsolute(edit.path) && !edit.path.split(/[\\/]/).includes('..'));
  const file = path.join(root, edit.path), source = fs.readFileSync(file, 'utf8');
  assert(edit.from && source.split(edit.from).length === 2, 'expected one frozen edit occurrence');
  fs.writeFileSync(file, source.replace(edit.from, edit.to));
}
function verifyReference(manifest, reference, native, declarationTexts, typescript) {
  assert.equal(reference.projectRevision, manifest.revision, 'foreign Native source');
  assert.equal(reference.archiveSha256, manifest.archiveSha256, 'foreign Native fixture');
  assert.deepEqual(reference.profile, manifest.profiles[0], 'changed Native profile');
  assert.deepEqual(reference.typescript, typescript, 'changed pinned compiler or libraries');
  assert.deepEqual(declarationTexts, reference.declarations, 'changed Native declaration bytes or inventory');
  const observableStages = value => value.stages.map(({stage, artifacts, inputs}) => ({stage, artifacts, inputs}));
  assert.deepEqual(observableStages(native), observableStages(reference.nativeAcceptance), 'changed Native edit observations');
  assert.equal(assess(native, native, {equal: true}, manifest), true, 'invalid Native acceptance');
}
function audit({compiler, typescript, output, nativeOnly = false}) {
  const manifest = require('./defu-v6.1.4.json');
  compiler = fs.realpathSync(compiler); typescript = fs.realpathSync(typescript);
  output = path.resolve(output); fs.mkdirSync(output);
  const repo = path.resolve(__dirname, '../..'), source = sourceSnapshot(repo);
  const tsLibrary = path.resolve(path.dirname(typescript), '../lib/typescript.js');
  assert.equal(require(tsLibrary).version, '5.9.3');
  const report = {schema: 1, startedAt: new Date().toISOString(), project: manifest,
    scope: 'Complete unchanged defu production source at one frozen CommonJS/ES2022 profile, authored consumers/runtime/edit probes; separate from the upstream test suite and global compiler case counts.',
    sourceRevision: source.sourceRevision, sourceSnapshot: source,
    compilerSha256: hash(fs.readFileSync(compiler)), node: process.version,
    typescript: {version: '5.9.3', executableSha256: hash(fs.readFileSync(typescript)),
      implementationSha256: hash(fs.readFileSync(tsLibrary)),
      libraries: inventory(path.dirname(tsLibrary), name => /[/\\]lib\..*\.d\.ts$/.test(name))},
    tools: inventory(__dirname, name => name.endsWith('.cjs')), commands: []};
  const save = () => fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  function run(label, program, args, cwd) {
    report.active = label; save();
    const result = cp.spawnSync(program, args, {cwd, encoding: 'utf8', timeout: 240000,
      maxBuffer: 16 * 1024 * 1024, env: {...process.env, FORCE_COLOR: '0'}});
    const record = {label, command: [program, ...args], cwd, exit: result.status,
      stdout: label + '.stdout', stderr: label + '.stderr', error: result.error?.message};
    for (const stream of ['stdout', 'stderr']) {
      fs.writeFileSync(path.join(output, record[stream]), result[stream] || '');
      record[stream + 'Sha256'] = hash(result[stream] || '');
    }
    report.commands.push(record); save();
    if (result.error || result.status === null) throw result.error || Error('terminated process: ' + label);
    return record;
  }
  const artifacts = directory => inventory(path.join(directory, 'dist'), name => !name.endsWith('.tsbuildinfo'));
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  function probe(name, program, buildArgs) {
    const directory = path.join(output, name); fs.mkdirSync(directory); unpackFixture(directory, manifest);
    const profile = manifest.profiles[0];
    const config = {compilerOptions: {...profile.compilerOptions}, include: profile.include};
    const configFile = path.join(directory, 'tsconfig.json');
    fs.writeFileSync(configFile, JSON.stringify(config, null, 2) + '\n');
    const result = {stages: []};
    function build(stage, required = true) {
      const command = run(name + '-' + stage, program, buildArgs, directory);
      const publicState = path.join(directory, 'dist/project.tsbuildinfo');
      const data = {stage, command, artifacts: artifacts(directory),
        inputs: inventory(path.join(directory, 'src'), () => true),
        publicState: fs.existsSync(publicState) ? {sha256: hash(fs.readFileSync(publicState)),
          bytes: fs.statSync(publicState).size} : null};
      result.stages.push(data); save();
      if (required && command.exit !== 0) {
        result.blockedAt = stage; result.incremental = {passed: false};
      }
      return data;
    }
    function runtime(stage) {
      const command = run(name + '-' + stage + '-runtime', process.execPath,
        [path.join(__dirname, 'defu-runtime.cjs'), path.join(directory, 'dist')], directory);
      return command.exit === 0 ? JSON.parse(fs.readFileSync(path.join(output, command.stdout), 'utf8')) : {passed: [], command};
    }
    const consumerArgs = ['--noEmit', '--strict', '--target', 'ES2022', '--module', 'CommonJS',
      '--moduleResolution', 'Node', '--lib', 'ES2022', '--pretty', 'false'];
    function consumer(stage, sourceText, rejection = false) {
      const entry = 'consumer.ts'; fs.writeFileSync(path.join(directory, entry), sourceText);
      const command = run(name + '-' + stage, process.execPath, [typescript, entry, ...consumerArgs], directory);
      const codes = [...fs.readFileSync(path.join(output, command.stdout), 'utf8').matchAll(/error TS(\d+):/g)].map(match => +match[1]);
      return rejection ? command.exit === 2 && same(codes, [2322]) : command.exit === 0;
    }
    const cold = build('cold'); result.cold = cold.command;
    if (cold.command.exit !== 0) {
      result.downstream = 'Blocked by cold compilation; no declaration/runtime/consumer/incremental pass is credited.';
      return result;
    }
    fs.cpSync(path.join(directory, 'dist'), path.join(output, name + '-cold-artifacts'), {recursive: true});
    result.runtime = runtime('cold');
    result.consumers = {positive: consumer('positive-consumer', consumers.positive),
      negative: consumer('negative-consumer', consumers.negative, true),
      typeBefore: consumer('type-consumer-before', consumers.typeBoundary)};
    config.compilerOptions = {...config.compilerOptions, ...manifest.incrementalOptions};
    fs.writeFileSync(configFile, JSON.stringify(config, null, 2) + '\n');
    const initial = build('incremental-initial'); if (result.blockedAt) return result;
    const warm = build('warm'); if (result.blockedAt) return result;
    applyEdit(directory, manifest.edits.implementation);
    const implementation = build('imported-implementation-edit'); if (result.blockedAt) return result;
    const implementationRuntime = runtime('implementation');
    applyEdit(directory, manifest.edits.exportedType);
    const type = build('exported-type-edit'); if (result.blockedAt) return result;
    result.typeAfterRejected = consumer('type-consumer-after', consumers.typeBoundary, true);
    const extra = manifest.edits.addedDependency;
    fs.writeFileSync(path.join(directory, extra.path), extra.source);
    applyEdit(directory, {path: extra.importPath, from: extra.from, to: extra.to});
    const importing = path.join(directory, extra.importPath);
    fs.writeFileSync(importing, extra.import + fs.readFileSync(importing, 'utf8'));
    const added = build('dependency-added'); if (result.blockedAt) return result;
    const addedRuntime = runtime('dependency-added');
    fs.unlinkSync(path.join(directory, extra.path));
    const missing = build('dependency-missing', false);
    fs.writeFileSync(path.join(directory, extra.path), extra.source);
    const restored = build('dependency-restored'); if (result.blockedAt) return result;
    const restoredRuntime = runtime('dependency-restored');
    const clean = path.join(output, name + '-clean'); fs.mkdirSync(clean);
    unpackFixture(clean, manifest);
    fs.cpSync(path.join(directory, 'src'), path.join(clean, 'src'), {recursive: true});
    fs.copyFileSync(configFile, path.join(clean, 'tsconfig.json'));
    const rebuilt = run(name + '-clean-rebuild', program, buildArgs, clean);
    const warmAgain = build('restored-warm'); if (result.blockedAt) return result;
    result.incremental = {
      warmUnchanged: same(initial.artifacts, warm.artifacts),
      implementationChanged: !same(warm.artifacts, implementation.artifacts),
      typeChanged: !same(implementation.artifacts, type.artifacts),
      dependencyAdded: !same(type.artifacts, added.artifacts),
      missingDependencyRejected: missing.command.exit !== 0,
      missingDependencyArtifactsUnchanged: same(added.artifacts, missing.artifacts),
      restoredWarmUnchanged: same(restored.artifacts, warmAgain.artifacts),
      matchesClean: rebuilt.exit === 0 && same(restored.artifacts, artifacts(clean)),
      runtime: [implementationRuntime, addedRuntime, restoredRuntime].every(value => same(value.passed, manifest.runtimeScenarios)),
      publicStatePublished: result.stages.filter(stage => stage.stage !== 'cold').every(stage => stage.publicState?.bytes > 0),
      cleanArtifacts: artifacts(clean), publicStateExcludedFromArtifactComparison: 'dist/project.tsbuildinfo; per-stage presence, bytes and SHA-256 retained separately',
    };
    result.incremental.passed = Object.entries(result.incremental).filter(([key]) =>
      !['cleanArtifacts', 'publicStateExcludedFromArtifactComparison'].includes(key)).every(([, value]) => value === true);
    return result;
  }
  try {
    save();
    report.native = probe('native', process.execPath, [typescript, '-p', 'tsconfig.json', '--pretty', 'false']);
    assert(assess(report.native, report.native, {equal: true}, manifest), 'invalid Native baseline');
    const nativeDirectory = path.join(output, 'native-cold-artifacts');
    assert.equal(Object.keys(declarations(nativeDirectory, nativeDirectory).native).length, 3, 'all production declarations required');
    const declarationTexts = Object.fromEntries(Object.keys(declarations(nativeDirectory, nativeDirectory).native).map(name => {
      const bytes = fs.readFileSync(path.join(nativeDirectory, name));
      return [name, {sha256: hash(bytes), text: bytes.toString('utf8')}];
    }));
    verifyReference(manifest, require('./defu-v6.1.4.native.json'), report.native, declarationTexts, report.typescript);
    report.nativeBaselineValid = true; save();
    if (!nativeOnly) {
      report.bluets = probe('bluets', compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json']);
      report.declarations = declarations(nativeDirectory, path.join(output, 'bluets-cold-artifacts'));
      report.compatible = assess(report.native, report.bluets, report.declarations, manifest);
    }
    assert.equal(hash(fs.readFileSync(compiler)), report.compilerSha256);
    assert.equal(hash(fs.readFileSync(tsLibrary)), report.typescript.implementationSha256);
    assert.equal(sourceSnapshot(repo).sourceRevision, report.sourceRevision);
    report.conclusion = nativeOnly ? 'native-baseline-valid' : report.compatible ? 'compatible-with-frozen-profile' : 'blocked';
    report.finishedAt = new Date().toISOString(); delete report.active; save(); return report;
  } catch (error) {
    report.conclusion = 'audit-error'; report.error = error.stack; report.finishedAt = new Date().toISOString(); save(); throw error;
  }
}
if (require.main === module) {
  try {
    const args = process.argv.slice(2);
    assert([6, 7].includes(args.length) && args[0] === '--compiler' && args[2] === '--typescript' && args[4] === '--output' &&
      (args.length === 6 || args[6] === '--native-only'), 'usage: --compiler <bluetsc> --typescript <tsc> --output <new-directory> [--native-only]');
    const report = audit({compiler: args[1], typescript: args[3], output: args[5], nativeOnly: args[6] === '--native-only'});
    console.log(JSON.stringify({conclusion: report.conclusion, compatible: report.compatible, nativeBaselineValid: report.nativeBaselineValid}));
    process.exitCode = report.conclusion === 'blocked' ? 2 : 0;
  } catch (error) { console.error(error); process.exitCode = 1; }
}
module.exports = {audit, assess, applyEdit, verifyReference};
