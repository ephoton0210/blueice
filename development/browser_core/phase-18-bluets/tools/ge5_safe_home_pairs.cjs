// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exact roles compose only with typed, independently parsed private descriptor capabilities.
const assert = require('node:assert/strict'), ir = require('./ge5_safe_home_ir.cjs');
function role(view, start, end, owner, label) {
  for (let at = start; at < end; at++) assert.deepEqual(view.owner(at), owner, label + ' UTF-16 owner at ' + at);
  view.cover(start, end); (view.roleStarts ??= new Map()).set(start, owner); (view.roleEnds ??= new Map()).set(end, view.owner(end));
}
function tokens(ts, profile, node, omitted = []) {
  const {view} = profile, ids = new Map();
  for (const id of ir.walk(ts, node, ts.isIdentifier)) {
    if (ts.isPropertyAccessExpression(id.parent) && id.parent.name === id || ts.isPropertyAssignment(id.parent) && id.parent.name === id) continue;
    if (id === profile.core.name) continue;
    const name = profile.aliases.get(id.text); if (name) ids.set(id.getStart(view.source), name);
  }
  return view.tokens(node).filter(t => !omitted.some(n => n.getStart(view.source) <= t.start && t.end <= n.end)).map(token => {
    const name = ids.get(token.start);
    if (!name) return token;
    const owner = token.owners[0]; assert(token.owners.every(v => JSON.stringify(v) === JSON.stringify(owner)), 'complete typed private/class reference is uniform');
    return {...token, text: name, owners: [owner]};
  });
}
function ends(ts, native, nn, actual, an, label, {omitNative = [], omitActual = []} = {}, points = []) {
  assert.equal(an.kind, nn.kind, label + ' complete AST kind');
  assert.deepEqual(actual.owner(an.end), native.owner(nn.end), label + ' exact logical endpoint ' + ts.SyntaxKind[nn.kind]);
  points.push({kind: ts.SyntaxKind[nn.kind], native: nn.end, actual: an.end, owner: native.owner(nn.end)});
  const nc = [], ac = [];
  ts.forEachChild(nn, n => { if (!omitNative.includes(n)) nc.push(n); });
  ts.forEachChild(an, n => { if (!omitActual.includes(n)) ac.push(n); });
  assert.equal(ac.length, nc.length, label + ' complete logical child inventory');
  for (let i = 0; i < nc.length; i++) ends(ts, native, nc[i], actual, ac[i], label, {omitNative, omitActual}, points);
  return points;
}
function definition(ts, n, ne, a, ae, options, contract) {
  assert.equal(ne.member, ae.member); const source = ne.member.node, nv = n.view, av = a.view;
  const loweredAsync = source.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword) && options.target === 'ES2015';
  assert.equal(!!ae.fn.asteriskToken, !!ne.fn.asteriskToken);
  assert.equal(!!ae.fn.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword), !!ne.fn.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword));
  const nf = nv.tokens(ne.fn), af = av.tokens(ae.fn), name = ae.fn.name;
  const nn = nf.find(t => t.text === 'function'), an = af.find(t => t.text === 'function');
  assert.deepEqual(an.owners, nn.owners, 'complete private callable function header');
  role(av, name.getStart(av.source), name.end, null, 'generated private callable internal name');
  let points;
  if (loweredAsync) points = require('./ge5_safe_home_async.cjs').compare(ts, n, ne.fn, a, ae.fn, source, options, contract, {privateCallable: true}).points;
  else {
    contract.pair(tokens(ts, n, ne.fn), tokens(ts, a, ae.fn, [name]), 'complete original private callable and generated function header');
    points = ends(ts, nv, ne.fn, av, ae.fn, 'complete original private callable', {omitActual: [name]});
  }
  const headerOwner = nv.owner(ne.definition.getStart(nv.source));
  role(nv, ne.definition.getStart(nv.source), ne.fn.getStart(nv.source), headerOwner, 'Native source-bound private naming intrinsic prefix');
  role(av, ae.definition.getStart(av.source), ae.fn.getStart(av.source), headerOwner, 'source-bound private naming intrinsic prefix');
  const ending = nv.owner(ne.fn.end);
  role(nv, ne.fn.end, ne.definition.end, ending, 'Native exact original private callable name result');
  role(av, ae.fn.end, ae.definition.end, ending, 'exact original private callable name descriptor result');
  assert.deepEqual(av.owner(ae.definition.end), nv.owner(ne.definition.end), 'exact complete private naming intrinsic end');
  return {source, native: ne, actual: ae, points};
}
module.exports = {role, tokens, ends, definition};
