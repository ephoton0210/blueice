// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs'), moduleRoles = require('./ge5_interop_module.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);
function fixture(index, filename, change = value => value) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const mapName = 'out/' + filename.replace(/\.ts$/, '.js.map'), map = expected.maps[mapName];
  const code = reference.blobs[expected.outputs[map.output]], raw = authority.raw(reference, expected, mapName);
  return moduleRoles.compare(ts, stream(ts, code, raw), stream(ts, change(code), raw), original.options,
    original.files, original.entry, filename, false);
}

test('the complete Native graph closes every module character and logical declaration endpoint', () => {
  for (const index of [4884, 4886, 4888, 4890, 4924, 4933, 4964, 4968, 5004, 5013, 5044, 5084, 5164]) {
    for (const filename of ['main.ts', 'dep.ts', 'left.ts', 'right.ts']) {
      try { assert.equal(fixture(index, filename).completeModuleValidated, true); }
      catch (error) { error.message = index + ' ' + filename + ': ' + error.message; throw error; }
    }
  }
});

test('a System star helper cannot invert its default exclusion', () => {
  assert.throws(() => fixture(5044, 'main.ts', code => {
    assert(code.includes('n !== "default"')); return code.replace('n !== "default"', 'n === "default"');
  }));
});

test('a function publication must preserve its exact original function', () => {
  assert.throws(() => fixture(4884, 'left.ts', code => {
    assert(code.includes('exports.read = read;')); return code.replace('exports.read = read;', 'exports.read = reed;');
  }));
});

test('interop report paths keep the exact full source selection isolated from partial products', () => {
  const {reportNames} = require('./verify_ge5_interop_module_acceptance.cjs');
  const fs = require('node:fs'), os = require('node:os');
  const selected = Array.from({length: 363}, (_, index) => 4884 + index);
  const full = reportNames(selected, selected), partial = reportNames([4892, 4933, 5013], selected);
  assert.equal(full.report, 'interop-complete-module-acceptance.json');
  assert.equal(full.progress, 'interop-module-acceptance-progress.jsonl');
  assert.equal(partial.report, 'interop-complete-module-acceptance-partial.json');
  assert.equal(partial.progress, 'interop-module-acceptance-partial-progress.jsonl');
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ge5-interop-report-path-'));
  try {
    for (const order of [[full, partial], [partial, full]]) {
      for (const names of order) {
        const count = names === full ? 363 : 3;
        fs.writeFileSync(path.join(directory, names.report), JSON.stringify({configurations: count}));
        fs.writeFileSync(path.join(directory, names.progress), String(count));
      }
      assert.equal(JSON.parse(fs.readFileSync(path.join(directory, full.report))).configurations, 363);
      assert.equal(JSON.parse(fs.readFileSync(path.join(directory, partial.report))).configurations, 3);
      assert.equal(fs.readFileSync(path.join(directory, full.progress), 'utf8'), '363');
      assert.equal(fs.readFileSync(path.join(directory, partial.progress), 'utf8'), '3');
    }
  } finally { fs.rmSync(directory, {recursive: true, force: true}); }
  assert.throws(() => reportNames([...selected.slice(0, -1), 99999], selected), /deep-equal/);
  assert.throws(() => reportNames([4892, 4933, 99999], selected), /deep-equal/);
  assert.throws(() => reportNames([4892], selected), /deep-equal/, 'no new single-selector acceptance');
  assert.throws(() => reportNames(selected.slice(0, 362), selected), /deep-equal/);
});
