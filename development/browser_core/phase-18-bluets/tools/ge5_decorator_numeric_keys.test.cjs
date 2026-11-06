// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const numeric = require('./ge5_decorator_numeric_keys.cjs');
const original = 'class C { @dec 0x10() { return 2; } }';
function read(code, input = original, kind = 'decorate') {
  const source = ts.createSourceFile('emitted.js', code, 99, true);
  const node = source.statements[0].expression;
  return numeric.read(ts, {source}, [input], [{node, kind}]);
}
test('numeric helper-key spelling is authorized by one original method identity', () => {
  const left = read('__decorate([dec], C.prototype, 16, null);');
  const right = read('__decorate([dec], C.prototype, 0x10, null);');
  assert.equal(left.length, 1); assert.equal(right.length, 1);
  assert.equal(left[0].text, right[0].text); assert(left[0].numericMethodKey);
});
test('wrong numeric key and static phase cannot borrow an original method', () => {
  assert.throws(() => read('__decorate([dec], C.prototype, 17, null);'), /one decorated original method/);
  assert.throws(() => read('__decorate([dec], C, 16, null);'), /one decorated original method/);
});
test('a different receiver cannot borrow the original class', () => {
  assert.throws(() => read('__decorate([dec], X.prototype, 16, null);'), /unique original class/);
});
test('numeric methods retain descriptor acquisition and verified helper kind', () => {
  assert.throws(() => read('__decorate([dec], C.prototype, 16, {});'), /descriptor acquisition/);
  assert.deepEqual(read('__other([dec], C.prototype, 16, null);', original, 'other'), []);
});
test('numeric expressions and non-method source keys receive no numeric method role', () => {
  assert.deepEqual(read('__decorate([dec], C.prototype, 8 + 8, null);'), []);
  assert.deepEqual(read('__decorate([dec], C.prototype, 16, null);', 'class C { @dec get 0x10(){return 2;} }'), []);
});
