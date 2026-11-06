// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute the emitted graph and observe updates through its original export cycle.
const assert = require('node:assert/strict'), vm = require('node:vm'), path = require('node:path');

async function execute(ts, outputs, files, options, branch) {
  const library = {exports: {}};
  if (files['node_modules/tslib/tslib.js']) vm.runInNewContext(files['node_modules/tslib/tslib.js'],
    {exports: library.exports, module: library}, {timeout: 1000});
  const globals = options.noEmitHelpers && !options.importHelpers ? {...library.exports} : {};
  const context = vm.createContext(globals), cache = new Map();
  function resolve(from, name) {
    assert(name.startsWith('./') || name.startsWith('../'), 'original relative emitted graph dependency');
    const result = path.posix.normalize(path.posix.join(path.posix.dirname(from), name));
    assert(Object.hasOwn(outputs, result), 'dependency belongs to the exact emitted graph'); return result;
  }
  const parsed = ts.createSourceFile('main.js', outputs['out/main.js'], ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const esm = parsed.statements.some(node => ts.isImportDeclaration(node) || ts.isExportDeclaration(node)
    || node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword));
  let load;
  if (esm) {
    assert.equal(typeof vm.SourceTextModule, 'function', 'cyclic ESM execution requires --experimental-vm-modules');
    function module(name) {
      if (cache.has(name)) return cache.get(name);
      const item = name === 'tslib' ? new vm.SyntheticModule(Object.keys(library.exports), function () {
        for (const key of Object.keys(library.exports)) this.setExport(key, library.exports[key]);
      }, {context}) : new vm.SourceTextModule(outputs[name], {context, identifier: name});
      cache.set(name, item); return item;
    }
    const main = module('out/main.js');
    await main.link((name, from) => module(name === 'tslib' ? name : resolve(from.identifier, name)));
    await main.evaluate({timeout: 1000}); load = name => module(name).namespace;
  } else if (String(options.module).toLowerCase() === 'system') {
    function instantiate(name) {
      if (cache.has(name)) return cache.get(name);
      const record = {namespace: {}, subscribers: [], state: 'declaring'}; cache.set(name, record);
      function publish(key, value) {
        const entries = typeof key === 'string' ? [[key, value]] : Object.entries(key); let changed = false;
        for (const [name, item] of entries) {
          if (!Object.hasOwn(record.namespace, name) || !Object.is(record.namespace[name], item)) {
            record.namespace[name] = item; changed = true;
          }
        }
        if (changed) for (const setter of record.subscribers) setter(record.namespace);
        return value;
      }
      const sandbox = {...globals, System: {register(dependencies, factory) {
        assert(!record.body, 'one registration per emitted module');
        record.dependencies = Array.from(dependencies, dependency => resolve(name, dependency));
        record.body = factory(publish, {id: name});
      }}};
      vm.runInNewContext(outputs[name], sandbox, {timeout: 1000});
      assert(record.body && record.body.setters.length === record.dependencies.length);
      record.state = 'linked';
      record.dependencies.forEach((dependency, index) => {
        const target = instantiate(dependency), setter = record.body.setters[index];
        target.subscribers.push(setter); setter(target.namespace);
      });
      return record;
    }
    function evaluate(name) {
      const record = instantiate(name); if (record.state === 'evaluating' || record.state === 'evaluated') return record;
      record.state = 'evaluating'; for (const dependency of record.dependencies) evaluate(dependency);
      record.body.execute(); record.state = 'evaluated'; return record;
    }
    evaluate('out/main.js'); load = name => cache.get(name).namespace;
  } else {
    load = function (name) {
      if (cache.has(name)) return cache.get(name).exports;
      const module = {exports: {}}; cache.set(name, module);
      const require = dependency => dependency === 'tslib' ? library.exports : load(resolve(name, dependency));
      let registrations = 0;
      const define = (dependencies, factory) => {
        registrations++; const result = factory(...dependencies.map(name => name === 'exports' ? module.exports : name === 'require' ? require : require(name)));
        if (result !== undefined) module.exports = result;
      };
      define.amd = {};
      const amd = String(options.module).toLowerCase() === 'amd' || String(options.module).toLowerCase() === 'umd' && branch === 'amd';
      vm.runInNewContext(outputs[name], {...globals, ...(amd ? {define} : {exports: module.exports, module, require})}, {timeout: 1000});
      assert.equal(registrations, amd ? 1 : 0); return module.exports;
    };
    load('out/main.js');
  }
  const main = load('out/main.js'), left = load('out/left.js'), right = load('out/right.js'), dep = load('out/dep.js');
  assert.deepEqual(Object.keys(main).sort(), ['leftValue', 'read', 'result', 'update']);
  assert.equal(main.read, left.read); assert.equal(main.update, left.update); assert(!Object.hasOwn(main, 'default'));
  const observations = [[main.result, dep.default(1), dep.value, left.read(), right.current(), main.leftValue]];
  assert.deepEqual(observations[0], [14, 2, 3, 9, 4, 4]);
  for (const value of [10, -2, 4]) {
    main.update(value);
    const observed = [main.result, main.read(), left.read(), right.current(), main.leftValue];
    assert.deepEqual(observed, [14, value + 5, value + 5, value, value], 'live imports and star exports survive cycle mutation');
    observations.push(observed);
  }
  return observations;
}

async function run(ts, native, actual, files, options) {
  const branches = String(options.module).toLowerCase() === 'umd' ? ['commonjs', 'amd'] : ['default'], results = [];
  for (const branch of branches) {
    const expected = await execute(ts, native, files, options, branch), observed = await execute(ts, actual, files, options, branch);
    assert.deepEqual(observed, expected, 'complete Native cyclic graph execution'); results.push({branch, observations: expected});
  }
  return {executions: branches.length * 2, results};
}

module.exports = {execute, run};
