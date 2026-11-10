// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const {hash, sourceSnapshot} = require('./audit.cjs');
function reproduce({compiler, typescript, output}) {
  const manifest = require('./defu-reproductions.json');
  const reference = require('./defu-reproductions.native.json');
  compiler = fs.realpathSync(compiler); typescript = fs.realpathSync(typescript);
  const implementation = path.resolve(path.dirname(typescript), '../lib/typescript.js');
  assert.equal(require(implementation).version, '5.9.3');
  assert.equal(hash(fs.readFileSync(implementation)), reference.implementationSha256);
  output = path.resolve(output); fs.mkdirSync(output);
  const repo = path.resolve(__dirname, '../..'), source = sourceSnapshot(repo);
  const report = {schema: 1, recordedAt: new Date().toISOString(), sourceRevision: source.sourceRevision,
    sourceSnapshot: source, compilerSha256: hash(fs.readFileSync(compiler)),
    scope: 'Eight authored project-blocker controls; verdict comparison only, complete raw diagnostics retained. Separate from global compiler case counts.', cases: []};
  const save = () => fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  for (const [index, entry] of manifest.cases.entries()) {
    const row = {id: entry.id, sourceSha256: hash(entry.source)};
    assert.equal(row.id, reference.cases[index].id);
    assert.equal(row.sourceSha256, reference.cases[index].sourceSha256);
    for (const role of ['native', 'bluets']) {
      const directory = path.join(output, entry.id, role); fs.mkdirSync(path.join(directory, 'src'), {recursive: true});
      fs.writeFileSync(path.join(directory, 'src/main.ts'), entry.source);
      const {id, ...config} = manifest.profile;
      fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify(config, null, 2) + '\n');
      const command = role === 'native' ? [process.execPath, typescript, '-p', 'tsconfig.json', '--pretty', 'false'] :
        [compiler, 'build', '--config', 'tsconfig.json', '--diagnostics-json'];
      const result = cp.spawnSync(command[0], command.slice(1), {cwd: directory, encoding: 'utf8',
        timeout: 120000, maxBuffer: 8 * 1024 * 1024, env: {...process.env, FORCE_COLOR: '0'}});
      if (result.error || result.status === null) throw result.error || Error('terminated ' + entry.id);
      const dist = path.join(directory, 'dist');
      const observation = {exit: result.status, stdout: result.stdout, stderr: result.stderr,
        declarations: fs.existsSync(dist) ? Object.fromEntries(fs.readdirSync(dist).filter(name => name.endsWith('.d.ts')).sort()
          .map(name => [name, fs.readFileSync(path.join(dist, name), 'utf8')])) : {}};
      for (const stream of ['stdout', 'stderr']) fs.writeFileSync(path.join(directory, stream + '.log'), observation[stream]);
      row[role] = {...observation, command};
      if (role === 'native') {
        assert.equal(result.status === 0, entry.nativeExpectedAccept, 'changed Native verdict');
        assert.deepEqual(observation, reference.cases[index].native, 'changed frozen Native observation');
      }
    }
    row.verdictAgreement = (row.native.exit === 0) === (row.bluets.exit === 0);
    report.cases.push(row); save();
  }
  assert.equal(hash(fs.readFileSync(compiler)), report.compilerSha256);
  assert.equal(sourceSnapshot(repo).sourceRevision, report.sourceRevision);
  report.verdictAgreements = report.cases.filter(row => row.verdictAgreement).length;
  report.conclusion = report.verdictAgreements === report.cases.length ? 'all-verdicts-agree' : 'observed-differences';
  save(); return report;
}
if (require.main === module) {
  try {
    const args = process.argv.slice(2);
    assert(args.length === 6 && args[0] === '--compiler' && args[2] === '--typescript' && args[4] === '--output');
    const report = reproduce({compiler: args[1], typescript: args[3], output: args[5]});
    console.log(JSON.stringify({conclusion: report.conclusion, verdictAgreements: report.verdictAgreements, cases: report.cases.length}));
    process.exitCode = report.conclusion === 'observed-differences' ? 2 : 0;
  } catch (error) { console.error(error); process.exitCode = 1; }
}
module.exports = {reproduce};
