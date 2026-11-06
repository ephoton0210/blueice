// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Only exact, independently proved async method subtrees may be delegated.
// The enclosing class continues to own following gaps and descriptor transport.
const assert = require('node:assert/strict'), proofs = new WeakSet();
const start = (view, node) => node.getStart(view.source);
function create(ts, native, actual, n, a, original, pairs = []) {
  const sourceMethods = original.members.filter(node => ts.isMethodDeclaration(node) || ts.isGetAccessorDeclaration(node) || ts.isSetAccessorDeclaration(node));
  const items = [], seen = new Set();
  function emitted(view, klass, source, index) {
    const name = source.name.getText(original.getSourceFile());
    if (klass.native) {
      const method = klass.node.members.filter(node => ts.isMethodDeclaration(node) || ts.isGetAccessorDeclaration(node) || ts.isSetAccessorDeclaration(node))[index];
      assert(method && ts.isMethodDeclaration(method) && method.parent === klass.node, 'exact retained class method');
      assert.equal(method.name.getText(view.source), name);
      return method;
    }
    const member = klass.methods[index]; assert(member && ts.isFunctionExpression(member.fn));
    assert.equal(ts.isStringLiteral(member.key) ? member.key.text : member.key.getText(view.source), name);
    const static_ = source.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.StaticKeyword) ?? false;
    const receiver = member.descriptor ? member.target : member.target.expression;
    assert(receiver, 'exact original method receiver');
    assert.equal(receiver.getText(view.source), klass.name + (static_ ? '' : '.prototype'), 'exact original method placement');
    assert(member.fn.getSourceFile() === view.source && member.statement.parent === klass.factory.body, 'method belongs to the exact class factory');
    return member.fn;
  }
  for (const pair of pairs) {
    assert(pair && ts.isMethodDeclaration(pair.source) && pair.source.parent === original
      && pair.source.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.AsyncKeyword)
      && !pair.source.asteriskToken && pair.source.body, 'exact original non-generator async class method');
    assert(!seen.has(pair.source), 'one delegated proof per original method'); seen.add(pair.source);
    const index = sourceMethods.indexOf(pair.source); assert(index >= 0);
    assert(pair.native === emitted(native, n, pair.source, index), 'exact Native emitted method identity');
    assert(pair.actual === emitted(actual, a, pair.source, index), 'exact actual emitted method identity');
    for (const [view, method] of [[native, pair.native], [actual, pair.actual]]) {
      for (let offset = start(view, method); offset < method.end; offset++) assert(view.covered(offset), 'delegated method must already prove every UTF-16 character');
    }
    items.push({...pair, index});
  }
  function region(item) {
    const nn = item.native, an = item.actual;
    native.roleStarts ??= new Map(); native.roleEnds ??= new Map(); actual.roleStarts ??= new Map(); actual.roleEnds ??= new Map();
    actual.nativeStarts ??= new Map(); actual.nativeEnds ??= new Map();
    native.roleStarts.set(start(native, nn), native.owner(start(native, nn))); native.roleEnds.set(nn.end, native.owner(nn.end));
    actual.roleStarts.set(start(actual, an), native.owner(start(native, nn))); actual.roleEnds.set(an.end, native.owner(nn.end));
    actual.nativeStarts.set(start(actual, an), start(native, nn)); actual.nativeEnds.set(an.end, nn.end);
  }
  function paired(nn, an) {
    const item = items.find(item => item.native === nn || item.actual === an);
    if (!item) return false;
    assert(item.native === nn && item.actual === an, 'the same original method owns both proofs'); region(item); return true;
  }
  function project(view, tokens) {
    const side = view === native ? 'native' : 'actual', result = [];
    for (const token of tokens) {
      const item = items.find(item => start(view, item[side]) <= token.start && token.end <= item[side].end);
      if (!item) {result.push(token); continue;}
      if (result.at(-1)?.methodHole === item) continue;
      const node = item[side], from = start(view, node);
      assert(tokens.some(token => token.start === from) && tokens.some(token => token.end === node.end), 'a method is delegated as a complete syntax subtree');
      result.push({text: '$class.async.method.' + item.index, start: from, end: node.end, methodHole: item});
    }
    return result;
  }
  function tokensPaired(nn, an) {
    assert(nn.methodHole && nn.methodHole === an.methodHole, 'the same exact method proof'); region(nn.methodHole);
  }
  const proof = {items, paired, project, tokensPaired, native, actual}; proofs.add(proof); return proof;
}
function coversComment(proof, view, comment) {
  assert(proofs.has(proof), 'exact typed method proof');
  assert(view === proof.native || view === proof.actual, 'exact proved method view');
  assert(view.comments().includes(comment), 'exact scanned method comment');
  const side = view === proof.native ? 'native' : 'actual';
  if (!proof.items.some(item => start(view, item[side]) <= comment.start && comment.end <= item[side].end)) return false;
  for (let offset = comment.start; offset < comment.end; offset++) assert(view.covered(offset), 'method comment already proves every UTF-16 unit');
  return true;
}
module.exports = {create, coversComment};
