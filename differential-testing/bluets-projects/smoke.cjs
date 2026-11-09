// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const mitt = require(process.argv[2]).default;
const passed = [];
function check(name, run) { run(); passed.push(name); }

check('typed event delivery', () => {
  const emitter = mitt(), values = [];
  emitter.on('count', value => values.push(value));
  emitter.emit('count', 42);
  assert.deepEqual(values, [42]);
});
check('wildcard follows named handlers', () => {
  const emitter = mitt(), values = [];
  emitter.on('*', (type, value) => values.push([type, value]));
  emitter.on('text', value => values.push(value));
  emitter.emit('text', 'hello');
  assert.deepEqual(values, ['hello', ['text', 'hello']]);
});
check('remove one exact handler', () => {
  const emitter = mitt(), values = [], handler = value => values.push(value);
  emitter.on('count', handler); emitter.off('count', handler); emitter.emit('count', 1);
  assert.deepEqual(values, []);
});
check('remove all handlers for one event', () => {
  const emitter = mitt(), values = [];
  emitter.on('count', value => values.push(value));
  emitter.off('count'); emitter.emit('count', 1);
  assert.deepEqual(values, []);
});
check('symbol event identity', () => {
  const emitter = mitt(), event = Symbol('event'), values = [];
  emitter.on(event, value => values.push(value));
  emitter.emit(Symbol('event'), 0); emitter.emit(event, 7);
  assert.deepEqual(values, [7]);
});
check('caller supplied handler map', () => {
  const values = [], all = new Map([['count', [value => values.push(value)]]]), emitter = mitt(all);
  assert.equal(emitter.all, all); emitter.emit('count', 3);
  assert.deepEqual(values, [3]);
});
check('handler removal preserves the current dispatch snapshot', () => {
  const emitter = mitt(), values = [], second = () => values.push('second');
  emitter.on('event', () => { values.push('first'); emitter.off('event', second); });
  emitter.on('event', second); emitter.emit('event'); emitter.emit('event');
  assert.deepEqual(values, ['first', 'second', 'first']);
});
check('clear releases all registrations', () => {
  const emitter = mitt(), values = [];
  emitter.on('*', () => values.push('unexpected')); emitter.all.clear(); emitter.emit('event');
  assert.deepEqual(values, []); assert.equal(emitter.all.size, 0);
});
console.log(JSON.stringify({passed, count: passed.length}));
