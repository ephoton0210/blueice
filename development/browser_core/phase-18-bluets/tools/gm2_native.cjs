// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Pinned native build/CLI observations. No BlueTS output supplies an expectation.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const {spawnSync} = require('node:child_process');
const observe = require('./gm2_observations.cjs');

function status(root, item) {
  const text = item.messageText.toString();
  const quoted = [...text.matchAll(/'([^']+)'/g)].map(match => match[1].replaceAll(root + '/', ''));
  switch (item.code) {
    case 6358: return {kind: 'build', project: quoted[0]};
    case 6351: case 6361: return {kind: 'current', project: quoted[0]};
    case 6354: return {kind:'signature-current',project:quoted[0]};
    case 6359: return {kind:'timestamps',project:quoted[0]};
    case 6400: return {kind:'pending-timestamps',project:quoted[0]};
    case 6363: case 6362: case 6383: case 6382:
      return {kind: item.code === 6363 || item.code === 6383 ? 'blocked' : 'skip', project: quoted[0],
        dependency: text.match(/dependency '([^']+)'/)[1].replaceAll(root + '/', ''),
        reason: text.includes('was not built') ? 'not-built' : 'errors'};
    default:
      assert(!/Building project|Skipping build|can't be built|is up to date|Updating output timestamps/.test(text),
        'unclassified native build event ' + item.code + ': ' + text);
      return null;
  }
}
function host(ts, root, diagnostics, events, watch = false, completed) {
  const system = {...ts.sys, getCurrentDirectory: () => root};
  const report = d => diagnostics.push(observe.diagnostic(ts, root, d));
  const reportStatus = d => {
    const value = status(root, d);
    if (value) events.push(value);
  };
  const result = watch ? ts.createSolutionBuilderWithWatchHost(system, undefined, report, reportStatus, completed) :
    ts.createSolutionBuilderHost(system, undefined, report, reportStatus);
  const create = result.createProgram;
  result.createProgram = (...args) => {
    const program = create(...args);
    const files = program.getProgram().getSourceFiles().map(file => file.fileName)
      .filter(file => file.startsWith(root + '/')).map(file => path.relative(root, file).replaceAll(path.sep, '/'));
    result.selected.push(...files);
    return program;
  };
  result.selected = [];
  return result;
}
function build(ts, root, stop) {
  const diagnostics = [], events = [];
  const compilerHost = host(ts, root, diagnostics, events);
  const exit = ts.createSolutionBuilder(compilerHost, [path.join(root, 'tsconfig.json')], {
    verbose: true, stopBuildOnErrors: !!stop,
  }).build();
  return {exit, events, diagnostics};
}
function cli(ts, root, args) {
  const loader = require(path.join(__dirname, '../../../../backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
  const executable = loader.cliPath(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
  const actual = spawnSync(process.execPath, [executable, ...args], {cwd: root, encoding: 'utf8'});
  if (actual.error) throw actual.error;
  const parsed = ts.parseCommandLine(args);
  const projectArgument = args.findIndex(value => value === '--project' || value === '-p');
  const filenames = parsed.fileNames;
  let diagnostics = parsed.errors.slice();
  if (projectArgument >= 0 && filenames.length) {
    diagnostics.push(ts.createCompilerDiagnostic(ts.Diagnostics.Option_project_cannot_be_mixed_with_source_files_on_a_command_line));
  }
  const renderedCodes = [...actual.stdout.matchAll(/error TS(\d+):/g)].map(match => Number(match[1])).sort((a, b) => a - b);
  const verify = values => assert.deepEqual(values.map(d => d.code).sort((a, b) => a - b), renderedCodes,
    'native API diagnostics must describe the actual native CLI');
  if (diagnostics.length) verify(diagnostics);
  if (diagnostics.length) return {exit: actual.status, diagnostics: diagnostics.map(d => observe.diagnostic(ts, root, d)),
    selectedInputs: [], shown: null};
  if (parsed.options.showConfig) {
    return {exit: actual.status, diagnostics: [], selectedInputs: [], shown: JSON.parse(actual.stdout)};
  }
  const compilerHost = ts.createCompilerHost(parsed.options);
  compilerHost.getCurrentDirectory = () => root;
  compilerHost.getSourceFile = (file, languageVersion) => {
    const text = ts.sys.readFile(path.resolve(root, file));
    return text === undefined ? undefined : ts.createSourceFile(file, text, languageVersion, true);
  };
  // The actual native CLI above owns emission; this program only describes the
  // full diagnostic/selected-input boundary without writing relative outputs.
  compilerHost.writeFile = () => {};
  const program = ts.createProgram(filenames, parsed.options, compilerHost);
  diagnostics = ts.getPreEmitDiagnostics(program);
  const result = program.emit();
  diagnostics = ts.sortAndDeduplicateDiagnostics([...diagnostics, ...result.diagnostics]);
  verify(diagnostics);
  return {exit: actual.status,
    diagnostics: diagnostics.map(d => observe.diagnostic(ts, root, d)),
    selectedInputs: program.getSourceFiles().map(file => path.resolve(root, file.fileName)).filter(file => file.startsWith(root + '/'))
      .map(file => path.relative(root, file).replaceAll(path.sep, '/')).sort(), shown: null};
}
function statePath(root, workflow) {
  return path.join(root, workflow.stateFile || 'tsconfig.tsbuildinfo');
}
function normalizeState(ts, root, workflow) {
  const file = statePath(root, workflow);
  const value = JSON.parse(fs.readFileSync(file, 'utf8'));
  const lib = path.dirname(ts.getDefaultLibFilePath({}));
  value.fileNames = value.fileNames.map(name => {
    const absolute = path.resolve(path.dirname(file), name);
    return absolute.startsWith(lib + '/') ? '@typescript/lib/' + path.basename(absolute) : name;
  });
  return value;
}
function programState(ts, root, workflow) {
  const config = ts.readConfigFile(path.join(root, 'tsconfig.json'), ts.sys.readFile);
  assert(!config.error);
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
  parsed.options.configFilePath = path.join(root, 'tsconfig.json');
  const value = ts.readBuilderProgram(parsed.options, {
    ...ts.sys, getCurrentDirectory: () => root, useCaseSensitiveFileNames: () => ts.sys.useCaseSensitiveFileNames,
  });
  assert(value, workflow.id + ': real native builder could not read build-info');
  const prefix = ts.sys.useCaseSensitiveFileNames ? root : root.toLowerCase();
  const owned = [...value.state.fileInfos].filter(([name]) => name.startsWith(prefix + '/'));
  return observe.sorted(Object.fromEntries(owned.map(([name, info]) =>
    [name.slice(prefix.length + 1).replaceAll(path.sep, '/'), {version: info.version, signature: info.signature ?? null}])));
}
function restoreState(ts, root, workflow, value) {
  value = structuredClone(value);
  const file = statePath(root, workflow);
  const lib = path.dirname(ts.getDefaultLibFilePath({}));
  value.fileNames = value.fileNames.map(name => name.startsWith('@typescript/lib/') ?
    path.relative(path.dirname(file), path.join(lib, path.basename(name))).replaceAll(path.sep, '/') : name);
  observe.write(root, {[path.relative(root, file)]: JSON.stringify(value)});
}
module.exports = {status, host, build, cli, statePath, normalizeState, programState, restoreState};
