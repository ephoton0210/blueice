// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Supplemental calls exercise both default branches and the promise boundary.
const assert = require('node:assert/strict'), vm = require('node:vm');
async function run(ts, native, actual, options, proof) {
  async function execute(code, expected) {
    const sandbox = {}, exports = {}, context = vm.createContext(sandbox);
    let result;
    if (options.module === 'ESNext') {
      const instance = new vm.SourceTextModule(code, { context, identifier: 'memory:///main.js' });
      await instance.link(name => assert.fail('unexpected dependency ' + name));
      await instance.evaluate(); assert.deepEqual(Object.keys(instance.namespace), ['result']);
      result = instance.namespace.result;
    } else {
      sandbox.exports = exports; sandbox.module = { exports };
      vm.runInContext(code, context, { timeout: 1000 }); result = exports.result;
    }
    try { const value = await result; assert(!Object.hasOwn(expected, 'throws'), 'a rejected promise was required'); assert.equal(value, expected.value); }
    catch (error) { if (!Object.hasOwn(expected, 'throws')) throw error; assert.equal(error, expected.throws); }
  }
  function fixture(view, item, argument, reject) {
    const edits = [[item.resultCall.getStart(view.source), item.resultCall.end, 'calculate(' + argument + ')']];
    if (reject) edits.push([item.awaitCall.expression.name.getStart(view.source), item.awaitCall.expression.name.end, 'reject']);
    let code = view.code;
    for (const [first, last, text] of edits.sort((a, b) => b[0] - a[0])) code = code.slice(0, first) + text + code.slice(last);
    return code;
  }
  const variants = [];
  for (const [mode, argument, expected, reject] of [
    ['original', null, { value: 42 }, false], ['provided', '7', { value: 9 }, false],
    ['zero', '0', { value: 2 }, false], ['null', 'null', { value: 2 }, false],
    ['undefined', 'void 0', { value: 42 }, false], ['string', '"4"', { value: '42' }, false],
    ['rejection-default', '', { throws: 2 }, true], ['rejection-provided', '7', { throws: 2 }, true],
  ]) {
    await execute(argument === null ? native.code : fixture(native, proof.native, argument, reject), expected);
    await execute(argument === null ? actual.code : fixture(actual, proof.actual, argument, reject), expected);
    variants.push({ mode, executions: 2, ...expected });
  }
  return { executions: 2, additionalFixtureExecutions: (variants.length - 1) * 2, variants };
}
module.exports = { run };
