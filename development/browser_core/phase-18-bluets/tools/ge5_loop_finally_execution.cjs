// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Run actual iterators across normal, abrupt and previously unselected loop exits.
const assert = require('node:assert/strict');
const { execute } = require('./ge5_module_execution.cjs');
async function run(ts, native, actual, options, proof) {
  const profile = { format: 'plain', module: options.module, esm: options.module === 'ESNext' };
  const step = (value, done) => value === undefined ? { done } : { value, done };
  const variants = [];
  function fixture(view, trace, mode) {
    if (mode === 'original') return view;
    const statement = view.source.statements.at(-1);
    const expression = ts.isVariableStatement(statement) ? statement.declarationList.declarations[0].initializer : statement.expression.right;
    const bodies = {
      'return-body': 'return JSON.stringify([current.next(),current.return(8),current.next(),current.next()]);',
      'throw-body': 'var a=current.next(),b=current.throw(7),caught;try{current.next();}catch(error){caught=error;}return JSON.stringify([a,b,caught,current.next()]);',
      'return-finally': 'return JSON.stringify([current.next(),current.next(),current.return(8),current.next()]);',
      'throw-finally': 'var a=current.next(),b=current.next(),caught;try{current.throw(7);}catch(error){caught=error;}return JSON.stringify([a,b,caught,current.next()]);',
      'empty-loop': 'return JSON.stringify([current.next(),current.next()]);',
      'continue-exit': 'return JSON.stringify([current.next(),current.next(),current.next(),current.next(),current.next()]);',
      'no-break': 'return JSON.stringify([current.next(),current.next(),current.next(),current.next(),current.next(),current.next(),current.next()]);',
      'no-continue': 'return JSON.stringify([current.next(),current.next(),current.next(),current.next(),current.next(),current.next(),current.next()]);',
      'return-second-body': 'return JSON.stringify([current.next(),current.next(),current.next(),current.return(8),current.next()]);',
    };
    assert(bodies[mode]);
    const text = '(function(){var current=sequence();' + bodies[mode] + '})()';
    const edits = [[expression.getStart(view.source), expression.end, text]];
    const steps = view.source.statements.filter(ts.isVariableStatement).flatMap(statement => [...statement.declarationList.declarations])
      .find(declaration => declaration.name.getText() === 'steps');
    assert(steps && ts.isArrayLiteralExpression(steps.initializer));
    edits.push([steps.initializer.getStart(view.source), steps.initializer.end, '[]']);
    if (mode === 'empty-loop' || mode === 'continue-exit') {
      const bound = trace.events.find(event => event.kind === 'branch' && event.value.operator === '<').value.right.node;
      edits.push([bound.getStart(view.source), bound.end, mode === 'empty-loop' ? '0' : '2']);
    }
    if (mode === 'no-break' || mode === 'no-continue') {
      const number = mode === 'no-break' ? '2' : '1';
      const condition = trace.events.find(event => event.kind === 'branch' && event.value.operator === '===' && event.value.right.text === number).value.right.node;
      edits.push([condition.getStart(view.source), condition.end, '9']);
    }
    let code = view.code;
    for (const [from, to, replacement] of edits.sort((a, b) => b[0] - a[0])) code = code.slice(0, from) + replacement + code.slice(to);
    return { code };
  }
  const a = step(0, false), b = step(10, false), c = step(1, false), d = step(11, false);
  const e = step(2, false), f = step(12, false), done = step(undefined, true), result = step(99, true);
  const cases = [
    ['original', [a, b, c, d, e, f, result]],
    ['return-body', [a, b, step(8, true), done]],
    ['throw-body', [a, b, 7, done]],
    ['return-finally', [a, b, step(8, true), done]],
    ['throw-finally', [a, b, 7, done]],
    ['empty-loop', [result, done]],
    ['continue-exit', [a, b, c, d, result]],
    ['no-break', [a, b, c, d, e, f, result]],
    ['no-continue', [a, b, c, d, e, f, result]],
    ['return-second-body', [a, b, c, d, step(8, true)]],
  ];
  for (const [mode, steps] of cases) {
    const expected = JSON.stringify(steps);
    const n = await execute(ts, fixture(native, proof.native, mode), profile, 'default', expected);
    const a = await execute(ts, fixture(actual, proof.actual, mode), profile, 'default', expected);
    assert.equal(a, n); variants.push({ mode, executions: 2, result: expected });
  }
  return { executions: 2, additionalFixtureExecutions: 18, variants };
}
module.exports = { run };
