// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const {hash, inventory, sourceSnapshot, unpackFixture} = require('./audit.cjs');
const {applyEdit} = require('./defu-audit.cjs');
const manifest = require('./defu-v6.1.4.json');
const reference = require('./defu-v6.1.4.native.json');
const matrix = require('./defu-interchange.json');
const consumers = require('./defu-consumers.cjs');
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function complete(report) {
  return report.workflows?.length === matrix.profiles.length * matrix.directions.length &&
    same(report.workflows.map(row => row.id).sort(), matrix.profiles.flatMap(profile =>
      matrix.directions.map(direction => profile.id + '-' + direction)).sort()) &&
    report.workflows.every(row => same(Object.keys(row.checks).sort(), [...matrix.requiredChecks].sort()) &&
      Object.values(row.checks).every(value => value === true)) &&
    same(Object.keys(report.ownerChecks || {}).sort(), [...matrix.ownerChecks].sort()) &&
    Object.values(report.ownerChecks).every(value => value === true);
}
function evaluate({compiler, typescript, output, sourceTree}) {
  compiler = fs.realpathSync(compiler); typescript = fs.realpathSync(typescript);
  output = path.resolve(output); fs.mkdirSync(output);
  const repo = path.resolve(__dirname, '../..'), source = sourceSnapshot(repo, sourceTree);
  const implementation = path.resolve(path.dirname(typescript), '../lib/typescript.js');
  assert.equal(require(implementation).version, '5.9.3');
  assert.equal(hash(fs.readFileSync(implementation)), reference.typescript.implementationSha256);
  assert.equal(matrix.projectRevision, manifest.revision);
  const report = {schema: 1, startedAt: new Date().toISOString(), matrix,
    sourceSnapshot: source, sourceRevision: source.sourceRevision,
    node: process.version, typescript: {version: '5.9.3', executableSha256: hash(fs.readFileSync(typescript)),
      implementationSha256: hash(fs.readFileSync(implementation)),
      libraries: inventory(path.dirname(implementation), name => /[/\\]lib\..*\.d\.ts$/.test(name))},
    compilerSha256: hash(fs.readFileSync(compiler)), tools: inventory(__dirname, name => name.endsWith('.cjs')),
    commands: [], workflows: [], ownerChecks: {}};
  assert.deepEqual(report.typescript, reference.typescript, 'changed pinned Native compiler or libraries');
  const save = () => fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  function run(label, command, cwd) {
    report.active = label; save();
    const result = cp.spawnSync(command[0], command.slice(1), {cwd, encoding: 'utf8', timeout: 240000,
      maxBuffer: 16 * 1024 * 1024, env: {...process.env, FORCE_COLOR: '0'}});
    const row = {label, command, cwd, exit: result.status};
    for (const stream of ['stdout', 'stderr']) {
      row[stream] = label + '.' + stream;
      fs.writeFileSync(path.join(output, row[stream]), result[stream] || '');
      row[stream + 'Sha256'] = hash(result[stream] || '');
    }
    report.commands.push(row); save();
    if (result.error || result.status === null) throw result.error || Error('terminated: ' + label);
    return row;
  }
  const artifacts = root => inventory(path.join(root, 'dist'), name => !name.endsWith('.tsbuildinfo'));
  const nativeDeclarations = stage => Object.fromEntries(Object.entries(reference.nativeAcceptance.stages
    .find(row => row.stage === stage).artifacts).filter(([name]) => name.endsWith('.d.ts')));
  function config(profile) {
    return {compilerOptions: {...manifest.profiles[0].compilerOptions, ...manifest.incrementalOptions},
      ...(profile.include ? {include: profile.include} : {files: profile.files})};
  }
  function prepare(name, options) {
    const root = path.join(output, name); fs.mkdirSync(root); unpackFixture(root, manifest);
    fs.writeFileSync(path.join(root, 'tsconfig.json'), JSON.stringify(options, null, 2) + '\n'); return root;
  }
  function build(label, role, root) {
    const command = role === 'native' ? [process.execPath, typescript, '-p', 'tsconfig.json', '--pretty', 'false', '--listEmittedFiles'] :
      [compiler, '--build', 'tsconfig.json', '--verbose', '--diagnostics-json'];
    const row = run(label, command, root), file = path.join(root, 'dist/project.tsbuildinfo');
    row.artifacts = artifacts(root); row.inputs = inventory(path.join(root, 'src'), () => true);
    row.publicState = fs.existsSync(file) ? {sha256: hash(fs.readFileSync(file)), bytes: fs.statSync(file).size} : null;
    save(); return row;
  }
  function runtime(label, root) {
    const row = run(label, [process.execPath, path.join(__dirname, 'defu-runtime.cjs'), path.join(root, 'dist')], root);
    return row.exit === 0 && same(JSON.parse(fs.readFileSync(path.join(output, row.stdout), 'utf8')).passed, manifest.runtimeScenarios);
  }
  function consumer(label, root, text, reject = false) {
    fs.writeFileSync(path.join(root, 'consumer.ts'), text);
    const row = run(label, [process.execPath, typescript, 'consumer.ts', '--noEmit', '--strict', '--target', 'ES2022',
      '--module', 'CommonJS', '--moduleResolution', 'Node', '--lib', 'ES2022', '--pretty', 'false'], root);
    const codes = [...fs.readFileSync(path.join(output, row.stdout), 'utf8').matchAll(/error TS(\d+):/g)].map(match => +match[1]);
    return reject ? row.exit === 2 && same(codes, [2322]) : row.exit === 0;
  }
  try {
    save();
    for (const profile of matrix.profiles) for (const direction of matrix.directions) {
      const id = profile.id + '-' + direction, [donor, receiver] = direction.split('-to-'), options = config(profile);
      const root = prepare(id, options), row = {id, profile: profile.id, direction, checks: {}};
      report.workflows.push(row); const checks = row.checks;
      const baseline = prepare(id + '-native-control', options);
      const nativeControl = build(id + '-native-control', 'native', baseline);
      checks.nativeBaseline = nativeControl.exit === 0 && same(nativeControl.artifacts,
        reference.nativeAcceptance.stages.find(stage => stage.stage === 'cold').artifacts);
      assert(checks.nativeBaseline, 'changed frozen Native project baseline');
      const donated = build(id + '-donor', donor, root); checks.donorAccepted = donated.exit === 0;
      assert(checks.donorAccepted, 'donor build failed');
      checks.donorDeclarations = same(inventory(path.join(root, 'dist'), name => name.endsWith('.d.ts')), nativeDeclarations('cold'));
      checks.donorRuntime = runtime(id + '-donor-runtime', root);
      const received = build(id + '-handoff', receiver, root); checks.handoffAccepted = received.exit === 0;
      assert(checks.handoffAccepted, 'receiver build failed');
      const clean = prepare(id + '-initial-clean', options), cleanBuild = build(id + '-initial-clean', receiver, clean);
      const retained = same(received.artifacts, donated.artifacts), rebuilt = same(received.artifacts, cleanBuild.artifacts);
      row.handoff = {retainedExactDonorArtifacts: retained, equalsExactReceiverClean: rebuilt,
        receiverTrace: fs.readFileSync(path.join(output, received.stdout), 'utf8')};
      checks.handoffArtifactsAccounted = cleanBuild.exit === 0 && (retained || rebuilt);
      checks.exactDeclarations = same(inventory(path.join(root, 'dist'), name => name.endsWith('.d.ts')), nativeDeclarations('cold'));
      checks.runtime = runtime(id + '-handoff-runtime', root);
      checks.positiveConsumer = consumer(id + '-positive', root, consumers.positive);
      checks.negativeConsumer = consumer(id + '-negative', root, consumers.negative, true);
      checks.typeBeforeAccepted = consumer(id + '-type-before', root, consumers.typeBoundary);
      const warm = build(id + '-warm', receiver, root);
      checks.warmUnchanged = warm.exit === 0 && same(warm.artifacts, received.artifacts);
      applyEdit(root, manifest.edits.implementation);
      const impl = build(id + '-implementation', receiver, root);
      checks.implementationEdit = impl.exit === 0 && !same(impl.artifacts, warm.artifacts) && runtime(id + '-implementation-runtime', root);
      applyEdit(root, manifest.edits.exportedType);
      const typed = build(id + '-type', receiver, root);
      checks.exportedTypeEdit = typed.exit === 0 && !same(typed.artifacts, impl.artifacts) &&
        same(inventory(path.join(root, 'dist'), name => name.endsWith('.d.ts')), nativeDeclarations('exported-type-edit'));
      checks.typeAfterRejected = consumer(id + '-type-after', root, consumers.typeBoundary, true);
      const extra = manifest.edits.addedDependency;
      fs.writeFileSync(path.join(root, extra.path), extra.source);
      applyEdit(root, {path: extra.importPath, from: extra.from, to: extra.to});
      const importing = path.join(root, extra.importPath); fs.writeFileSync(importing, extra.import + fs.readFileSync(importing, 'utf8'));
      const added = build(id + '-dependency-added', receiver, root);
      checks.dependencyAdded = added.exit === 0 && !same(added.artifacts, typed.artifacts) && runtime(id + '-added-runtime', root);
      fs.unlinkSync(path.join(root, extra.path)); const missing = build(id + '-dependency-missing', receiver, root);
      checks.missingDependencyRejected = missing.exit !== 0;
      checks.missingDependencyArtifactsUnchanged = same(missing.artifacts, added.artifacts);
      fs.writeFileSync(path.join(root, extra.path), extra.source); const restored = build(id + '-dependency-restored', receiver, root);
      checks.restoredDependency = restored.exit === 0 && runtime(id + '-restored-runtime', root);
      const editedClean = prepare(id + '-edited-clean', options);
      fs.cpSync(path.join(root, 'src'), path.join(editedClean, 'src'), {recursive: true});
      const reconstructed = build(id + '-edited-clean', receiver, editedClean);
      checks.editedCleanEqual = reconstructed.exit === 0 && same(restored.artifacts, reconstructed.artifacts);
      const state = path.join(root, 'dist/project.tsbuildinfo'); fs.writeFileSync(state, '{invalid');
      const corrupt = build(id + '-corrupt-state', receiver, root);
      checks.corruptStateRecovery = corrupt.exit === 0 && same(corrupt.artifacts, reconstructed.artifacts);
      fs.unlinkSync(state); const absent = build(id + '-missing-state', receiver, root);
      checks.missingStateRecovery = absent.exit === 0 && same(absent.artifacts, reconstructed.artifacts);
      const finalWarm = build(id + '-restored-warm', receiver, root);
      checks.restoredWarmUnchanged = finalWarm.exit === 0 && same(finalWarm.artifacts, absent.artifacts);
      save();
    }
    const outside = path.join(output, 'outside-state.tsbuildinfo'); fs.writeFileSync(outside, 'owner-sentinel');
    for (const [id, statePath] of [['external-state-path', outside], ['input-state-collision', 'src/types.ts'],
      ['output-state-collision', 'dist/defu.js']]) {
      const options = config(matrix.profiles[0]); options.compilerOptions.tsBuildInfoFile = statePath;
      const root = prepare(id, options), inputs = inventory(path.join(root, 'src'), () => true);
      const rejected = build(id, 'bluets', root);
      report.ownerChecks[id] = rejected.exit !== 0 && same(artifacts(root), {}) &&
        same(inputs, inventory(path.join(root, 'src'), () => true)) && fs.readFileSync(outside, 'utf8') === 'owner-sentinel';
    }
    assert.equal(hash(fs.readFileSync(compiler)), report.compilerSha256);
    assert.equal(hash(fs.readFileSync(typescript)), report.typescript.executableSha256);
    assert.equal(hash(fs.readFileSync(implementation)), report.typescript.implementationSha256);
    assert.equal(sourceSnapshot(repo, sourceTree).sourceRevision, report.sourceRevision);
    report.conclusion = complete(report) ? 'current-policy-verified' : 'observed-differences';
    report.finishedAt = new Date().toISOString(); delete report.active; save(); return report;
  } catch (error) { report.conclusion = 'audit-error'; report.error = error.stack; save(); throw error; }
}
if (require.main === module) {
  try {
    const args = process.argv.slice(2);
    assert([6, 8].includes(args.length) && args[0] === '--compiler' && args[2] === '--typescript' && args[4] === '--output' &&
      (args.length === 6 || args[6] === '--source-tree'),
      'usage: --compiler <bluetsc> --typescript <tsc> --output <new-directory> [--source-tree <backend-tree-sha>]');
    const report = evaluate({compiler: args[1], typescript: args[3], output: args[5], sourceTree: args[7]});
    console.log(JSON.stringify({conclusion: report.conclusion, workflows: report.workflows.length, ownerChecks: report.ownerChecks}));
    process.exitCode = report.conclusion === 'current-policy-verified' ? 0 : 2;
  } catch (error) { console.error(error); process.exitCode = 1; }
}
module.exports = {evaluate, complete};
