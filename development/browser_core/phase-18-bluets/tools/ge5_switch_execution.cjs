// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute actual CommonJS/ESM modules and each original switch completion path.
const assert = require('node:assert/strict');
const { execute } = require('./ge5_module_execution.cjs');
async function run(ts, native, actual, options) {
  const profile = { format: 'plain', module: options.module, esm: options.module === 'ESNext' };
  const step = (value, done) => value === undefined ? { done } : { value, done };
  const variants = [];
  function fixture(view, mode) {
    if (mode === 'original') return view;
    const statement = view.source.statements.at(-1);
    const expression = ts.isVariableStatement(statement) ? statement.declarationList.declarations[0].initializer : statement.expression.right;
    let body;
    if (mode === 'case-four') body = 'return JSON.stringify([first,current.next(4),current.next()]);';
    if (mode === 'default') body = 'return JSON.stringify([first,current.next(9),current.next()]);';
    if (mode === 'fallthrough-zero') body = 'return JSON.stringify([first,current.next(2),current.next(0),current.next()]);';
    if (mode === 'early-return') body = 'return JSON.stringify([first,current.return(8),current.next()]);';
    if (mode === 'external-throw') body = 'var caught;try{current.throw(7);}catch(error){caught=error;}return JSON.stringify([first,caught,current.next()]);';
    assert(body);
    const text = '(function(){var current=sequence();var first=current.next();' + body + '})()';
    return { code: view.code.slice(0, expression.getStart(view.source)) + text + view.code.slice(expression.end) };
  }
  for (const mode of ['original', 'case-four', 'default', 'fallthrough-zero', 'early-return', 'external-throw']) {
    const steps = mode === 'original' ? [step(1, false), step(3, false), step(9, true), step(undefined, true)]
      : mode === 'case-four' ? [step(1, false), step(4, true), step(undefined, true)]
      : mode === 'default' ? [step(1, false), step(9, true), step(undefined, true)]
      : mode === 'fallthrough-zero' ? [step(1, false), step(3, false), step(4, true), step(undefined, true)]
      : mode === 'early-return' ? [step(1, false), step(8, true), step(undefined, true)]
      : [step(1, false), 7, step(undefined, true)];
    const expected = JSON.stringify(steps);
    const n = await execute(ts, fixture(native, mode), profile, 'default', expected);
    const a = await execute(ts, fixture(actual, mode), profile, 'default', expected);
    assert.equal(a, n); variants.push({ mode, executions: 2, result: expected });
  }
  return { executions: 2, additionalFixtureExecutions: 10, variants };
}
module.exports = { run };
