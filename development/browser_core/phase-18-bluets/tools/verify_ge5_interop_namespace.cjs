// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exercise the emitted public namespace import with an explicit dependency.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const native = ts.transpileModule(input.source, {
  fileName: 'main.ts',
  compilerOptions: {target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, esModuleInterop: true},
}).outputText;

const profiles = [
  ['hidden-own', () => Object.defineProperty({value: 3}, 'hidden', {value: 9})],
  ['callable-own', () => Object.defineProperty(function dependency() {}, 'hidden', {value: 9})],
  ['own-versus-inherited', () => Object.assign(Object.create({inherited: 7}), {value: 3})],
  ['mutable-own', () => ({value: 3})],
  ['readonly-own', () => Object.defineProperty({}, 'value', {value: 3, enumerable: false})],
  ['accessor-own', () => Object.defineProperty({}, 'value', {get() { return 3; }, configurable: true})],
  ['already-module', () => ({__esModule: true, value: 3})],
  ['null', () => null],
  ['primitive-number', () => 7],
  ['primitive-string', () => 'ab'],
];

function observe(code, make, mode) {
  const dependency = make();
  const context = vm.createContext({exports: {}, require(specifier) {
    assert.equal(specifier, './dep.js', 'only the declared dependency is available');
    return dependency;
  }});
  if (mode.includes('withoutNames')) vm.runInContext('Object.getOwnPropertyNames = undefined;', context);
  if (mode.includes('withoutCreate')) vm.runInContext('Object.create = undefined;', context);
  vm.runInContext(code, context, {timeout: 3000});
  const namespace = context.exports.imported;
  const keys = Object.getOwnPropertyNames(namespace);
  const properties = keys.map(key => {
    const descriptor = Object.getOwnPropertyDescriptor(namespace, key);
    return {key, enumerable: descriptor.enumerable, configurable: descriptor.configurable,
      writable: descriptor.writable ?? null, getter: typeof descriptor.get === 'function',
      sameValue: namespace[key] === Object(dependency)[key]};
  });
  const before = namespace.value;
  if (dependency && Object.getOwnPropertyDescriptor(dependency, 'value')?.writable) dependency.value = 8;
  return {keys, properties, sameNamespace: namespace === dependency,
    defaultIsDependency: namespace.default === dependency, before: before ?? null,
    after: namespace.value ?? null, hidden: namespace.hidden ?? null,
    inherited: namespace.inherited ?? null};
}

const failures = [];
const modes = ['modern', 'withoutNames', 'withoutCreate', 'withoutNames-withoutCreate'];
for (const mode of modes) for (const [profile, make] of profiles) {
  const wanted = observe(native, make, mode), actual = observe(input.javascript, make, mode);
  try { assert.deepEqual(actual, wanted, profile); }
  catch (error) { failures.push({profile, mode, native: wanted, bluets: actual, message: error.message}); }
}
console.log(JSON.stringify({profiles: profiles.length, modes, comparisons: profiles.length * modes.length, failures}, null, 2));
assert.equal(failures.length, 0, 'all original namespace import property profiles match pinned Native');
