// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Real Node loaders, closed AMD/UMD/System loaders and the actual pinned tslib.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const vm = require('node:vm');
const { spawnSync } = require('node:child_process');
const { createRequire } = require('node:module');
const { pathToFileURL } = require('node:url');
const { owned, write } = require('./gm1_observations.cjs');

function sourceFiles(input, row) {
  return { ...(row.provider === 'tslib-2.8.1' ? input.sharedFiles : {}), ...row.files };
}
function inspect(ts, outputs, helperNames) {
  return Object.fromEntries(Object.entries(outputs).filter(([n]) => /\.(?:js|mjs|cjs)$/.test(n)).sort().map(([name, text]) => {
    const dependencies = new Set(), helpers = new Set();
    const source = ts.createSourceFile(name, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
    const factories = new Set(), requires = new Set(['require']);
    for (const statement of source.statements) {
      if (!ts.isImportDeclaration(statement) || !['module', 'node:module'].includes(statement.moduleSpecifier.text)) continue;
      const bindings = statement.importClause?.namedBindings;
      if (bindings && ts.isNamedImports(bindings)) for (const binding of bindings.elements)
        if ((binding.propertyName?.text || binding.name.text) === 'createRequire') factories.add(binding.name.text);
    }
    for (const statement of source.statements) if (ts.isVariableStatement(statement)) {
      for (const binding of statement.declarationList.declarations) if (ts.isIdentifier(binding.name) && binding.initializer &&
        ts.isCallExpression(binding.initializer) && ts.isIdentifier(binding.initializer.expression) && factories.has(binding.initializer.expression.text)) requires.add(binding.name.text);
    }
    function visit(node) {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) {
        dependencies.add(node.moduleSpecifier.text);
      }
      if (ts.isCallExpression(node)) {
        const expr = node.expression;
        if (ts.isIdentifier(expr) && requires.has(expr.text) && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) dependencies.add(node.arguments[0].text);
        if (ts.isPropertyAccessExpression(expr) && ['register', 'define'].includes(expr.name.text) || ts.isIdentifier(expr) && expr.text === 'define') {
          const array = node.arguments.find(ts.isArrayLiteralExpression);
          if (array) for (const item of array.elements) if (ts.isStringLiteral(item) && !['require', 'exports'].includes(item.text)) dependencies.add(item.text);
        }
      }
      if (ts.isIdentifier(node) && helperNames.has(node.text)) helpers.add(node.text);
      ts.forEachChild(node, visit);
    }
    visit(source);
    return [name, { dependencies: [...dependencies].sort(), helperImports: dependencies.has('tslib') ? [...helpers].sort() : [] }];
  }));
}

