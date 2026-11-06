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

// Cargo emits both the normal bin and a bin unit-test harness. Only the former
// is the compiler used by public CLI oracles; target.test describes capability.
function compilerGuard(messages, expected) {
  if (expected === undefined) return null;
  if (!/^[a-f0-9]{64}$/.test(expected)) throw Error('invalid expected compiler SHA-256');
  const paths = new Set(messages.filter(message =>
    message.reason === 'compiler-artifact' && message.target?.name === 'bluetsc' &&
    Array.isArray(message.target.kind) && message.target.kind.includes('bin') &&
    message.profile?.test === false && typeof message.executable === 'string')
    .map(message => path.resolve(message.executable)));
  if (paths.size !== 1) throw Error('expected exactly one normal bluetsc Cargo executable; found ' + paths.size);
  const executable = [...paths][0];
  const check = () => {
    const actual = crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex');
    if (actual !== expected) throw Error(`compiler SHA-256 changed: ${executable}; expected ${expected}; got ${actual}`);
  };
  check();
  return Object.freeze({executable, sha256: expected, check});
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

function toolHashes(directory = __dirname) {
  return Object.fromEntries(fs.readdirSync(directory).filter(name => /\.(cjs|py)$/.test(name)).sort()
    .map(name => [name, crypto.createHash('sha256').update(fs.readFileSync(path.join(directory, name))).digest('hex')]));
}

function suiteJobs(value = 1) {
  if (!Number.isInteger(value) || value < 1 || value > 4) throw Error('suite jobs must be an integer from 1 to 4');
  return value;
}

function suiteEnvironment(output, index, jobs, environment) {
  if (jobs === 1) return { environment };
  const directory = path.join(output, 'suite-artifacts', String(index).padStart(3, '0'));
  const destinations = {
    BLUEICE_GE5_ROLE_REPORT_DIRECTORY: path.join(directory, 'reports'),
    BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY: path.join(directory, 'observations'),
    BLUEICE_GE5_ADJACENT_OBSERVATION_DIRECTORY: path.join(directory, 'adjacent-observations'),
    BLUEICE_GE5_NODE_ROLE_REPORT: path.join(directory, 'node-roles.json'),
    BLUEICE_GE5_ASYNC_ROLE_REPORT: path.join(directory, 'async-role-report.json'),
    BLUEICE_GE5_CONTINUATION_ROLE_REPORT: path.join(directory, 'continuation-role-report.json'),
    BLUEICE_GE5_NATIVE_FIX_REPORT: path.join(directory, 'native-fix-report.json'),
    BLUEICE_GE5_SYSTEM_PHASE_REPORT: path.join(directory, 'system-phase-report.json'),
    BLUEICE_GE5_REPORT: path.join(directory, 'supplemental-report.json'),
  };
  const artifactPaths = Object.fromEntries(Object.entries(destinations)
    .filter(([name]) => environment[name] !== undefined));
  for (const [name, destination] of Object.entries(artifactPaths)) {
    fs.mkdirSync(name.endsWith('_DIRECTORY') ? destination : path.dirname(destination), {recursive: true});
  }
  return { environment: {...environment, ...artifactPaths}, artifactPaths };
}

// Starts suites in catalog order, records them at the same index, and waits for
// every started child even after a failure. Only the parent performs Cargo work.
async function runSuites({binaries, inventory, output, root, report, save,
  checkCompiler, jobs = 1, environment = process.env}) {
  suiteJobs(jobs);
  if (binaries.length !== inventory.suites.length) throw Error('suite inventory length differs');
  let next = 0, failure;
  const active = new Map();
  const current = () => {
    const values = [...active.values()].sort((a, b) => a.index - b.index);
    fs.writeFileSync(path.join(output, 'current.json'), JSON.stringify(jobs === 1 && values.length
      ? values[0] : {suite_jobs: jobs, active: values}) + '\n');
  };
  const fail = error => { failure ||= error; report.conclusion = 'failure'; report.error ||= error.message; };
  async function run(index) {
    const binary = binaries[index], suite = inventory.suites[index];
    const log = `${String(index).padStart(3, '0')}-${suite.harness.replaceAll(/[^a-zA-Z0-9_-]/g, '_')}.log`;
    const filename = path.join(output, log), started = Date.now();
    const row = {harness: suite.harness, conclusion: 'in_progress', log};
    report.suites[index] = row;
    let fd, pending;
    try {
      checkCompiler();
      const all = testNames(command(binary.executable, ['--list'], {cwd: root, env: environment}));
      const ignored = testNames(command(binary.executable, ['--list', '--ignored'], {cwd: root, env: environment}));
      if (JSON.stringify(ignored) !== JSON.stringify([...suite.ignored_tests].sort()))
        throw Error('compiled ignored-test set differs: ' + suite.harness);
      if (!all.length) throw Error('empty integration binary: ' + suite.harness);
      // Listing can take time while another suite is running; check again
      // immediately before launching a child against the shared normal CLI.
      checkCompiler();
      const context = suiteEnvironment(output, index, jobs, environment);
      if (context.artifactPaths) row.artifact_paths = context.artifactPaths;
      row.pinned_tests = ignored; row.test_names = all;
      fd = fs.openSync(filename, 'wx');
      const child = spawn(binary.executable, ['--include-ignored'], {cwd: root, env: context.environment,
        stdio: ['ignore', fd, fd]});
      pending = new Promise((resolve, reject) => {
        child.once('error', reject); child.once('exit', (code, signal) => resolve({code, signal}));
      });
      active.set(index, {index, harness: suite.harness, pid: child.pid,
        started_at: new Date().toISOString(), log});
      current(); save();
      console.log(`START ${index + 1}/${binaries.length} ${suite.harness} (${all.length} tests, ${ignored.length} selected ignored)`);
      const exit = await pending;
      pending = null;
      fs.closeSync(fd); fd = undefined;
      row.exit_code = exit.code; row.signal = exit.signal;
      checkCompiler();
      if (exit.code !== 0) throw Error('test process exit: ' + JSON.stringify(exit));
      Object.assign(row, summary(fs.readFileSync(filename, 'utf8'), all.length));
      row.conclusion = 'success';
      console.log(`PASS ${index + 1}/${binaries.length} ${suite.harness}: ${row.passed}/${all.length}`);
    } catch (error) {
      row.conclusion = 'failure'; row.error = error.message; fail(error);
      if (!fs.existsSync(filename)) fs.writeFileSync(filename, 'suite preflight failure: ' + error.message + '\n', {flag: 'wx'});
    } finally {
      // A report/filesystem error after spawn must also drain this child.
      if (pending) { try { await pending; } catch (error) { fail(error); } }
      if (fd !== undefined) fs.closeSync(fd);
      active.delete(index);
      row.seconds = (Date.now() - started) / 1000;
      if (fs.existsSync(filename)) row.log_sha256 = crypto.createHash('sha256').update(fs.readFileSync(filename)).digest('hex');
      current(); save();
    }
  }
  async function worker() {
    while (!failure && next < binaries.length) {
      const index = next++;
      try { await run(index); }
      catch (error) { fail(error); }
    }
  }
  await Promise.all(Array.from({length: jobs}, worker));
  try { checkCompiler(); } catch (error) { fail(error); }
  if (!failure && (report.suites.length !== binaries.length || report.suites.some(row => row.conclusion !== 'success')))
    fail(Error('incomplete suite measurement'));
  if (failure) { save(); throw failure; }
}

async function measure(output, root = path.resolve(__dirname, '../../../..'), staged = false, jobs = 1) {
  suiteJobs(jobs);
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
  const checkSource = () => command(cargo, ['run', '-p', 'blueice-bluets', '--example', 'compatibility_inventory', '--locked', '--offline', '--', staged ? '--check-staged' : '--check'], { cwd: root });
  checkSource();
  const inventoryPath = path.join(root, 'development/browser_core/phase-18-bluets/COMPATIBILITY_SUITES.json');
  const inventoryBytes = fs.readFileSync(inventoryPath, 'utf8');
  const inventory = JSON.parse(inventoryBytes);
  const tools = toolHashes();
  const artifacts = command(cargo, ['test', '-p', 'blueice-bluets', '-p', 'blueice-bluets-bluejs',
    '--no-run', '--locked', '--offline', '--message-format=json'], { cwd: root });
  const messages = artifacts.split(/\r?\n/).filter(Boolean).map(JSON.parse);
  // Fail before listing or launching a suite if no-run rebuilt the normal CLI.
  const compiler = compilerGuard(messages, process.env.BLUEICE_GE5_EXPECTED_COMPILER_SHA256);
  const binaries = executables(messages, inventory);
  const report = { schema: 1, typescript: version, suite_jobs: jobs, source_revision: inventory.source_revision,
    ...(compiler ? {compilerSha256: compiler.sha256, compilerExecutable: compiler.executable} : {}),
    source_graph_sha256: inventory.source_graph_sha256,
    runner_base_revision: command('git', ['rev-parse', 'HEAD'], { cwd: root }).trim(),
    oracle_tools_sha256: tools,
    source_mode: staged ? 'immutable-staged-backend-tree' : 'committed-head',
    platform: process.platform + '-' + process.arch,
    rustc: command('rustc', ['--version']).trim(), started_at: new Date().toISOString(),
    finished_at: null, conclusion: 'in_progress', suites: [] };
  const save = () => fs.writeFileSync(path.join(output, 'measurement.json'), JSON.stringify(report, null, 2) + '\n');
  save();
  const checkCompiler = () => compiler?.check();
  try {
    await runSuites({binaries, inventory, output, root, report, save, checkCompiler, jobs});
    // Do not publish a green run if its source/index or catalog changed meanwhile.
    checkSource();
    if (fs.readFileSync(inventoryPath, 'utf8') !== inventoryBytes) throw Error('suite catalog changed during measurement');
    if (JSON.stringify(toolHashes()) !== JSON.stringify(tools)) throw Error('oracle tooling changed during measurement');
    checkCompiler();
    report.finished_at = new Date().toISOString(); report.conclusion = 'success'; save();
    console.log(`COMPLETE ${report.suites.length}/${binaries.length} suites: ${path.join(output, 'measurement.json')}`);
    return report;
  } catch (error) {
    report.finished_at = new Date().toISOString(); report.conclusion = 'failure';
    report.error = error.message; save(); throw error;
  }
}

function parseArguments(args) {
  const usage = 'usage: node measure_compatibility_cases.cjs --output-dir <empty-directory> [--staged-source] [--jobs 1..4]';
  if (args[0] !== '--output-dir' || !args[1] || args[1].startsWith('--')) throw Error(usage);
  let staged = false, jobs = 1, hasJobs = false;
  for (let index = 2; index < args.length; index++) {
    if (args[index] === '--staged-source' && !staged) staged = true;
    else if (args[index] === '--jobs' && !hasJobs && /^[1-4]$/.test(args[index + 1] || '')) {
      jobs = suiteJobs(Number(args[++index])); hasJobs = true;
    } else throw Error(usage);
  }
  return {output: args[1], staged, jobs};
}

module.exports = { executables, compilerGuard, testNames, summary, toolHashes, suiteJobs, runSuites, parseArguments };

if (require.main === module) {
  try {
    const args = parseArguments(process.argv.slice(2));
    measure(args.output, undefined, args.staged, args.jobs).catch(error => { console.error(error); process.exitCode = 1; });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
