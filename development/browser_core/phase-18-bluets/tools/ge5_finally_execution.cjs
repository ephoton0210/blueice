// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Real modules distinguish pending completions from abrupt exits inside finally.
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
    if (mode === 'normal') body = 'return JSON.stringify([current.next(),current.next(),current.next()]);';
    if (mode === 'throw-before-finally') body = 'return JSON.stringify([current.next(),current.throw(7),current.next()]);';
    if (mode === 'return-inside-finally') body = 'return JSON.stringify([current.next(),current.next(),current.return(9),current.next()]);';
    if (mode === 'throw-inside-finally') body = 'var first=current.next(),second=current.next(),caught;try{current.throw(7);}catch(error){caught=error;}return JSON.stringify([first,second,caught,current.next()]);';
    if (mode === 'return-before-start') body = 'return JSON.stringify([current.return(8),current.next()]);';
    if (mode === 'throw-before-start') body = 'var caught;try{current.throw(7);}catch(error){caught=error;}return JSON.stringify([caught,current.next()]);';
    if (mode === 'completed') body = 'return JSON.stringify([current.next(),current.next(),current.next(),current.return(8),current.next()]);';
    assert(body);
    const text = '(function(){var current=sequence();' + body + '})()';
    return { code: view.code.slice(0, expression.getStart(view.source)) + text + view.code.slice(expression.end) };
  }
  for (const mode of ['original', 'normal', 'throw-before-finally', 'return-inside-finally', 'throw-inside-finally',
    'return-before-start', 'throw-before-start', 'completed']) {
    const prefix = [step(1, false), step(3, false)];
    const steps = ['original', 'normal', 'throw-before-finally'].includes(mode) ? [...prefix, step(4, true)]
      : mode === 'return-inside-finally' ? [...prefix, step(9, true), step(undefined, true)]
      : mode === 'throw-inside-finally' ? [...prefix, 7, step(undefined, true)]
      : mode === 'return-before-start' ? [step(8, true), step(undefined, true)]
      : mode === 'throw-before-start' ? [7, step(undefined, true)]
      : [...prefix, step(4, true), step(8, true), step(undefined, true)];
    const expected = JSON.stringify(steps);
    const n = await execute(ts, fixture(native, mode), profile, 'default', expected);
    const a = await execute(ts, fixture(actual, mode), profile, 'default', expected);
    assert.equal(a, n); variants.push({ mode, executions: 2, result: expected });
  }
  return { executions: 2, additionalFixtureExecutions: 14, variants };
}
module.exports = { run };
