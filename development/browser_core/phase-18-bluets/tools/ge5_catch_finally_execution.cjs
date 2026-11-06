// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exercise real catch/finally modules, including abrupt completions at each yield.
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
      'return-after-first': 'var steps=[current.next(),current.return(8),current.next(),current.next()];',
      'throw-after-first': 'var steps=[current.next(),current.throw(7),current.next(),current.next()];',
      'return-in-catch': 'var steps=[current.next(),current.next(),current.return(8),current.next()];',
      'throw-in-catch': 'var a=current.next(),b=current.next(),c=current.throw(7),caught;try{current.next();}catch(error){caught=error;}var steps=[a,b,c,caught,current.next()];',
      'return-in-finally': 'var steps=[current.next(),current.next(),current.next(),current.return(8),current.next()];',
      'throw-in-finally': 'var a=current.next(),b=current.next(),c=current.next(),caught;try{current.throw(7);}catch(error){caught=error;}var steps=[a,b,c,caught,current.next()];',
      'return-before-start': 'var steps=[current.return(8),current.next()];',
      'throw-before-start': 'var caught;try{current.throw(7);}catch(error){caught=error;}var steps=[caught,current.next()];',
      'completed': 'var steps=[current.next(),current.next(),current.next(),current.next(),current.return(8),current.next()];',
      'default-return': 'var steps=[current.next(),current.next(),current.next()];',
    };
    assert(bodies[mode]);
    const text = '(function(){var current=sequence();' + bodies[mode] + 'return JSON.stringify([steps,log]);})()';
    const edits = [[expression.getStart(view.source), expression.end, text]];
    const originalSteps = view.source.statements.filter(ts.isVariableStatement)
      .flatMap(statement => [...statement.declarationList.declarations]).find(declaration => declaration.name.getText() === 'steps');
    assert(originalSteps && ts.isArrayLiteralExpression(originalSteps.initializer));
    edits.push([originalSteps.initializer.getStart(view.source), originalSteps.initializer.end, '[]']);
    if (mode === 'default-return') {
      const selector = trace.events.find(event => event.kind === 'case').value.node;
      assert(ts.isNumericLiteral(selector) && selector.text === '1');
      edits.push([selector.getStart(view.source), selector.end, '0']);
    }
    let code = view.code;
    for (const [from, to, replacement] of edits.sort((a, b) => b[0] - a[0])) code = code.slice(0, from) + replacement + code.slice(to);
    return { code };
  }
  const a = step(1, false), b = step(3, false), c = step(4, false), d = step(5, true), done = step(undefined, true);
  const cases = [
    ['original', [a, b, c, d], ['2', 'finally']],
    ['return-after-first', [a, c, step(8, true), done], ['finally']],
    ['throw-after-first', [a, b, c, d], ['7', 'finally']],
    ['return-in-catch', [a, b, c, step(8, true)], ['2', 'finally']],
    ['throw-in-catch', [a, b, c, 7, done], ['2', 'finally']],
    ['return-in-finally', [a, b, c, step(8, true), done], ['2', 'finally']],
    ['throw-in-finally', [a, b, c, 7, done], ['2', 'finally']],
    ['return-before-start', [step(8, true), done], []],
    ['throw-before-start', [7, done], []],
    ['completed', [a, b, c, d, step(8, true), done], ['2', 'finally']],
    ['default-return', [c, step(9, true), done], ['finally']],
  ];
  for (const [mode, steps, log] of cases) {
    const expected = JSON.stringify([steps, log]);
    const n = await execute(ts, fixture(native, proof.native, mode), profile, 'default', expected);
    const actualResult = await execute(ts, fixture(actual, proof.actual, mode), profile, 'default', expected);
    assert.equal(actualResult, n); variants.push({ mode, executions: 2, result: expected });
  }
  return { executions: 2, additionalFixtureExecutions: 20, variants };
}
module.exports = { run };
