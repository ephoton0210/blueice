// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn, spawnSync } = require('node:child_process');

function executables(messages, inventory) {
  const found = new Map();
  for (const message of messages) {
    if (message.reason !== 'compiler-artifact' || !message.profile?.test ||
        !message.target?.kind.includes('test') || !message.executable) continue;
    const source = message.target.src_path.replaceAll('\\', '/');
    const suite = inventory.suites.find(suite => source.endsWith('/' + suite.harness));
    if (!suite) continue;
    if (found.has(suite.harness)) throw Error('duplicate Cargo artifact: ' + suite.harness);
    found.set(suite.harness, { harness: suite.harness, executable: message.executable });
  }
  return inventory.suites.map(suite => {
    if (!found.has(suite.harness)) throw Error('missing Cargo artifact: ' + suite.harness);
    return found.get(suite.harness);
  });
}

function testNames(text) {
  return [...text.matchAll(/^(.+): test\r?$/gm)].map(match => match[1]).sort();
}

function summary(text, expected) {
  const matches = [...text.matchAll(/test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered out;/g)];
  const last = matches.at(-1);
  if (!last) throw Error('missing libtest summary');
  const result = { passed: Number(last[2]), failed: Number(last[3]),
    ignored: Number(last[4]), filtered: Number(last[5]) };
  if (last[1] !== 'ok' || result.passed !== expected || result.failed || result.ignored || result.filtered)
    throw Error('incomplete or failed libtest run: ' + JSON.stringify({ expected, ...result }));
  return result;
}

function command(executable, args, options = {}) {
  const result = spawnSync(executable, args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(`${executable} failed (${result.status}):\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

async function measure(output, root = path.resolve(__dirname, '../../../..')) {
  // Windows's explicit Unix exclusions are validated separately. A reduced
  // platform run must never be published as the complete case denominator.
  if (!['darwin', 'linux'].includes(process.platform)) throw Error('complete case measurement requires macOS/Linux');
  const forbidden = Object.keys(process.env).filter(key =>
    (key.startsWith('BLUEICE_WRITE_') && process.env[key] === '1') || key.startsWith('BLUETS_'));
  if (forbidden.length) throw Error('recording/filter environment must be unset: ' + forbidden.join(', '));
  if (!process.env.BLUEICE_BLUETSC_ORACLE) throw Error('set BLUEICE_BLUETSC_ORACLE');
  const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
  const version = load(process.env.BLUEICE_BLUETSC_ORACLE).version;
  if (version !== '5.9.3') throw Error('unpinned TypeScript: ' + version);
  output = path.resolve(output);
  if (!fs.existsSync(output)) fs.mkdirSync(output);
  if (fs.readdirSync(output).length) throw Error('output directory must be empty: ' + output);
  const cargo = process.env.CARGO || 'cargo';
  command(cargo, ['run', '-p', 'blueice-bluets', '--example', 'compatibility_inventory', '--locked', '--offline', '--', '--check'], { cwd: root });
  const inventory = JSON.parse(fs.readFileSync(path.join(root, 'development/browser_core/phase-18-bluets/COMPATIBILITY_SUITES.json'), 'utf8'));
  const artifacts = command(cargo, ['test', '-p', 'blueice-bluets', '-p', 'blueice-bluets-bluejs',
    '--no-run', '--locked', '--offline', '--message-format=json'], { cwd: root });
  const binaries = executables(artifacts.split(/\r?\n/).filter(Boolean).map(JSON.parse), inventory);
  const report = { schema: 1, typescript: version, source_revision: inventory.source_revision,
    source_graph_sha256: inventory.source_graph_sha256,
    runner_revision: command('git', ['rev-parse', 'HEAD'], { cwd: root }).trim(),
    platform: process.platform + '-' + process.arch,
    rustc: command('rustc', ['--version']).trim(), started_at: new Date().toISOString(),
    finished_at: null, conclusion: 'in_progress', suites: [] };
  const save = () => fs.writeFileSync(path.join(output, 'measurement.json'), JSON.stringify(report, null, 2) + '\n');
  save();
  for (let index = 0; index < binaries.length; index++) {
    const binary = binaries[index], suite = inventory.suites[index];
    const all = testNames(command(binary.executable, ['--list'], { cwd: root }));
    const ignored = testNames(command(binary.executable, ['--list', '--ignored'], { cwd: root }));
    const expected = [...suite.ignored_tests].sort();
    if (JSON.stringify(ignored) !== JSON.stringify(expected)) throw Error('compiled ignored-test set differs: ' + suite.harness);
    if (!all.length) throw Error('empty integration binary: ' + suite.harness);
    const log = `${String(index).padStart(3, '0')}-${suite.harness.replaceAll(/[^a-zA-Z0-9_-]/g, '_')}.log`;
    const fd = fs.openSync(path.join(output, log), 'wx');
    const started = Date.now();
    const child = spawn(binary.executable, ['--include-ignored'], { cwd: root, env: process.env,
      stdio: ['ignore', fd, fd] });
    fs.writeFileSync(path.join(output, 'current.json'), JSON.stringify({ harness: suite.harness,
      pid: child.pid, started_at: new Date().toISOString(), log }) + '\n');
    console.log(`START ${index + 1}/${binaries.length} ${suite.harness} (${all.length} tests, ${ignored.length} selected ignored)`);
    const exit = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', (code, signal) => resolve({ code, signal })); });
    fs.closeSync(fd);
    const bytes = fs.readFileSync(path.join(output, log));
    let counts;
    try {
      if (exit.code !== 0) throw Error('test process exit: ' + JSON.stringify(exit));
      counts = summary(bytes.toString('utf8'), all.length);
    } catch (error) {
      report.conclusion = 'failure';
      report.suites.push({ harness: suite.harness, conclusion: 'failure', log, error: error.message });
      report.finished_at = new Date().toISOString(); save(); throw error;
    }
    report.suites.push({ harness: suite.harness, conclusion: 'success', ...counts,
      // Legacy schema-1 field name means selected ignored Rust functions;
      // it includes an owner-only control, not exclusively tsc comparisons.
      pinned_tests: ignored, test_names: all, seconds: (Date.now() - started) / 1000,
      log, log_sha256: crypto.createHash('sha256').update(bytes).digest('hex') });
    save();
    console.log(`PASS ${index + 1}/${binaries.length} ${suite.harness}: ${counts.passed}/${all.length}`);
  }
  report.finished_at = new Date().toISOString(); report.conclusion = 'success'; save();
  console.log(`COMPLETE ${report.suites.length}/${binaries.length} suites: ${path.join(output, 'measurement.json')}`);
  return report;
}

module.exports = { executables, testNames, summary };

if (require.main === module) {
  if (process.argv.length !== 4 || process.argv[2] !== '--output-dir') {
    console.error('usage: node measure_compatibility_cases.cjs --output-dir <empty-directory>'); process.exitCode = 1;
  } else measure(process.argv[3]).catch(error => { console.error(error); process.exitCode = 1; });
}
