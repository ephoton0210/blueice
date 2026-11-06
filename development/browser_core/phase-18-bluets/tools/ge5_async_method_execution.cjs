// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute real modules with alternate dynamic receivers and abrupt completions.
const assert = require('node:assert/strict'), vm = require('node:vm');
async function run(ts, native, actual, options, proof) {
  const variants = [];
  async function execute(view, item, variant) {
    const reads = [], sandbox = {__methodReads: reads}, context = vm.createContext(sandbox), exports = {};
    const edits = [], start = node => node.getStart(view.source);
    if (variant.call) edits.push([start(item.resultCall), item.resultCall.end, variant.call]);
    const calls = [];
    function visit(node) {
      if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
        && node.expression.expression.getText(view.source) === 'Promise') calls.push(node);
      ts.forEachChild(node, visit);
    }
    visit(item.callback.body); assert.equal(calls.length, 1); const awaited = calls[0];
    if (variant.reject) edits.push([start(awaited.expression.name), awaited.expression.name.end, 'reject']);
    if (variant.order) {
      edits.push([start(item.value), item.value.end, 'get value(){globalThis.__methodReads.push("get");return 40;}']);
      edits.push([start(awaited.arguments[0]), awaited.arguments[0].end, '(globalThis.__methodReads.push("await"),extra)']);
    }
    if (variant.mutate) edits.push([start(awaited.arguments[0]), awaited.arguments[0].end, '(box.value=100,extra)']);
    let code = view.code;
    for (const [first, last, text] of edits.sort((a, b) => b[0] - a[0])) code = code.slice(0, first) + text + code.slice(last);
    let result;
    if (options.module === 'ESNext') {
      const instance = new vm.SourceTextModule(code, {context, identifier: 'memory:///main.js'});
      await instance.link(name => assert.fail('unexpected method dependency ' + name));
      await instance.evaluate(); assert.deepEqual(Object.keys(instance.namespace), ['result']); result = instance.namespace.result;
    } else {
      sandbox.exports = exports; sandbox.module = {exports};
      vm.runInContext(code, context, {timeout: 1000}); result = exports.result;
    }
    let rejected = false, value;
    try { value = await result; } catch (error) { rejected = true; value = error; }
    assert.equal(rejected, Object.hasOwn(variant, 'throws'), variant.mode + ': promise completion kind');
    if (variant.typeError) assert.equal(value?.name, 'TypeError', variant.mode);
    else if (variant.nan) assert(Number.isNaN(value), variant.mode);
    else assert.equal(value, rejected ? variant.throws : variant.value, variant.mode);
    assert.deepEqual(reads, variant.order ? ['get', 'await'] : [], variant.mode + ': exact getter/order trace');
  }
  for (const variant of [
    {mode: 'original', value: 42}, {mode: 'provided', call: 'box.read(7)', value: 47},
    {mode: 'zero', call: 'box.read(0)', value: 40}, {mode: 'null', call: 'box.read(null)', value: 40},
    {mode: 'undefined', call: 'box.read(void 0)', nan: true},
    {mode: 'string', call: 'box.read("4")', value: '404'},
    {mode: 'borrowed-receiver', call: 'box.read.call({value:9},3)', value: 12},
    {mode: 'apply-arguments', call: 'box.read.apply({value:9},[3,99])', value: 12},
    {mode: 'bound-receiver', call: 'box.read.bind({value:9})(3)', value: 12},
    {mode: 'detached-rejects', call: '(0,box.read)(2)', throws: true, typeError: true},
    {mode: 'getter-before-await', order: true, value: 42},
    {mode: 'left-read-before-await-mutation', mutate: true, value: 42},
    {mode: 'receiver-getter-throws', call: 'box.read.call({get value(){throw 7;}},2)', throws: 7},
    {mode: 'await-rejects', reject: true, throws: 2},
  ]) {
    await execute(native, proof.native, variant); await execute(actual, proof.actual, variant);
    variants.push({mode: variant.mode, executions: 2});
  }
  return {executions: 2, additionalFixtureExecutions: (variants.length - 1) * 2, variants};
}
module.exports = {run};
