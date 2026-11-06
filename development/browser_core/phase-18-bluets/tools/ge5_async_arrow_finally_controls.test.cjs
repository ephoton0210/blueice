// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const {stream} = require('./ge5_role_stream.cjs');
const runners = [['async arrow', require('./ge5_async_arrow_finally_mutations.cjs')],
  ['async generator', require('./ge5_async_generator_mutations.cjs')]];

function fixture() {
  const code = 'function compute(){Promise.resolve(1);events.push(1);compute(1);Promise.resolve(2);}const arrow=()=>{};';
  const raw = JSON.stringify({version: 3, file: 'main.js', sources: ['main.ts'],
    sourcesContent: [code], names: [], mappings: 'AAAA'});
  const native = stream(ts, code, raw), actual = stream(ts, code, raw);
  native.cover(0, code.length);
  actual.cover(0, code.length);
  return {native, actual, code, proof: {parts: {actual: {
    compute: actual.source.statements[0],
    expression: actual.source.statements[1].declarationList.declarations[0].initializer,
  }}}};
}

for (const [kind, controls] of runners) {
test(kind + ': previously classified streams cannot supply coverage to a fresh comparison', () => {
  const {native, actual, code, proof} = fixture(), stop = new Error('fresh baseline reached');
  let calls = 0;
  assert.throws(() => controls.run(ts, native, actual, {}, code, proof, (_ts, left, right) => {
    calls++;
    assert.notEqual(left, native, 'Native AST and coverage are recreated');
    assert.notEqual(right, actual, 'actual AST and coverage are recreated');
    assert.equal(left.covered(0), false);
    assert.equal(right.covered(0), false);
    throw stop;
  }), error => error === stop);
  assert.equal(calls, 1);
});

test(kind + ': an unchanged baseline failure propagates before rejected mutations are counted', () => {
  const {native, actual, code, proof} = fixture(), failure = new Error('unchanged artifact rejected');
  let calls = 0;
  assert.throws(() => controls.run(ts, native, actual, {}, code, proof, () => {
    calls++;
    throw failure;
  }), error => error === failure);
  assert.equal(calls, 1, 'failing controls cannot conceal an invalid baseline');
});

}
