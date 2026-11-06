// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Both function identities, pending completions, delegate operations and gaps matter.
const assert = require('node:assert/strict');
const { stream } = require('./ge5_role_stream.cjs');
const { mutate } = require('./ge5_role_mutations.cjs');
const { compare } = require('./ge5_linear_roles.cjs');
const options = { delegation: true, exceptions: 'delegate-finally' };
function run(ts, native, actual, originals, proof) {
  assert.deepEqual(Object.keys(proof.functions), ['child', 'sequence']);
  const controls = [], start = node => node.getStart(actual.source);
  const check = changed => compare(ts, native, changed, originals, options);
  function mapMutation(label, offset, generated = false) {
    assert.equal(actual.owner(offset) === null, generated, label + ' mutation precondition');
    assert.throws(() => check(mutate(ts, actual, offset, generated ? [0, 0, 0] : null)), undefined, label);
    controls.push({ label, rejected: true });
  }
  function codeMutation(label, node, replacement) {
    const from = start(node), length = node.end - from;
    assert(replacement.length <= length);
    const changed = stream(ts, actual.code.slice(0, from) + replacement.padEnd(length, ' ') + actual.code.slice(node.end), actual.raw);
    assert.throws(() => check(changed), undefined, label); controls.push({ label, rejected: true });
  }
  for (const [name, pair] of Object.entries(proof.functions)) {
    assert.equal(pair.actualPaths.length, 1);
    const trace = pair.actual, returned = trace.events.filter(event => event.kind === 'return');
    assert.equal(returned.length, 1); assert.equal(trace.completion, returned[0].value, name + ' finalizer preserves the pending return');
    mapMutation(name + ' erase function identity interior', start(trace.machine.node.name) + 1);
    mapMutation(name + ' grant dispatcher source', start(trace.machine.dispatcher), true);
    mapMutation(name + ' grant region registration source', start(trace.regions[0].statement), true);
    for (const [index, event] of trace.events.entries()) {
      if (!event.value) continue;
      mapMutation(name + ' erase event ' + index + ' operand', start(event.value.node));
      if (['yield', 'delegate', 'return'].includes(event.kind)) {
        mapMutation(name + ' erase event ' + index + ' operation', start(event.statement));
        mapMutation(name + ' erase event ' + index + ' endpoint', event.root.end - 1);
      }
      if (event.kind === 'effect') {
        assert.equal(event.value.kind, 'call');
        codeMutation(name + ' change finalizer effect', event.value.arguments[0].node, '"other"');
      }
      if (event.kind === 'yield') codeMutation(name + ' change yielded operand ' + index, event.value.node, '9');
      if (event.kind === 'delegate') {
        assert.equal(event.value.callee.text, 'child'); assert.equal(event.value.arguments.length, 0);
        codeMutation('change delegated function identity', event.value.callee.node, 'other');
        const kind = event.statement.expression.properties.find(property => property.name.getText() === 'kind').initializer;
        codeMutation('replace delegate with a plain yield', kind, '"yield"');
      }
    }
    if (name === 'child') codeMutation('change child pending return', returned[0].value.node, '9');
    else {
      assert.equal(returned[0].value.kind, 'sent');
      codeMutation('replace delegate result consumption', returned[0].value.node.expression.name, 'next');
    }
    const region = trace.regions[0], field = name => region.statement.expression.properties.find(property => property.name.getText() === name).initializer;
    codeMutation(name + ' skip region body', field('entry'), String(region.after));
    codeMutation(name + ' bypass finalizer', field('finalizer'), String(region.after));
    codeMutation(name + ' corrupt region continuation', field('after'), String(region.finalizer));
    codeMutation(name + ' invent catch region', field('caught'), '0');
    codeMutation(name + ' replace enter with leave', field('kind'), '"leave"');
    for (const [state, statements] of trace.machine.states) if (!trace.visited.has(state)) {
      mapMutation(name + ' grant dead state ' + state + ' source', start(statements[0]), true);
    }
    const finish = [...trace.machine.states.values()].flat().find(statement => statement.getText() === 'return {kind:"finish"};');
    assert(finish);
    codeMutation(name + ' replace finalizer finish with leave', finish.expression.properties[0].initializer, '"leave"');
    const extra = stream(ts, actual.code.slice(0, region.statement.end) + '0;' + actual.code.slice(region.statement.end), actual.raw);
    assert.throws(() => check(extra), undefined, name + ' extra region statement');
    controls.push({ label: name + ' extra region statement', rejected: true });
  }
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === proof.actual.machine.helper);
  assert(helper); mapMutation('grant generator helper definition source', start(helper.name) + 1, true);
  const helperKind = [];
  function helperOperations(node) {
    if (ts.isStringLiteral(node) && ['yield', 'delegate', 'return', 'throw', 'enter', 'leave', 'finish'].includes(node.text)) helperKind.push(node);
    ts.forEachChild(node, helperOperations);
  }
  helperOperations(helper);
  assert(helperKind.length >= 7, 'reviewed helper operation inventory');
  for (const [index, operation] of helperKind.entries()) codeMutation('corrupt generator helper operation ' + index, operation, '"x"');
  mapMutation('grant map comment source', actual.code.indexOf('//# sourceMappingURL=') + 4, true);
  return controls;
}
module.exports = { run };
