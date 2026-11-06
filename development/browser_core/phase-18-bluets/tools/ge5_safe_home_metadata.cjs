// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Private descriptor construction and application have distinct typed event identities.
const assert = require('node:assert/strict'), ir = require('./ge5_safe_home_ir.cjs'), pairs = require('./ge5_safe_home_pairs.cjs');
function compare(ts, n, a, contract, definitions, helpers) {
  assert.equal(definitions.length, n.entries.length);
  const points = [], events = [];
  function complete(p, node) { for (const token of p.view.tokens(node)) assert(p.view.covered(token.start), 'metadata hole belongs to an already complete private callable definition'); }
  function paired(nn, an, label, excludedN = [], excludedA = []) {
    contract.pair(pairs.tokens(ts, n, nn, excludedN), pairs.tokens(ts, a, an, excludedA), label);
    points.push(...pairs.ends(ts, n.view, nn, a.view, an, label, {omitNative: excludedN, omitActual: excludedA}));
  }
  function replaceTokens(p, node, replacements) {
    const found = [];
    for (const token of pairs.tokens(ts, p, node)) {
      const hole = replacements.find(x => x.node.getStart(p.view.source) <= token.start && token.end <= x.node.end);
      if (!hole) { found.push(token); continue; }
      if (found.at(-1)?.hole === hole) continue;
      const role = hole.roleNode ?? hole.node, owner = p.view.owner(role.getStart(p.view.source));
      for (const t of p.view.tokens(role)) assert(t.owners.every(v => JSON.stringify(v) === JSON.stringify(owner)), 'uniform typed metadata argument reference');
      found.push({text: hole.text, start: hole.node.getStart(p.view.source), end: hole.node.end, owners: [owner], hole});
    }
    return found;
  }
  function statement(p, node) { assert(ts.isExpressionStatement(node.parent)); return node.parent; }
  const direct = p => p.metadataBody.statements.filter(s => !helpers.metadata.find(m => m.profile === p)?.base.hoist || s !== helpers.metadata.find(m => m.profile === p).base.hoist);
  for (let index = 0; index < n.entries.length; index++) {
    const ne = n.entries[index], ae = a.entries[index]; assert.equal(definitions[index].native, ne); assert.equal(definitions[index].actual, ae);
    complete(n, ne.definition); complete(a, ae.definition);
    // Both algorithms create exactly the same fresh string-named private callable object.
    contract.pair(pairs.tokens(ts, n, ne.assignment).filter(t => !n.view.covered(t.start)), pairs.tokens(ts, a, ae.assignment).filter(t => !a.view.covered(t.start)), 'complete private descriptor construction prefix/suffix '+index);
    assert.deepEqual(a.view.owner(ae.assignment.end), n.view.owner(ne.assignment.end), 'exact complete private descriptor construction end');
    const as = statement(a, ae.assignment), semicolon = a.view.tokens(as).at(-1); assert.equal(semicolon.text, ';');
    pairs.role(a.view, semicolon.start, semicolon.end, n.view.owner(ne.assignment.end), 'explicit private descriptor construction statement separator');
    assert.deepEqual(a.view.owner(as.end), n.view.owner(ne.assignment.end), 'private construction event exact statement end');
    function decorators(p, entry) {
      const found = direct(p).filter(s => ts.isExpressionStatement(s) && ts.isBinaryExpression(s.expression) && s.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(s.expression.left) && s.expression.left.text === entry.call.arguments[2].text);
      assert.equal(found.length, 1); const expr = found[0].expression;
      assert(ts.isArrayLiteralExpression(expr.right)); assert.deepEqual(expr.right.elements.map(e => e.getText(p.view.source)), entry.member.decorators.map(d => d.expression.getText(n.cap.original)), 'complete original private decorator evaluation order');
      return found[0];
    }
    const nd = decorators(n, ne), ad = decorators(a, ae); paired(nd, ad, 'complete original private decorator evaluation '+index);
    // descriptorIn is a non-null object, so the proved helper never evaluates ctor as a target.
    const nc = ne.call.arguments[0], ac = ae.call.arguments[0];
    assert(nc.kind === ts.SyntaxKind.ThisKeyword || ts.isIdentifier(nc) && nc.text === n.receiver);
    assert.equal(ac.kind, ts.SyntaxKind.NullKeyword);
    const nt = replaceTokens(n, ne.call, [{node: nc, text: '$private.descriptor.target.unused'}, {node: ne.assignment, roleNode: ne.assignment.left, text: '$private.'+index+'.descriptor'}]);
    const at = replaceTokens(a, ae.call, [{node: ac, text: '$private.descriptor.target.unused'}, {node: ae.call.arguments[1], text: '$private.'+index+'.descriptor'}]);
    contract.pair(nt, at, 'complete private decorator helper and context event '+index);
    points.push(...pairs.ends(ts, n.view, ne.call, a.view, ae.call, 'complete private decoration event', {omitNative: [nc, ne.assignment], omitActual: [ac, ae.call.arguments[1]]}));
    assert.deepEqual(a.view.owner(ac.end), n.view.owner(nc.end), 'exact ignored descriptor constructor transport end');
    assert.deepEqual(a.view.owner(ae.call.arguments[1].end), n.view.owner(ne.assignment.end), 'exact generated descriptor reference role');
    const ns = statement(n, ne.call), ass = statement(a, ae.call); paired(ns, ass, 'complete private decoration statement '+index, [ne.assignment, nc], [ae.call.arguments[1], ac]);
    events.push({member: ne.member, native: {decorators: nd, descriptor: ne.assignment, call: ns}, actual: {decorators: ad, descriptor: as, call: ass}});
  }
  const publications = p => direct(p).filter(ts.isIfStatement);
  const np = publications(n), ap = publications(a); assert.equal(np.length, 1); assert.equal(ap.length, 1);
  function publication(p, node) {
    assert(!node.elseStatement && ts.isIdentifier(node.expression) && node.expression.text === p.metadata.name.text && ts.isExpressionStatement(node.thenStatement));
    const call = node.thenStatement.expression; assert(ts.isCallExpression(call) && call.expression.getText(p.view.source) === 'Object.defineProperty' && call.arguments.length === 3);
    const receiver = call.arguments[0]; assert(receiver.kind === ts.SyntaxKind.ThisKeyword || !p.blue && ts.isIdentifier(receiver) && receiver.text === p.receiver);
    assert.equal(call.arguments[1].getText(p.view.source), 'Symbol.metadata');
    const descriptor = ir.properties(ts, call.arguments[2], ['enumerable', 'configurable', 'writable', 'value']);
    for (const name of ['enumerable', 'configurable', 'writable']) assert.equal(descriptor[name].kind, ts.SyntaxKind.TrueKeyword);
    assert(ts.isIdentifier(descriptor.value) && descriptor.value.text === p.metadata.name.text);
    return receiver;
  }
  const nr = publication(n, np[0]), ar = publication(a, ap[0]);
  contract.pair(replaceTokens(n, np[0], [{node: nr, text: '$class.metadata.receiver'}]), replaceTokens(a, ap[0], [{node: ar, text: '$class.metadata.receiver'}]), 'complete inherited private metadata publication');
  points.push(...pairs.ends(ts, n.view, np[0], a.view, ap[0], 'complete inherited metadata publication', {omitNative: [nr], omitActual: [ar]}));
  const extra = [];
  for (const p of [n, a]) {
    const calls = direct(p).filter(s => ts.isExpressionStatement(s) && ts.isCallExpression(s.expression) && p.aliases.get(s.expression.expression.getText(p.view.source)) === '$helper.runInitializers');
    assert.equal(calls.length, Number(n.cap.members[0].static), 'complete static private extra-initializer phase inventory');
    if (calls.length) { const call = calls[0].expression; assert.equal(call.arguments.length, 2); assert(ts.isIdentifier(call.arguments[1]) && call.arguments[1].text === p.entries[0].call.arguments[5].text); extra.push({profile: p, statement: calls[0], receiver: call.arguments[0]}); }
    const declared = helpers.metadata.find(m => m.profile === p).statement;
    const known = new Set([declared, ...events.flatMap(e => p === n ? [e.native.decorators, e.native.call] : [e.actual.decorators, e.actual.descriptor, e.actual.call]), ...publications(p), ...calls]);
    assert.equal(direct(p).length, known.size, 'complete direct private metadata statement inventory'); assert(direct(p).every(s => known.has(s)));
    const schedule = direct(p).map(s => s === declared ? 'metadata' : events.findIndex(e => (p === n ? e.native : e.actual).decorators === s) >= 0 ? 'decorators.'+events.findIndex(e => (p === n ? e.native : e.actual).decorators === s) : events.findIndex(e => (p === n ? e.native : e.actual).descriptor === s) >= 0 ? 'descriptor.'+events.findIndex(e => (p === n ? e.native : e.actual).descriptor === s) : events.findIndex(e => (p === n ? e.native : e.actual).call === s) >= 0 ? 'call.'+events.findIndex(e => (p === n ? e.native : e.actual).call === s) : publications(p).includes(s) ? 'publication' : 'extra');
    const expected = ['metadata', ...(p === n ? events.map((_, i) => 'decorators.'+i) : events.flatMap((_, i) => ['decorators.'+i, 'descriptor.'+i])), ...events.map((_, i) => 'call.'+i), 'publication', ...(calls.length ? ['extra'] : [])];
    assert.deepEqual(schedule, expected, 'exact source-derived private metadata evaluation schedule');
  }
  if (extra.length) {
    assert.equal(extra.length, 2); const [x, y] = extra;
    assert(x.receiver.kind === ts.SyntaxKind.ThisKeyword || ts.isIdentifier(x.receiver) && x.receiver.text === n.receiver); assert.equal(y.receiver.kind, ts.SyntaxKind.ThisKeyword);
    contract.pair(replaceTokens(n, x.statement, [{node: x.receiver, text: '$class.static.extra.receiver'}]), replaceTokens(a, y.statement, [{node: y.receiver, text: '$class.static.extra.receiver'}]), 'complete original static private extra-initializer call');
    points.push(...pairs.ends(ts, n.view, x.statement, a.view, y.statement, 'complete static private extra phase', {omitNative: [x.receiver], omitActual: [y.receiver]}));
  }
  return {events, publications: {native: np[0], actual: ap[0]}, extra, points};
}
module.exports = {compare};
