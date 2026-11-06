// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Switch paths, pending completions, source conversion and iterator getter ABI.
const assert = require('node:assert/strict'), execution = require('./ge5_class_execution.cjs');
const {replaceResult} = require('./ge5_generator_switch_borrowed.cjs');
async function run(ts, nativeOutputs, actualOutputs, files, options) {
  const variants = [{name: 'original', expected: 102}];
  function script(argument, steps, setup = '') {
    return '(function(){var effects=[];' + setup + 'var g=sequence(' + argument + '),s=[];'
      + 'function step(method,value){try{s.push(g[method](value));}catch(error){s.push({error:typeof error==="object"?error.name:error});}}'
      + steps + 'return JSON.stringify([s,effects]);})()';
  }
  const next = 'step("next");', finish = next.repeat(5);
  for (const [name, argument] of [['case-one-fallthrough', '1'], ['case-two-direct', '2'], ['default-arm', '0']]) {
    variants.push({name, replacement: script(argument, finish)});
  }
  for (const [name, steps] of [
    ['return-before-start', 'step("return",9);' + next],
    ['throw-before-start', 'step("throw",7);' + next],
    ['return-at-first-yield', next + 'step("return",9);' + next],
    ['throw-at-first-yield', next + 'step("throw",7);' + next],
    ['return-inside-delegation', next.repeat(2) + 'step("return",9);' + next],
    ['throw-inside-delegation', next.repeat(2) + 'step("throw",7);' + next],
    ['completed-return-throw', finish + 'step("return",9);step("throw",7);' + next],
  ]) variants.push({name, replacement: script('1', steps)});
  variants.push({name: 'caller-name-and-parameter-shadow', replacement:
    '(function(){var value=900,iterator={next:function(){throw "wrong lexical iterator";}};'
      + 'var g=sequence.call({value:900},2);return JSON.stringify([g.next(),g.next(),g.next()]);})()'});
  const conversion = (getter, body) => 'var input={};Object.defineProperty(input,Symbol.toPrimitive,{get:function(){effects.push("get");'
    + getter + 'return function(hint){effects.push("convert:"+hint);' + body + '};}});';
  variants.push({name: 'finally-conversion-once-after-default-yield', replacement: script('input', next.repeat(3), conversion('', 'return 2;'))});
  variants.push({name: 'finally-conversion-getter-abrupt', replacement: script('input', next.repeat(3), conversion('throw "conversion getter";', 'return 2;'))});
  variants.push({name: 'finally-conversion-call-abrupt', replacement: script('input', next.repeat(3), conversion('', 'throw "conversion call";'))});
  variants.push({name: 'finally-overrides-pending-return', replacement: script('input', next + 'step("return",9);' + next,
    conversion('throw "override return";', 'return 2;'))});
  variants.push({name: 'finally-overrides-pending-throw', replacement: script('input', next + 'step("throw",7);' + next,
    conversion('throw "override throw";', 'return 2;'))});
  const iterable = body => 'sequence.delegate={};sequence.delegate[Symbol.iterator]=function(){effects.push("iterator");' + body + '};';
  variants.push({name: 'delegate-next-getter-target-profile', delegate: true, replacement: script('2', finish,
    iterable('var i=0,o={};Object.defineProperty(o,"next",{get:function(){effects.push("next:get");return function(value){effects.push("next:"+value);return {value:++i,done:i>2};};}});return o;'))});
  variants.push({name: 'delegate-step-getter-and-identity-profile', delegate: true, replacement: script('2', next + 'step("return",9);',
    iterable('return {next:function(){var o={};Object.defineProperty(o,"done",{enumerable:true,get:function(){effects.push("done");return false;}});Object.defineProperty(o,"value",{enumerable:true,get:function(){effects.push("value");return 3;}});return o;}};'))});
  variants.push({name: 'delegate-primitive-step-target-profile', delegate: true, replacement: script('2', next.repeat(2), iterable('return {next:function(){return 5;}};'))});
  variants.push({name: 'delegate-missing-throw-closing-primitive-profile', delegate: true, replacement: script('2', next + 'step("throw",7);' + next,
    iterable('return {next:function(){return {value:3,done:false};},return:function(){effects.push("return");return 5;}};'))});
  variants.push({name: 'delegate-throw-getter-abrupt', delegate: true, replacement: script('2', next + 'step("throw",7);' + next,
    iterable('var o={next:function(){return {value:3,done:false};}};Object.defineProperty(o,"throw",{get:function(){effects.push("throw:get");throw "throw getter";}});return o;'))});
  variants.push({name: 'delegate-throw-recovery', delegate: true, replacement: script('2', next + 'step("throw",7);' + next.repeat(3),
    iterable('var i=0;return {next:function(){return {value:++i,done:i>1};},throw:function(value){effects.push("throw:"+value);return {value:9,done:false};}};'))});
  variants.push({name: 'delegate-return-getter', delegate: true, replacement: script('2', next + 'step("return",9);' + next,
    iterable('var o={next:function(){return {value:3,done:false};}};Object.defineProperty(o,"return",{get:function(){effects.push("return:get");return function(value){effects.push("return:"+value);return {value:value,done:true};};}});return o;'))});
  function edited(outputs, variant) {
    if (variant.name === 'original') return outputs;
    const changed = replaceResult(ts, outputs, variant.replacement);
    if (!variant.delegate) return changed;
    const code = changed['out/main.js'], file = ts.createSourceFile('runtime.js', code, ts.ScriptTarget.Latest, true), arrays = [];
    function visit(node) {
      if (ts.isFunctionDeclaration(node) && node.name?.text === 'sequence') {
        function operand(n) {
          if (ts.isArrayLiteralExpression(n) && n.elements.length === 2 && n.elements.every(ts.isNumericLiteral)
            && n.elements.map(e => e.text).join(',') === '20,30') arrays.push(n);
          ts.forEachChild(n, operand);
        }
        operand(node); return;
      }
      ts.forEachChild(node, visit);
    }
    visit(file); assert.equal(arrays.length, 1, 'the one original delegated operand is replaced once');
    const array = arrays[0];
    return {...changed, 'out/main.js': code.slice(0, array.getStart(file)) + 'sequence.delegate' + code.slice(array.end)};
  }
  const branches = options.module === 'UMD' ? ['commonjs', 'amd'] : ['commonjs'], results = [], failures = [];
  for (const variant of variants) {
    const values = [];
    for (const branch of branches) {
      const native = await execution.execute(edited(nativeOutputs, variant), files, options, branch);
      const actual = await execution.execute(edited(actualOutputs, variant), files, options, branch);
      if (Object.hasOwn(variant, 'expected')) assert.equal(native, variant.expected, 'pinned original generator observation');
      values.push({branch, native, actual});
      if (actual !== native) failures.push({name: variant.name, branch, native, actual});
    }
    results.push({name: variant.name, executions: branches.length * 2, values});
  }
  return {executions: branches.length * 2, additionalFixtureExecutions: (variants.length - 1) * branches.length * 2, variants: results, failures};
}
module.exports = {run};
