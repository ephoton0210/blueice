// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Coherent artifacts challenge physical roles, all switch paths and signed inputs.
const assert = require('node:assert/strict');
const {stream} = require('./ge5_role_stream.cjs'), {mutate, encode} = require('./ge5_role_mutations.cjs');
const {coherent} = require('./ge5_async_arrow_guard_mutations.cjs');
const helpers = require('./ge5_generator_switch_helpers.cjs'), source = require('./ge5_generator_switch_source.cjs');
function run(ts, native, actual, options, original, proof, compare) {
  const controls = [], start = node => node.getStart(actual.source);
  const check = changed => compare(ts, native, coherent(ts, changed), options, original);
  function reject(label, action, reason) {
    assert.throws(action, reason, label); controls.push({label, rejected: true, publicationCoherent: true});
  }
  function mapping(label, offset) {
    const owner = actual.owner(offset);
    reject(label, () => check(mutate(ts, actual, offset, owner === null ? [0, 0, 0] : null)));
  }
  function change(label, node, text) {
    const begin = start(node); assert(text.length <= node.end - begin, label + ' preserves all physical offsets');
    const code = actual.code.slice(0, begin) + text.padEnd(node.end - begin, ' ') + actual.code.slice(node.end);
    reject(label, () => check(stream(ts, code, actual.raw)));
  }
  for (const [key, pair] of proof.roles) for (const [index, token] of actual.tokens(pair.actual).entries()) {
    if (token.owners.every(owner => owner === null)) continue;
    mapping(key + ' erase source token ' + index, token.start);
    if (token.end - token.start > 1) mapping(key + ' erase source token interior ' + index, token.start + 1);
  }
  mapping('map publication cannot acquire source', actual.code.indexOf('//# sourceMappingURL=') + 4);
  const protocol = helpers.checked(proof.helpers.actual, actual);
  reject('forged lexical helper capability', () => helpers.checked({...proof.helpers.actual}, actual));
  reject('forged original AST inventory', () => source.checked({...proof.origin}));
  for (const helper of protocol.helpers) {
    mapping('helper cannot acquire original source ' + start(helper), start(helper));
    const methods = [];
    function visit(node) {if (ts.isStringLiteral(node) && node.text === 'next') methods.push(node); ts.forEachChild(node, visit);}
    visit(helper);
    if (methods.length) change('complete generator helper verb ' + start(helper), methods[0], '"noop"');
  }
  const fn = proof.roles.get('generator').actual;
  if (options.target === 'ES5') {
    const model = require('./ge5_generator_switch_machine.cjs'), n = proof.machine.native.machine, a = proof.machine.actual.machine;
    reject('forged switch-machine capability', () => model.read(ts, {...proof.machine.capability}, 'blue'));
    reject('cloned physical state graph is not owned', () => model.checked({...a}));
    const oldBody = proof.origin.fn.body;
    proof.origin.fn.body = ts.createSourceFile('wrong.ts', 'function* other(){yield 9;}', ts.ScriptTarget.Latest, true).statements[0].body;
    try {reject('a different original body cannot reuse a signed source proof', () => model.read(ts, proof.machine.capability, 'blue'));}
    finally {proof.origin.fn.body = oldBody;}
    const oldNames = new Set(proof.origin.identifiers); proof.origin.identifiers.clear();
    try {reject('original identifier inventory cannot be replaced by an allowlist', () => model.read(ts, proof.machine.capability, 'blue'));}
    finally {for (const name of oldNames) proof.origin.identifiers.add(name);}
    const states = [...a.states.values()].flat(), property = (statement, name) => statement.expression.properties.find(p => p.name.getText(actual.source) === name).initializer;
    const tagged = kind => states.find(statement => ts.isReturnStatement(statement) && ts.isObjectLiteralExpression(statement.expression)
      && statement.expression.properties.some(p => ts.isPropertyAssignment(p) && p.name.getText(actual.source) === 'kind' && p.initializer.text === kind));
    const region = tagged('enter'), finish = tagged('finish'), delegated = tagged('delegate'), broken = tagged('jump');
    assert(region && finish && delegated && broken);
    mapping('generated region registration cannot acquire source', start(region));
    mapping('generated dispatcher cannot acquire source', start(a.dispatcher));
    change('pending return cannot skip its finalizer', property(region, 'finalizer'), property(region, 'after').getText(actual.source));
    change('the original source has no catch arm', property(region, 'caught'), '0');
    change('region entry cannot bypass the switch', property(region, 'entry'), property(region, 'after').getText(actual.source));
    change('finally finish cannot become normal leave', property(finish, 'kind'), '"leave"');
    change('source break must retain its one enclosing region', property(broken, 'depth'), '0');
    change('yield-star cannot become a plain yield', property(delegated, 'kind'), '"yield"');
    const seen = new Set();
    for (const path of proof.machine.actualPaths) for (const event of path.events) {
      const identity = event.kind === 'case' && !event.projection ? event.clause : event.statement;
      if (seen.has(identity)) continue; seen.add(identity);
      for (const key of event.kind === 'case' ? ['value', 'test'] : event.kind === 'jump' ? [] : ['value']) {
        mapping(event.kind + '/' + key + ' exact original operand end ' + event[key].node.end, event[key].node.end);
      }
      if (event.kind === 'case') {
        if (event.projection) {
          const descriptor = event.projection;
          mapping('captured source-case punctuation ' + start(descriptor.definition.statement), descriptor.definition.statement.end);
          change('case test cannot be duplicated ' + descriptor.original.expression.text, descriptor.definition.assignment.right,
            descriptor.original.expression.text === '1' ? '2' : '1');
          const condition = descriptor.assignment.right;
          const yes = condition.whenTrue.getText(actual.source), no = condition.whenFalse.getText(actual.source);
          if (yes.length <= no.length) change('case miss cannot select the hit arm ' + start(condition), condition.whenFalse, yes);
          else change('case hit cannot select the miss arm ' + start(condition), condition.whenTrue, no);
          change('case selector must be the original captured selector ' + start(condition), condition.condition.left, condition.condition.right.getText(actual.source));
        } else {
          const colon = actual.tokens(event.clause).find(token => token.text === ':'); assert(colon);
          mapping('source-case punctuation ' + start(event.clause), colon.end);
          change('case test cannot be duplicated ' + event.test.text, event.test.node, event.test.text === '1' ? '2' : '1');
        }
      }
      if (event.kind === 'effect') {
        assert.equal(event.value.kind, 'compound-assignment');
        change('finally update cannot change operation', event.value.node.operatorToken, '-=');
        change('finally update cannot change amount', event.value.node.right, '2');
      }
    }
    if (protocol.external) {
      const reference = protocol.reference('__generator');
      change('replace the independently bound generator helper', reference.namespace ? a.call.expression.name : a.call.expression, '__wrong');
      const sync = a.adapter.sync[0]; assert(sync);
      change('reversed native label synchronization cannot drift', sync.expression.right, '0');
      change('native pending yield cannot start armed', a.node.body.statements.find(s => ts.isVariableStatement(s)
        && s.declarationList.declarations[0].name.text === a.adapter.pending).declarationList.declarations[0].initializer, 'true');
    }
    const nv = stream(ts, native.code, native.raw), av = stream(ts, actual.code, actual.raw), origin = source.read(ts, original);
    const envelope = require('./ge5_module_envelope.cjs'), dependencies = protocol.imported ? ['tslib'] : [];
    const ne = envelope.read(ts, nv, options, {originals: [origin.file], dependencies}); ne.blue = false; ne.classifyTransport();
    const ae = envelope.read(ts, av, options, {blue: true, originals: [origin.file], dependencies}); ae.blue = true; ae.classifyTransport();
    const nc = helpers.create(ts, ne, options, origin), ac = helpers.create(ts, ae, options, origin);
    reject('real but uncovered headers cannot sign a switch model', () => model.create(ts, nv, nc, av, ac, origin), /independently proved generator header/);
    assert(n && a);
  } else {
    const yields = [], switches = [];
    function visit(node) {if (ts.isYieldExpression(node)) yields.push(node); if (ts.isSwitchStatement(node)) switches.push(node); ts.forEachChild(node, visit);}
    visit(fn); assert.equal(yields.length, 3); assert.equal(switches.length, 1);
    for (const [index, node] of yields.entries()) mapping('retained complete yield operand end ' + index, node.expression.end);
    const cases = switches[0].caseBlock.clauses.filter(ts.isCaseClause);
    change('retained first case cannot become the second', cases[0].expression, '2');
    change('retained delegate cannot drop its second value', yields[1].expression.elements[1], '20');
  }
  const segments = actual.decoded.segments.map(segment => [...segment]);
  const at = segments.findIndex(segment => segment.length > 2), base = segments[at], hidden = [base[0], base[1], 0, 0, 0];
  if (JSON.stringify(hidden.slice(2)) === JSON.stringify(base.slice(2))) hidden[4] = 1;
  segments.splice(at, 0, hidden);
  reject('hidden in-bounds source transition', () => check(stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), mappings: encode(segments)}))), /conflicting same-position/);
  controls.push(...require('./ge5_async_arrow_publication_mutations.cjs').run(ts, native, actual, options, original, check));
  return controls;
}
module.exports = {run};
