// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Opaque source/header/helper proof admits one complete switch/delegate profile.
const assert = require('node:assert/strict'), models = new WeakMap(), machines = new WeakMap();
const helpers = require('./ge5_generator_switch_helpers.cjs');
const profile = 'switch-delegate-finally';
function create(ts, native, nativeCapability, actual, actualCapability, original) {
  const origin = require('./ge5_generator_switch_source.cjs').checked(original);
  const np = helpers.checked(nativeCapability, native), ap = helpers.checked(actualCapability, actual);
  assert(np.lowered && ap.lowered);
  for (const proof of [np, ap]) {
    assert.equal(proof.fn.name.text, origin.fn.name.text);
    const view = proof.view, end = proof.fn.body.getStart(view.source) + 1;
    for (let at = proof.fn.getStart(view.source); at < end; at++) assert(view.covered(at), 'complete independently proved generator header');
    const returned = proof.fn.body.statements.at(-1);
    assert(ts.isReturnStatement(returned) && ts.isCallExpression(returned.expression));
    assert(proof.isReference(returned.expression.expression, '__generator'), 'exact independently bound generator driver');
    if (view.decoded.metadata.sourcesContent) assert.equal(view.decoded.metadata.sourcesContent[0], origin.file.text);
  }
  const capability = {}; models.set(capability, {ts, origin, native: np, actual: ap}); return capability;
}
function model(capability, ts, view, node) {
  const result = models.get(capability); assert(result && result.ts === ts, 'unforgeable original switch-machine capability');
  require('./ge5_generator_switch_source.cjs').checked(result.origin);
  const proof = view === result.native.view ? result.native : view === result.actual.view ? result.actual : null;
  assert(proof && proof.fn === node && node.getSourceFile() === view.source, 'exact original generated generator node/view');
  const end = node.body.getStart(view.source) + 1;
  assert.equal(view.source.text, view.code, 'the exact originally parsed physical module remains bound');
  for (let at = node.getStart(view.source); at < end; at++) assert(view.covered(at), 'generator header proof remains complete');
  return {...result, proof};
}
function adapter(capability, ts, view, node, options) {
  const result = model(capability, ts, view, node), proof = result.proof;
  assert(proof === result.actual && proof.external && options.exceptions === profile && options.switch && options.delegation);
  assert.equal(options.generatorReference, proof.reference('__generator'));
  assert.equal(options.valuesReference, proof.reference('__values'));
  return result;
}
function read(ts, capability, dialect) {
  const stored = models.get(capability); assert(stored && stored.ts === ts);
  const proof = dialect === 'native' ? stored.native : dialect === 'blue' ? stored.actual : null; assert(proof);
  model(capability, ts, proof.view, proof.fn);
  const options = {exceptions: profile, switch: true, delegation: true, switchContract: capability,
    ...dialect === 'native' || proof.external ? {generatorReference: proof.reference('__generator')} : {},
    valuesReference: proof.reference('__values')};
  const machine = require('./ge5_continuation_machine.cjs').read(ts, proof.view, proof.fn, dialect, [stored.origin.file.text], options);
  assert.equal(machine.userSlots.size, 0, 'no source local declaration belongs to the generator body');
  for (const name of machine.slots) assert(!stored.origin.identifiers.has(name), 'all carried values are fresh relative to the entire original AST');
  const state = {capability, ...stored, proof, cases: new Map(), definitions: new Set()};
  machines.set(machine, state);
  if (dialect === 'blue' && proof.external) bindCases(ts, machine, state);
  return machine;
}
function bindCases(ts, machine, state) {
  const {origin} = state, branches = [], definitions = new Map();
  for (const statements of machine.states.values()) for (const statement of statements) {
    if (!ts.isExpressionStatement(statement) || !ts.isBinaryExpression(statement.expression)) continue;
    const assignment = statement.expression;
    if (ts.isIdentifier(assignment.left) && machine.slots.has(assignment.left.text)) {
      assert.equal(assignment.operatorToken.kind, ts.SyntaxKind.EqualsToken);
      assert(!definitions.has(assignment.left.text)); definitions.set(assignment.left.text, {statement, assignment});
    }
    if (assignment.left.getText(machine.view.source) !== machine.context + '.label' || !ts.isConditionalExpression(assignment.right)) continue;
    const test = assignment.right.condition;
    assert(ts.isBinaryExpression(test) && test.operatorToken.kind === ts.SyntaxKind.EqualsEqualsEqualsToken);
    assert(ts.isIdentifier(test.left) && ts.isIdentifier(test.right) && test.left.text !== test.right.text);
    assert(machine.slots.has(test.left.text) && machine.slots.has(test.right.text)); branches.push({statement, assignment, test});
  }
  assert.equal(branches.length, 2, 'complete original ordered source-case inventory');
  const selector = new Set();
  for (const branch of branches) {
    const definition = definitions.get(branch.test.right.text); assert(definition);
    const value = definition.assignment.right;
    assert(ts.isNumericLiteral(value));
    const original = [origin.first, origin.second].filter(node => node.expression.text === value.text);
    assert.equal(original.length, 1, 'every captured case test belongs to one original case');
    assert(![...state.cases.values()].some(item => item.original === original[0]), 'no duplicated source case');
    const descriptor = {original: original[0], definition, ...branch};
    state.cases.set(branch.test, descriptor); state.definitions.add(definition.statement); selector.add(branch.test.left.text);
  }
  assert.equal(selector.size, 1, 'one shared captured switch selector');
  const captured = definitions.get([...selector][0]); assert(captured);
  assert(ts.isIdentifier(captured.assignment.right) && captured.assignment.right.text === origin.parameter);
  state.selector = captured;
}
function checked(machine) {
  const state = machines.get(machine); assert(state, 'unforgeable source-bound switch machine');
  model(state.capability, state.ts, machine.view, machine.node); return state;
}
function owns(machine) {return machines.has(machine);}
function selection(machine, condition) {return checked(machine).cases.get(condition) || null;}
function caseDefinition(machine, statement) {return checked(machine).definitions.has(statement);}
function denseDelegate(ts, machine, operand) {
  const state = checked(machine);
  assert(ts.isArrayLiteralExpression(operand) && operand.elements.every(ts.isNumericLiteral));
  assert.deepEqual(operand.elements.map(e => e.text), state.origin.array.elements.map(e => e.text));
}
function compareCase(ts, n, x, a, y, operations) {
  const ns = checked(n), as = checked(a); assert.equal(ns.capability, as.capability);
  assert(as.proof.external && y.projection === selection(a, y.statement.expression.right.condition));
  const descriptor = y.projection, native = n.view, actual = a.view;
  assert.equal(x.clause.expression.getText(native.source), descriptor.original.expression.getText(as.origin.file));
  const {value, uniform, cover} = operations;
  value(x.test, y.test);
  const header = native.tokens(x.clause).filter(t => t.end <= x.opcode.getStart(native.source));
  assert.deepEqual(header.map(t => t.text), ['case', descriptor.original.expression.text, ':']);
  const literal = header[1], colon = header[2], definition = descriptor.definition;
  native.noSource(header[0].start, literal.start, 'Native generated case keyword');
  actual.noSource(definition.statement.getStart(actual.source), definition.assignment.right.getStart(actual.source), 'generated case-test capture prefix');
  const endpoint = native.owner(literal.end), punctuationEnd = native.owner(colon.end);
  uniform(native, literal.end, colon.end, endpoint, 'Native source-case punctuation');
  cover(native, literal.end, colon.end, punctuationEnd);
  uniform(actual, definition.assignment.right.end, definition.statement.end, endpoint, 'captured source-case punctuation');
  assert.deepEqual(actual.owner(definition.statement.end), punctuationEnd, 'captured source-case completion');
  cover(actual, definition.assignment.right.end, definition.statement.end, punctuationEnd);
  const clauses = x.statement.caseBlock.clauses;
  native.noSource(x.statement.getStart(native.source), clauses[0].getStart(native.source), 'Native generated switch selector');
  native.noSource(x.statement.caseBlock.end - 1, x.statement.caseBlock.end, 'Native generated switch close');
  const prefix = native.owner(x.opcode.getStart(native.source)), trailing = native.owner(x.opcode.end); assert(prefix);
  uniform(native, x.opcode.getStart(native.source), x.opcode.end, prefix, 'Native ordered source-case jump');
  cover(native, x.opcode.getStart(native.source), x.opcode.end, trailing);
  uniform(actual, y.opcode.getStart(actual.source), y.opcode.end, prefix, 'captured ordered source-case selection');
  assert.deepEqual(actual.owner(y.opcode.end), trailing, 'captured source-case selection endpoint');
  cover(actual, y.opcode.getStart(actual.source), y.opcode.end, trailing);
}
module.exports = {profile, create, read, adapter, checked, owns, selection, caseDefinition, denseDelegate, compareCase};