function dependency(file, specifier, root) {
  const resolved = createRequire(file).resolve(specifier);
  if (!fs.realpathSync(resolved).startsWith(root + path.sep)) throw Error('loader dependency escaped owner root');
  return resolved;
}
function ordinary(entry, root, amd, globals) {
  const modules = new Map();
  function load(file) {
    if (modules.has(file)) return modules.get(file);
    if (file.includes(path.sep + 'node_modules' + path.sep)) return createRequire(file)(file);
    let exports = {};
    modules.set(file, exports);
    const require = specifier => load(dependency(file, specifier, root));
    const context = { ...globals, Symbol, Promise, Map, Set, WeakMap, WeakSet };
    if (amd) {
      context.define = (...args) => {
        const factory = args.pop(), dependencies = args.pop();
        const result = factory(...dependencies.map(name => name === 'exports' ? exports : name === 'require' ? require : require(name)));
        if (result !== undefined) { exports = result; modules.set(file, result); }
      };
      context.define.amd = {};
    } else Object.assign(context, { exports, module: { exports }, require });
    vm.runInNewContext(fs.readFileSync(file, 'utf8'), context, { filename: file, timeout: 5000 });
    if (!amd) { exports = context.module.exports; modules.set(file, exports); }
    return exports;
  }
  return load(entry);
}
async function system(entry, root, globals) {
  const records = new Map();
  function register(file) {
    if (records.has(file)) return records.get(file);
    const record = { exports: {}, listeners: [], executed: false };
    records.set(file, record);
    if (file.includes(path.sep + 'node_modules' + path.sep)) {
      Object.assign(record.exports, createRequire(file)(file));
      record.dependencies = []; record.imports = []; record.body = { execute() {} };
      return record;
    }
    const publish = (name, value) => {
      Object.assign(record.exports, typeof name === 'object' ? name : { [name]: value });
      for (const listener of record.listeners) listener(record.exports);
      return value;
    };
    const context = { ...globals, Symbol, Promise, Map, Set, WeakMap, WeakSet,
      System: { register(dependencies, factory) { record.dependencies = dependencies; record.body = factory(publish, { id: file }); } } };
    vm.runInNewContext(fs.readFileSync(file, 'utf8'), context, { filename: file, timeout: 5000 });
    record.imports = record.dependencies.map((name, index) => {
      const imported = register(dependency(file, name, root)), setter = record.body.setters[index];
      if (setter) { imported.listeners.push(setter); setter(imported.exports); }
      return imported;
    });
    return record;
  }
  async function execute(record) {
    if (record.executed) return;
    record.executed = true;
    for (const imported of record.imports) await execute(imported);
    await record.body.execute();
  }
  const record = register(entry); await execute(record); return record.exports;
}
async function run(root, row, supplied, branch) {
  const entry = owned(root, row.runtimeEntry);
  let globals = {};
  if (supplied) {
    globals = createRequire(entry)(owned(root, 'node_modules/tslib/tslib.js'));
    Object.assign(globalThis, globals);
  }
  let namespace;
  if (row.runtimeModule === 'AMD' || row.runtimeModule === 'UMD' && branch === 'amd') namespace = ordinary(entry, root, true, globals);
  else if (row.runtimeModule === 'UMD') namespace = ordinary(entry, root, false, globals);
  else if (row.runtimeModule === 'System') namespace = await system(entry, root, globals);
  else if (row.runtimeModule === 'CommonJS') namespace = createRequire(entry)(entry);
  else namespace = await import(pathToFileURL(entry).href);
  const result = await namespace.result;
  let live = null;
  if (row.form === 'interop-exportstar-cycle') {
    const before = namespace.read(); namespace.update(10);
    live = { before, after: namespace.read() };
  }
  process.stdout.write(JSON.stringify({ result, live }));
}
function execute(input, row, outputs) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge2-run-')));
  try {
    write(root, sourceFiles(input, row)); write(root, row.runtimeFiles || {}); write(root, outputs);
    // Host globals are an explicit noEmitHelpers fixture contract; they are
    // never supplied to a production compiler/runtime or inferred from output.
    const supplied = !!row.options.noEmitHelpers && !row.options.importHelpers;
    const scenarios = [{ name: 'configured', supplied, branch: 'commonjs' }];
    if (supplied) scenarios.push({ name: 'missingGlobals', supplied: false, branch: 'commonjs' });
    if (row.runtimeModule === 'UMD') scenarios.push({ name: 'amdBranch', supplied, branch: 'amd' });
    return Object.fromEntries(scenarios.map(scenario => {
      const result = spawnSync(process.execPath, [__filename, '--run', root, JSON.stringify(row), String(scenario.supplied), scenario.branch],
        { encoding: 'utf8', timeout: 10000, maxBuffer: 2 * 1024 * 1024 });
      if (result.error) throw result.error;
      return [scenario.name, { exit: result.status, stdout: result.stdout, stderr: result.stderr.replaceAll(root, '<root>') }];
    }));
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
module.exports = { owned, write, sourceFiles, inspect, execute };
if (process.argv[2] === '--run') run(process.argv[3], JSON.parse(process.argv[4]), process.argv[5] === 'true', process.argv[6])
  .catch(error => { process.stderr.write(error.name + ': ' + error.message); process.exitCode = 1; });
