// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Helper algorithms and inherited metadata are generated capabilities, not token holes.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const ir = require('./ge5_safe_home_ir.cjs'), helpers = require('./ge5_decorator_helpers.cjs');
const names = {Get: ['__classPrivateFieldGet', '__bluetsClassPrivateGet'], Set: ['__classPrivateFieldSet', '__bluetsClassPrivateSet'], In: ['__classPrivateFieldIn', '__bluetsClassPrivateIn']};
function compare(ts, n, a, options) {
  const proof = helpers.compare(ts, n.view, a.view, [n.cap.original.text]);
  for (const [p, defs] of [[n, proof.native], [a, proof.actual]]) for (const [name, def] of defs.bindings) p.aliases.set(name, '$helper.' + def.kind);
  const originalNames = new Set(ir.walk(ts, n.cap.original, ts.isIdentifier).map(n => n.text));
  function read(p) {
    const result = new Map();
    for (const statement of ir.walk(ts, p.view.source, ts.isVariableStatement)) {
      if (statement.declarationList.declarations.length !== 1) continue;
      const d = statement.declarationList.declarations[0]; if (!ts.isIdentifier(d.name)) continue;
      const role = Object.keys(names).find(role => d.name.text === names[role][Number(p.blue)]);
      if (!role) continue;
      assert(!originalNames.has(d.name.text) && !result.has(role));
      let fn = d.initializer;
      if (!p.blue) { assert(ts.isBinaryExpression(fn) && fn.operatorToken.kind === ts.SyntaxKind.BarBarToken); const fallback = ir.unwrap(ts, fn.left); assert(ts.isBinaryExpression(fallback) && fallback.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken && fallback.left.kind === ts.SyntaxKind.ThisKeyword && ts.isPropertyAccessExpression(fallback.right) && fallback.right.expression.kind === ts.SyntaxKind.ThisKeyword && fallback.right.name.text === d.name.text); fn = fn.right; }
      assert(ts.isFunctionExpression(fn) && !fn.name && !fn.asteriskToken && !fn.modifiers?.length);
      p.view.noSource(statement.getStart(p.view.source), statement.end, 'complete owned private ' + role + ' helper');
      p.aliases.set(d.name.text, '$helper.private.' + role); result.set(role, {statement, fn});
    }
    return result;
  }
  const np = read(n), ap = read(a); assert.deepEqual([...ap.keys()].sort(), [...np.keys()].sort(), 'complete private helper algorithm inventory');
  assert.deepEqual([...np.keys()].sort(), options.target === 'ES2015' ? n.cap.members.some(m => m.kind === 'setter') ? ['Get', 'In', 'Set'] : ['Get', 'In'] : []);
  for (const [role, definition] of np) assert.deepEqual(helpers.semantic(ts, ap.get(role).fn), helpers.semantic(ts, definition.fn), 'independent complete private ' + role + ' helper algorithm');
  // The Native naming helper is used only on the original, proven private string names.
  const nameHelpers = n.view.source.statements.filter(ts.isVariableStatement).filter(s => s.declarationList.declarations.some(d => d.name.text === '__setFunctionName'));
  assert.equal(nameHelpers.length, 1); n.view.noSource(nameHelpers[0].getStart(n.view.source), nameHelpers[0].end, 'Native private string naming intrinsic algorithm');
  const sourceAsync = ir.walk(ts, n.cap.original, node => ts.isMethodDeclaration(node) && node.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword));
  const asyncAsset = ts.createSourceFile('async.js', fs.readFileSync(path.resolve(__dirname, '../../../../backend/bluets/src/emitter/targets/async_helpers.v1.js'), 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const asset = asyncAsset.statements.find(ts.isFunctionDeclaration); assert(asset);
  for (const p of [n, a]) {
    const defs = p.view.source.statements.filter(node => p.blue ? ts.isFunctionDeclaration(node) && /^__blueice_target_async_\d+$/.test(node.name?.text) : ts.isVariableStatement(node) && node.declarationList.declarations.length === 1 && node.declarationList.declarations[0].name.text === '__awaiter');
    assert.equal(defs.length, options.target === 'ES2015' && sourceAsync.length ? p.blue ? sourceAsync.length : 1 : 0, 'complete original async callable driver helper inventory');
    for (const node of defs) {
      if (p.blue) { assert(!originalNames.has(node.name.text)); assert.deepEqual(helpers.semantic(ts, node), helpers.semantic(ts, asset), 'complete owned async helper algorithm'); p.aliases.set(node.name.text, '$helper.async'); }
      else p.aliases.set('__awaiter', '$helper.async');
      p.view.noSource(node.getStart(p.view.source), node.end, 'complete generated safe-home callable promise helper');
    }
  }
  const metadata = [];
  for (const p of [n, a]) {
    const base = require('./ge5_class_expression_metadata.cjs').metadataBase(ts, {...p, original: p.cap.cls, metadataBlock: p.metadataBody});
    const statement = p.metadata.parent.parent; assert(ts.isVariableStatement(statement));
    p.view.noSource(statement.getStart(p.view.source), statement.end, 'complete generated inherited metadata allocation');
    if (base.hoist) p.view.noSource(base.hoist.getStart(p.view.source), base.hoist.end, 'Native inherited metadata capture declaration');
    if (base.helper) {
      assert(p.blue && options.target === 'ES2015');
      const found = p.view.source.statements.filter(s => ts.isFunctionDeclaration(s) && s.name.text === base.helper); assert.equal(found.length, 1);
      const expected = ts.createSourceFile('nullish.js', 'function f(value){return value === null || value === void 0;}', ts.ScriptTarget.Latest, true, ts.ScriptKind.JS).statements[0];
      assert.deepEqual(helpers.semantic(ts, found[0]), helpers.semantic(ts, expected), 'complete strict, noncoercing inherited metadata nullish test');
      p.view.noSource(found[0].getStart(p.view.source), found[0].end, 'complete generated inherited metadata nullish helper');
      const slot = p.view.source.statements.filter(ts.isVariableStatement).filter(s => s.declarationList.declarations.some(d => d.name.text === base.slot));
      assert.equal(slot.length, 1); assert.equal(slot[0].declarationList.declarations.length, 1); assert(!slot[0].declarationList.declarations[0].initializer);
      p.view.noSource(slot[0].getStart(p.view.source), slot[0].end, 'exact fresh inherited metadata lookup capture');
    }
    metadata.push({profile: p, base, statement});
  }
  return {decorators: proof, private: {native: np, actual: ap}, metadata};
}
module.exports = {compare};
