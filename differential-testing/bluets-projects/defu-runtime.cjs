// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const path = require('node:path');
function run(directory) {
  const api = require(path.resolve(directory, 'defu.js'));
  const {isPlainObject} = require(path.resolve(directory, '_utils.js'));
  const {defu, createDefu, defuFn, defuArrayFn} = api;
  const passed = [];
  function scenario(name, action) { action(); passed.push(name); }
  scenario('priority-and-defaults', () => {
    assert.deepEqual(defu({port: 3000}, {port: 80, host: 'localhost'}), {port: 3000, host: 'localhost'});
    assert.equal(api.default, defu);
  });
  scenario('nullish-values', () => {
    assert.deepEqual(defu({a: null, b: undefined, c: false, d: 0}, {a: 1, b: 2, c: true, d: 9}), {a: 1, b: 2, c: false, d: 0});
  });
  scenario('array-concatenation', () => {
    assert.deepEqual(defu({items: [1, 2]}, {items: [3]}), {items: [1, 2, 3]});
  });
  scenario('nested-records', () => {
    assert.deepEqual(defu({nested: {a: 1}}, {nested: {a: 2, b: 3}}), {nested: {a: 1, b: 3}});
  });
  scenario('nonplain-values', () => {
    class Item {}
    const item = new Item(), date = new Date(0), pattern = /test/, fn = () => 1;
    const merged = defu({item, date, pattern, fn}, {item: {}, date: {}, pattern: {}, fn: {}});
    assert.equal(merged.item, item); assert.equal(merged.date, date);
    assert.equal(merged.pattern, pattern); assert.equal(merged.fn, fn);
  });
  scenario('custom-number-merger', () => {
    const custom = createDefu((object, key, value) => {
      if (typeof object[key] === 'number' && typeof value === 'number') {
        object[key] += value; return true;
      }
    });
    assert.deepEqual(custom({count: 2}, {count: 3}), {count: 5});
  });
  scenario('namespace-forwarding', () => {
    const seen = [];
    const custom = createDefu((object, key, value, namespace) => {
      seen.push([String(key), namespace]);
    });
    assert.deepEqual(custom({nested: {a: 1}}, {nested: {b: 2}}), {nested: {a: 1, b: 2}});
    assert(seen.some(([key, namespace]) => key === 'a' && namespace === 'nested'));
  });
  scenario('function-merger', () => {
    assert.deepEqual(defuFn({count: value => value + 2}, {count: 3}), {count: 5});
  });
  scenario('array-function-merger', () => {
    const scalar = () => 5;
    const result = defuArrayFn({items: values => values.map(x => x * 2), scalar}, {items: [1, 2], scalar: 3});
    assert.deepEqual(result.items, [2, 4]); assert.equal(result.scalar, scalar);
  });
  scenario('prototype-key-observations', () => {
    const input = JSON.parse('{"__proto__":{"blueicePolluted":true},"constructor":{"prototype":{"blueicePolluted":true}},"safe":1}');
    // The original reduction copies an inherited enumerable property after
    // Object.assign changes its intermediate prototype. Preserve that Native
    // observation; this scenario does not claim library input sanitization.
    const result = defu(input, {base: 2});
    assert.deepEqual(result, {base: 2, safe: 1, blueicePolluted: true});
    assert.equal(Object.hasOwn(result, '__proto__'), false);
    assert.equal(Object.hasOwn(result, 'constructor'), false);
    assert.equal(Object.prototype.blueicePolluted, undefined);
  });
  scenario('cross-module-plain-object-helper', () => {
    assert.equal(isPlainObject({}), true); assert.equal(isPlainObject(Object.create(null)), true);
    for (const value of [null, [], new Date(0), new (class Item {})(), {[Symbol.iterator]() {}}]) {
      assert.equal(isPlainObject(value), false);
    }
    assert.equal(isPlainObject({[Symbol.toStringTag]: 'Module'}), true);
    assert.equal(isPlainObject({[Symbol.toStringTag]: 'Other'}), false);
  });
  scenario('named-export-inventory', () => {
    assert.deepEqual(Object.keys(api).sort(), ['createDefu', 'default', 'defu', 'defuArrayFn', 'defuFn']);
  });
  return {count: passed.length, passed};
}
if (require.main === module) console.log(JSON.stringify(run(process.argv[2])));
module.exports = {run};
