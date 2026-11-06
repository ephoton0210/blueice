// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute real derived methods with late super lookup and dynamic receivers.
const assert = require('node:assert/strict'), vm = require('node:vm');
async function run(ts, native, actual, options, proof) {
  const getter = 'Object.defineProperty(Base.prototype,"read",{configurable:true,get:function(){globalThis.__superTrace.push("get");return function(){globalThis.__superTrace.push("call");return this.seed;};}});';
  const variants = [
    {mode: 'original', value: 42},
    {mode: 'await-zero', operand: '0', value: 40},
    {mode: 'await-string', operand: '"4"', value: '404'},
    {mode: 'borrowed-receiver', setup: 'Base.prototype.read=function(){return this.seed;};', call: 'Derived.prototype.readAsync.call({seed:9})', value: 11},
    {mode: 'apply-arguments', setup: 'Base.prototype.read=function(){return this.seed;};', call: 'Derived.prototype.readAsync.apply({seed:9},[3,99])', value: 11},
    {mode: 'bound-receiver', setup: 'Base.prototype.read=function(){return this.seed;};', call: 'Derived.prototype.readAsync.bind({seed:13})()', value: 15},
    {mode: 'detached-constant-base-method', call: '(0,Derived.prototype.readAsync)()', value: 42},
    {mode: 'getter-and-invocation-after-await', setup: getter, call: 'Derived.prototype.readAsync.call({seed:9})', operand: '(globalThis.__superTrace.push("await"),2)', trace: ['await', 'get', 'call'], value: 11},
    {mode: 'base-method-replaced-while-suspended', call: '(function(){var result=new Derived().readAsync();Base.prototype.read=function(){return 80;};return result;})()', value: 82},
    {mode: 'base-getter-replaced-while-suspended', call: '(function(){var result=Derived.prototype.readAsync.call({seed:9});' + getter + 'return result;})()', trace: ['get', 'call'], value: 11},
    {mode: 'base-getter-rejects', setup: 'Object.defineProperty(Base.prototype,"read",{get:function(){throw 7;}});', throws: 7},
    {mode: 'base-method-rejects', setup: 'Base.prototype.read=function(){throw 8;};', throws: 8},
    {mode: 'await-rejects-before-super-getter', setup: getter, reject: true, throws: 2},
    {mode: 'original-runtime-arity', call: 'Promise.resolve(Derived.prototype.readAsync.length)', value: 0},
  ];
  async function execute(view, item, variant) {
    const trace = [], sandbox = {__superTrace: trace}, context = vm.createContext(sandbox), exports = {}, edits = [], start = node => node.getStart(view.source);
    if (variant.call || variant.setup) {
      let call = variant.call || item.resultCall.getText(view.source);
      if (variant.setup) call = '(function(){' + variant.setup + 'return ' + call + ';})()';
      edits.push([start(item.resultCall), item.resultCall.end, call]);
    }
    if (variant.reject) edits.push([start(item.awaitCall.expression.name), item.awaitCall.expression.name.end, 'reject']);
    if (variant.operand) edits.push([start(item.awaitCall.arguments[0]), item.awaitCall.arguments[0].end, variant.operand]);
    let code = view.code; for (const [from, to, text] of edits.sort((a, b) => b[0] - a[0])) code = code.slice(0, from) + text + code.slice(to);
    let result;
    if (options.module === 'ESNext') {
      const module = new vm.SourceTextModule(code, {context, identifier: 'memory:///main.js'});
      await module.link(name => assert.fail('unexpected super method dependency ' + name)); await module.evaluate();
      assert.deepEqual(Object.keys(module.namespace), ['Derived', 'result']); result = module.namespace.result;
    } else {
      sandbox.exports = exports; sandbox.module = {exports}; vm.runInContext(code, context, {timeout: 1000});
      assert.deepEqual(Object.keys(exports).sort(), ['Derived', 'result']); result = exports.result;
    }
    assert(result && typeof result.then === 'function'); let rejected = false, value;
    try {value = await result;} catch (error) {rejected = true; value = error;}
    assert.equal(rejected, Object.hasOwn(variant, 'throws'), variant.mode + ': promise completion');
    assert.equal(value, rejected ? variant.throws : variant.value, variant.mode);
    assert.deepEqual(trace, variant.trace || [], variant.mode + ': getter/call timing');
  }
  for (const variant of variants) {await execute(native, proof.native, variant); await execute(actual, proof.actual, variant);}
  return {executions: 2, additionalFixtureExecutions: (variants.length - 1) * 2,
    variants: variants.map(variant => ({mode: variant.mode, executions: 2}))};
}
module.exports = {run};
