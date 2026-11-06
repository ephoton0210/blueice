// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Real class modules preserve dynamic receivers, argument flow and evaluation.
const assert = require('node:assert/strict'), vm = require('node:vm');
async function run(ts, native, actual, options, proof) {
  const variants = [
    {mode: 'original', value: 42},
    {mode: 'provided', call: 'new Box().read(7)', value: 47},
    {mode: 'zero', call: 'new Box().read(0)', value: 40},
    {mode: 'null', call: 'new Box().read(null)', value: 40},
    {mode: 'undefined', call: 'new Box().read(void 0)', nan: true},
    {mode: 'string', call: 'new Box().read("4")', value: '404'},
    {mode: 'borrowed-receiver', call: 'Box.prototype.read.call({value:9},3)', value: 12},
    {mode: 'apply-arguments', call: 'Box.prototype.read.apply({value:9},[3,99])', value: 12},
    {mode: 'bound-receiver', call: 'Box.prototype.read.bind({value:9})(3)', value: 12},
    {mode: 'detached-rejects', call: '(0,Box.prototype.read)(2)', throws: true, typeError: true},
    {mode: 'getter-before-await', call: 'Box.prototype.read.call({get value(){globalThis.__methodReads.push("get");return 40;}},2)', order: true, value: 42},
    {mode: 'left-read-before-await-mutation', call: 'Box.prototype.read.call(globalThis.__methodReceiver,2)', mutate: true, value: 42},
    {mode: 'receiver-getter-throws', call: 'Box.prototype.read.call({get value(){throw 7;}},2)', throws: 7},
    {mode: 'await-rejects', reject: true, throws: 2},
    {mode: 'original-runtime-arity', call: 'Promise.resolve(Box.prototype.read.length)', value: 1},
  ];
  async function execute(view, item, variant) {
    const reads = [], exports = {}, sandbox = {__methodReads: reads, __methodReceiver: {value: 40}}, context = vm.createContext(sandbox);
    const edits = [], start = node => node.getStart(view.source), awaited = item.awaitCall;
    if (variant.call) edits.push([start(item.resultCall), item.resultCall.end, variant.call]);
    if (variant.reject) edits.push([start(awaited.expression.name), awaited.expression.name.end, 'reject']);
    if (variant.order) edits.push([start(awaited.arguments[0]), awaited.arguments[0].end, '(globalThis.__methodReads.push("await"),extra)']);
    if (variant.mutate) edits.push([start(awaited.arguments[0]), awaited.arguments[0].end, '(globalThis.__methodReceiver.value=100,extra)']);
    let code = view.code;
    for (const [from, to, text] of edits.sort((a, b) => b[0] - a[0])) code = code.slice(0, from) + text + code.slice(to);
    let result;
    if (options.module === 'ESNext') {
      const module = new vm.SourceTextModule(code, {context, identifier: 'memory:///main.js'});
      await module.link(name => assert.fail('unexpected class method dependency ' + name)); await module.evaluate();
      assert.deepEqual(Object.keys(module.namespace), ['Box', 'result']); assert.equal(typeof module.namespace.Box, 'function'); result = module.namespace.result;
    } else {
      sandbox.exports = exports; sandbox.module = {exports}; vm.runInContext(code, context, {timeout: 1000});
      assert.deepEqual(Object.keys(exports).sort(), ['Box', 'result']); assert.equal(typeof exports.Box, 'function'); result = exports.result;
    }
    assert(result && typeof result.then === 'function', 'class method completion is a promise');
    let rejected = false, value;
    try {value = await result;} catch (error) {rejected = true; value = error;}
    assert.equal(rejected, Object.hasOwn(variant, 'throws'), variant.mode + ': promise completion');
    if (variant.typeError) assert.equal(value?.name, 'TypeError', variant.mode);
    else if (variant.nan) assert(Number.isNaN(value), variant.mode);
    else assert.equal(value, rejected ? variant.throws : variant.value, variant.mode);
    assert.deepEqual(reads, variant.order ? ['get', 'await'] : [], variant.mode + ': exact getter/order trace');
  }
  for (const variant of variants) {
    await execute(native, proof.native, variant); await execute(actual, proof.actual, variant);
  }
  return {executions: 2, additionalFixtureExecutions: (variants.length - 1) * 2,
    variants: variants.map(variant => ({mode: variant.mode, executions: 2}))};
}
module.exports = {run};
