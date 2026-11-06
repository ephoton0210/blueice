// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Prove every physical character of the exact source-bound super lookup.
const assert = require('node:assert/strict'), {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
const proofs = new WeakMap(), bindings = new WeakMap(), start = (view, node) => node.getStart(view.source);
function prove(ts, native, n, actual, a, options) {
  assert(!n.retained && !a.retained && n.originalMethod === a.originalMethod);
  assert(n.originalSuperCall === a.originalSuperCall && n.originalSuperCall.expression.expression.kind === ts.SyntaxKind.SuperKeyword);
  const nn = n.superMember, an = a.superMember;
  assert(ts.isPropertyAccessExpression(nn) && ts.isCallExpression(an));
  function token(nt, at, label, rename = false) {
    if (rename) {
      const owner = nt.owners[0]; assert(nt.owners.every(value => JSON.stringify(value) === JSON.stringify(owner)), label + ': uniform Native role');
      assert(at.owners.every(value => JSON.stringify(value) === JSON.stringify(owner)), label + ': complete transformed role');
    } else {
      assert.equal(at.text, nt.text, label + ': token spelling'); assert.deepEqual(at.owners, nt.owners, label + ': complete UTF-16 roles');
    }
    native.cover(nt.start, nt.end); actual.cover(at.start, at.end);
  }
  const nt = native.tokens(nn), at = actual.tokens(an);
  let delimiter, key;
  if (options.target === 'ES5') {
    const nb = native.tokens(nn.expression), ab = actual.tokens(an.arguments[0]); assert.equal(nb.length, 3); assert.equal(ab.length, 3);
    token(nb[0], ab[0], 'exact original base constructor binding', true);
    token(nb[1], ab[1], 'base prototype delimiter'); token(nb[2], ab[2], 'base prototype lookup');
    delimiter = at.find(item => item.start === an.arguments[0].end); assert.equal(delimiter.text, ','); key = actual.token(an.arguments[1]);
    actual.noSource(start(actual, an), start(actual, an.arguments[0]), 'generated Reflect.get dispatch');
    actual.noSource(an.arguments[1].end, an.end, 'generated explicit super receiver and close');
  } else {
    assert.equal(nt.length, 3); assert.equal(at.length, 4);
    token(nt[0], at[0], 'exact source-bound getter binding', true);
    delimiter = at[1]; assert.equal(delimiter.text, '('); key = at[2];
    actual.noSource(at[3].start, at[3].end, 'generated super getter invocation closing');
  }
  assert.equal(key.text, '"' + nn.name.text + '"');
  token(nt.at(-2), {...delimiter, text: '.'}, 'super property lookup delimiter');
  token(nt.at(-1), key, 'original super property name', true);
  compareGap(native, nt.at(-2).end, nt.at(-1).start, actual, delimiter.end, key.start, 'super property delimiter to key boundary');
  const tail = (view, item) => view.tokens(item.superCall).filter(token => token.start >= item.superMember.end);
  compareTokens(native, tail(native, n), actual, tail(actual, a), 'complete source-bound super invocation receiver and punctuation');
  assert.deepEqual(actual.owner(an.end), native.owner(nn.end), 'super member logical endpoint');
  assert.deepEqual(actual.owner(a.superCall.end), native.owner(n.superCall.end), 'super call logical endpoint');
  assert(ts.isBinaryExpression(n.superCall.parent) && ts.isBinaryExpression(a.superCall.parent));
  compareTokens(native, [native.token(n.superCall.parent.operatorToken)], actual, [actual.token(a.superCall.parent.operatorToken)], 'binary operator following the transformed super call');
  for (const [view, node] of [[native, nn], [actual, an]]) for (let offset = start(view, node); offset < node.end; offset++) {
    assert(view.covered(offset), 'every delegated super-member UTF-16 unit is proved');
  }
  const proof = Object.freeze({kind: 'proved-derived-super-member-v1'});
  const side = item => Object.freeze({read: item.superMember, invocation: item.superCall, callback: item.callback, source: item.source.text});
  proofs.set(proof, Object.freeze({native, actual, n: side(n), a: side(a), entry: native.owner(start(native, nn)), end: native.owner(nn.end), role: 'original derived super.read'}));
  return proof;
}
function bind(machine, proof) {
  assert(proofs.has(proof), 'exact independently proved super-member capability');
  const record = proofs.get(proof), native = machine.view === record.native, actual = machine.view === record.actual; assert(native || actual, 'exact proved super-member view');
  const item = native ? record.n : record.a;
  assert(machine.node === item.callback && machine.originals.includes(item.source), 'same original method and emitted callback');
  let owned = false; for (let node = item.read; node; node = node.parent) if (node === machine.callback) owned = true;
  assert(owned, 'exact super member is inside the proved state dispatcher');
  const result = Object.freeze({proof, read: item.read, invocation: item.invocation, role: record.role, entry: record.entry, end: record.end});
  bindings.set(result, {record, item, native}); return result;
}
function entry(capability, node) {
  assert(bindings.has(capability), 'exact bound physical super-member proof');
  const call = capability.invocation;
  return node === capability.read || node === call.expression || node === call
    || node === call.parent && call.parent.left === call ? capability.entry : undefined;
}
function paired(left, right) {
  const n = bindings.get(left), a = bindings.get(right);
  assert(n && a && n.record === a.record && n.native && !a.native, 'both super values use the same exact physical proof');
  assert(left.read === n.record.n.read && right.read === n.record.a.read, 'exact Native and actual super values');
  return n.record;
}
module.exports = {prove, bind, paired, entry};
