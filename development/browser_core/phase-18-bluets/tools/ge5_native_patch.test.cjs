// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const { test } = require('node:test'), assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const { spawnSync } = require('node:child_process');
const graph = require('./ge5_inputs.cjs'), maps = require('./ge5_maps.cjs');
const patch = require('./ge5_native_patch.cjs'), compiler = require('./ge5_native_compile.cjs');
const root = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const filename = resolve(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc', 'typescript', 'lib/typescript.js');

test('the independent Native patch rejects a changed bundle before evaluating it', () => {
  const source = fs.readFileSync(filename, 'utf8');
  assert.equal(graph.sha256(source), patch.stockSha256);
  assert.throws(() => patch.patchSource(source + '\n'), /bundle hash/);
  assert.throws(() => patch.patchSource(patch.patchSource(source)), /bundle hash/);
});

test('stock reproduces EOF+1; the patched transform preserves every real statement mapping', () => {
  const input = JSON.parse(fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')));
  const reference = JSON.parse(fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')));
  const parents = graph.parents(root, input), stock = compiler(require(filename)), fixed = compiler(patch.load(filename));
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge5-native-regression-')));
  try {
    for (const variant of ['original', 'crlfAstral']) {
      const id = 'ge2_module_helpers/inherit-constructor-spread-system-es5-i0-n0/' + variant + '/external';
      const specification = input.cases.find(row => row.id === id), row = graph.resolve(input, specification, parents);
      const expected = reference.cases.find(row => row.id === id);
      const base = path.join(temporary, variant); fs.mkdirSync(base); graph.write(base, row.files);
      const before = stock(base, row), after = fixed(base, row);
      assert.deepEqual(stock(base, row), before); assert.deepEqual(fixed(base, row), after);
      for (const [name, hash] of Object.entries(expected.outputs)) assert.equal(before.outputs[name], reference.blobs[hash]);
      const fresh = spawnSync(process.execPath, [path.join(path.dirname(filename), 'tsc.js'), '-p', path.join(base, 'tsconfig.json')], { encoding: 'utf8' });
      assert.equal(fresh.status, 0, fresh.stdout + fresh.stderr);
      assert.equal(fs.readFileSync(path.join(base, 'out/main.js.map'), 'utf8'), before.outputs['out/main.js.map']);
      assert.deepEqual(after.diagnostics, before.diagnostics); assert.deepEqual(after.selectedInputs, before.selectedInputs);
      for (const [name, text] of Object.entries(before.outputs)) {
        if (name !== 'out/main.js.map') assert.equal(after.outputs[name], text, name);
      }
      const original = maps.decode(before.outputs['out/main.js.map'], before.outputs['out/main.js'], { recordInvalidCoordinates: true });
      assert.equal(original.coordinateIssues.length, 1); assert.throws(() => maps.decode(before.outputs['out/main.js.map'], before.outputs['out/main.js']), /UTF-16 source bounds/);
      const corrected = maps.decode(after.outputs['out/main.js.map'], after.outputs['out/main.js']);
      assert.deepEqual(corrected.metadata, original.metadata);
      assert.deepEqual(corrected.segments, original.segments.slice(0, -2));
      assert.deepEqual(original.segments.slice(-2).map(segment => segment.slice(1)), [[8, 0, 3, 0], [9, 0, 3, 1]]);
      assert.equal(corrected.segments.some(segment => segment[0] === original.coordinateIssues[0].segment[0]), false);
    }
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
});

test('synthetic brace suppression covers EOF variants and leaves other module transforms byte-identical', () => {
  const stockModule = require(filename), stock = compiler(stockModule), fixedModule = patch.load(filename);
  assert.equal(require(filename), stockModule); assert.notEqual(stockModule, fixedModule);
  const fixed = compiler(fixedModule);
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge5-native-eof-')));
  try {
    for (const [index, source] of [
      'export const value: number = 42;',
      'export const value: number = 42;\n',
      '/* 🧭 */ export const value: number = 42;\r\n',
      'export const value: number = 42;\n// trailing comment\n',
      'export interface Item { value: number; }\n',
      'export function read(): number { return 42; }\n',
    ].entries()) {
      for (const module of ['System', 'CommonJS', 'ESNext']) {
        const base = path.join(temporary, index + '-' + module); fs.mkdirSync(base);
        const row = { entry: 'main.ts', options: { target: 'ES2022', module, types: [], lib: ['ES2022'],
          strict: true, declaration: true, declarationMap: true, sourceMap: true, inlineSources: true, outDir: 'out' } };
        fs.writeFileSync(path.join(base, 'main.ts'), source);
        const before = stock(base, row), after = fixed(base, row);
        assert.deepEqual(after.diagnostics, []); assert.deepEqual(before.diagnostics, []);
        maps.artifacts(after.outputs);
        if (module !== 'System') assert.deepEqual(after, before);
        else for (const [name, text] of Object.entries(before.outputs)) {
          if (name !== 'out/main.js.map') assert.equal(after.outputs[name], text, name);
        }
      }
    }
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
});
