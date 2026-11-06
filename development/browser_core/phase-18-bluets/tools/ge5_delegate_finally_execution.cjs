// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exercise pending completions across both the delegate and outer finalizers.
const assert = require('node:assert/strict');
const { execute } = require('./ge5_module_execution.cjs');
async function run(ts, native, actual, options) {
  const profile = { format: 'plain', module: options.module, esm: options.module === 'ESNext' };
  const step = (value, done = false) => value === undefined ? { done } : { value, done };
  const prefix = [step(1), step(3)], log = ['child', 'outer'];
  const variants = [
    ['original', null, [...prefix, step(4), step(8, true)], log],
    ['normal', 's.push(g.next(),g.next(),g.next(),g.next());', [...prefix, step(4), step(2, true)], log],
    ['throw-before-child-finally', 's.push(g.next(),g.throw(7),g.next());try{g.next();}catch(e){s.push(e);}s.push(g.next());', [...prefix, step(4), 7, step(undefined, true)], log],
    ['return-inside-child-finally', 's.push(g.next(),g.next(),g.return(9),g.next());', [...prefix, step(4), step(9, true)], log],
    ['throw-inside-child-finally', 's.push(g.next(),g.next(),g.throw(7));try{g.next();}catch(e){s.push(e);}s.push(g.next());', [...prefix, step(4), 7, step(undefined, true)], log],
    ['return-inside-outer-finally', 's.push(g.next(),g.next(),g.next(),g.return(9),g.next());', [...prefix, step(4), step(9, true), step(undefined, true)], log],
    ['throw-inside-outer-finally', 's.push(g.next(),g.next(),g.next());try{g.throw(7);}catch(e){s.push(e);}s.push(g.next());', [...prefix, step(4), 7, step(undefined, true)], log],
    ['return-before-start', 's.push(g.return(8),g.next());', [step(8, true), step(undefined, true)], []],
    ['throw-before-start', 'try{g.throw(7);}catch(e){s.push(e);}s.push(g.next());', [7, step(undefined, true)], []],
    ['completed', 's.push(g.next(),g.next(),g.next(),g.next(),g.return(8),g.next());', [...prefix, step(4), step(2, true), step(8, true), step(undefined, true)], log],
  ];
  function fixture(view, body) {
    if (body === null) return view;
    let steps;
    for (const statement of view.source.statements) if (ts.isVariableStatement(statement)) {
      steps = statement.declarationList.declarations.find(node => node.name.getText() === 'steps') || steps;
    }
    assert(steps?.initializer, 'the original probe must be suppressed in each supplemental fixture');
    const last = view.source.statements.at(-1);
    const result = ts.isVariableStatement(last) ? last.declarationList.declarations[0].initializer : last.expression.right;
    assert(result);
    const replacements = [
      [steps.initializer, '[]'],
      [result, '(function(){var g=sequence(),s=[];' + body + 'return JSON.stringify([s,log]);})()'],
    ].sort(([a], [b]) => b.pos - a.pos);
    let code = view.code;
    for (const [node, text] of replacements) code = code.slice(0, node.getStart(view.source)) + text + code.slice(node.end);
    return { code };
  }
  const results = [];
  for (const [mode, body, steps, effects] of variants) {
    const expected = JSON.stringify([steps, effects]);
    const n = await execute(ts, fixture(native, body), profile, 'default', expected);
    const a = await execute(ts, fixture(actual, body), profile, 'default', expected);
    assert.equal(a, n); results.push({ mode, executions: 2, result: expected });
  }
  return { executions: 2, additionalFixtureExecutions: (results.length - 1) * 2, variants: results };
}
module.exports = { run };
