// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A large module gap must not hide a mapped comment's logical endpoint.
const test = require('node:test'), assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const { stream, compareGap } = require('./ge5_role_stream.cjs'), { mutate } = require('./ge5_role_mutations.cjs');
const reference = JSON.parse(fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')));
for (const module of ['commonjs', 'esnext']) {
  const id = `ge1_downlevel/super-write-receiver-order-es2015-${module}-assign-iterate/crlfAstral/external`;
  const row = reference.cases.find(row => row.id === id); assert(row);
  const code = reference.blobs[row.outputs['out/main.js']], raw = reference.blobs[row.outputs['out/main.js.map']];
  const comment = '/* G-E5 🧭 */', end = code.indexOf(comment) + comment.length;
  const boundary = stream(ts, code, raw).tokens(ts.createSourceFile('fixture.js', code, ts.ScriptTarget.Latest, true)).find(token => token.start >= end).start;
  test(module + ': exact Native comment and following gap remain admissible', () => {
    compareGap(stream(ts, code, raw), 0, boundary, stream(ts, code, raw), 0, boundary, 'whole module entry');
  });
  for (const [label, owner] of [['retarget', [0, 0, 0]], ['erase', null]]) test(module + ': ' + label + ' comment endpoint cannot hide in entry-wide gap', () => {
    const native = stream(ts, code, raw), actual = mutate(ts, stream(ts, code, raw), end, owner);
    assert.throws(() => compareGap(native, 0, boundary, actual, 0, boundary, 'whole module entry'));
  });
}
