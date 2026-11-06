// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Observe the pinned closure-versus-intrinsic iterator receiver ABI independently.
const assert = require('node:assert/strict');
const execution = require('./ge5_class_execution.cjs');

function replaceResult(ts, outputs, replacement) {
  const code = outputs['out/main.js'];
  const source = ts.createSourceFile('runtime.js', code, ts.ScriptTarget.Latest, true);
  const roots = new Set();
  function visit(node) {
    if (ts.isPropertyAccessExpression(node) && node.name.text === 'value'
      && ts.isCallExpression(node.expression) && node.expression.arguments.length === 0
      && node.expression.expression.getText(source) === 'iterator.next') {
      let root = node;
      while (ts.isBinaryExpression(root.parent)
        && root.parent.operatorToken.kind === ts.SyntaxKind.PlusToken) root = root.parent;
      if (ts.isBinaryExpression(root)) roots.add(root);
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  assert.equal(roots.size, 1, 'the complete original result is replaced once');
  const root = [...roots][0], calls = [];
  function count(node) {
    if (ts.isCallExpression(node) && node.expression.getText(source) === 'iterator.next') calls.push(node);
    ts.forEachChild(node, count);
  }
  count(root);
  assert.equal(calls.length, 4, 'all four original iterator observations are suppressed');
  return {...outputs, 'out/main.js': code.slice(0, root.getStart(source)) + replacement + code.slice(root.end)};
}

async function run(ts, nativeOutputs, actualOutputs, files, options) {
  const variants = [], failures = [];
  const branches = options.module === 'UMD' ? ['commonjs', 'amd'] : ['commonjs'];
  for (const [phase, prefix] of [['unstarted', ''], ['yielded', 'g.next();'], ['delegating', 'g.next();g.next();']]) {
    for (const verb of ['next', 'return', 'throw']) {
      const probe = '(function(){var g=sequence(1),receiver={value:900};' + prefix
        + 'try{return JSON.stringify({value:g.' + verb + '.call(receiver,9)});}'
        + 'catch(error){return JSON.stringify({error:typeof error==="object"?error.name:error});}})()';
      for (const branch of branches) {
        const native = await execution.execute(replaceResult(ts, nativeOutputs, probe), files, options, branch);
        const actual = await execution.execute(replaceResult(ts, actualOutputs, probe), files, options, branch);
        if (options.target !== 'ES5') assert.equal(native, '{"error":"TypeError"}', 'retained intrinsic receiver check');
        else assert.notEqual(native, '{"error":"TypeError"}', 'pinned lowered helper ignores method-call this');
        const row = {phase, verb, branch, borrowed: true, native, actual, executions: 2};
        variants.push(row);
        if (actual !== native) failures.push(row);
      }
    }
  }
  const direct = '(function(){var g=sequence(1);g.next();g.next();try{return JSON.stringify({value:g.throw(9)});}'
    + 'catch(error){return JSON.stringify({error:typeof error==="object"?error.name:error});}})()';
  for (const branch of branches) {
    const native = await execution.execute(replaceResult(ts, nativeOutputs, direct), files, options, branch);
    const actual = await execution.execute(replaceResult(ts, actualOutputs, direct), files, options, branch);
    assert.equal(native, options.target === 'ES5' ? '{"error":9}' : '{"error":"TypeError"}', 'target-specific missing delegate throw ABI');
    const row = {phase: 'delegating', verb: 'throw', branch, borrowed: false, native, actual, executions: 2};
    variants.push(row); if (actual !== native) failures.push(row);
  }
  return {executions: variants.length * 2, variants, failures};
}

module.exports = {run, replaceResult};
