// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Capabilities belong to original private members and independently parsed emitted phases.
const assert = require('node:assert/strict');
const unwrap = (ts, node) => { while (node && ts.isParenthesizedExpression(node)) node = node.expression; return node; };
function walk(ts, root, predicate) { const result = []; function visit(node) { if (predicate(node)) result.push(node); ts.forEachChild(node, visit); } visit(root); return result; }
function properties(ts, node, expected) {
  assert(ts.isObjectLiteralExpression(node));
  assert(node.properties.every(ts.isPropertyAssignment));
  assert.deepEqual(node.properties.map(p => p.name.text ?? p.name.getText()), expected);
  return Object.fromEntries(node.properties.map(p => [p.name.text ?? p.name.getText(), p.initializer]));
}
function source(ts, text) {
  const original = typeof text === 'string' ? ts.createSourceFile('original.ts', text, ts.ScriptTarget.Latest, true) : text;
  assert(ts.isSourceFile(original));
  const classes = original.statements.filter(ts.isClassDeclaration);
  assert.equal(classes.length, 2, 'complete original base and private derived class inventory');
  const derived = classes.filter(c => c.heritageClauses?.length);
  assert.equal(derived.length, 1); const cls = derived[0];
  assert(cls.name && !ts.getDecorators(cls)?.length);
  assert.equal(cls.heritageClauses.length, 1); assert.equal(cls.heritageClauses[0].types.length, 1);
  const base = cls.heritageClauses[0].types[0].expression;
  assert(ts.isIdentifier(base) && classes.some(c => c.name?.text === base.text));
  const members = cls.members.filter(m => ts.getDecorators(m)?.length).map((node, index) => {
    assert(ts.isPrivateIdentifier(node.name));
    assert(ts.isMethodDeclaration(node) || ts.isGetAccessorDeclaration(node) || ts.isSetAccessorDeclaration(node));
    assert(node.body && !walk(ts, node.body, n => n.kind === ts.SyntaxKind.SuperKeyword).length,
      'safe home-object body has no unsafe lowered lexical super operation');
    const decorators = ts.getDecorators(node); assert(decorators.every(d => ts.isIdentifier(d.expression)));
    const kind = ts.isMethodDeclaration(node) ? 'method' : ts.isGetAccessorDeclaration(node) ? 'getter' : 'setter';
    return {node, index, key: node.name.text, kind, static: !!node.modifiers?.some(m => m.kind === ts.SyntaxKind.StaticKeyword), decorators};
  });
  assert(members.length && members.every(m => m.static === members[0].static));
  assert(cls.members.every(m => members.some(p => p.node === m) || ts.isMethodDeclaration(m)),
    'complete original safe private/public callable inventory');
  return {original, cls, base, members, classes};
}
function read(ts, view, cap, blue) {
  const name = cap.cls.name.text, cores = walk(ts, view.source, n => ts.isClassExpression(n) && n.name?.text === name);
  assert.equal(cores.length, 1, 'one exact source-bound private derived class construction'); const core = cores[0];
  const variables = view.source.statements.filter(ts.isVariableStatement).flatMap(s => [...s.declarationList.declarations]).filter(d => ts.isIdentifier(d.name) && d.name.text === name);
  assert.equal(variables.length, 1); const binding = variables[0];
  const outerCall = unwrap(ts, binding.initializer); assert(ts.isCallExpression(outerCall) && !outerCall.arguments.length);
  const outer = unwrap(ts, outerCall.expression); assert(ts.isArrowFunction(outer) && !outer.parameters.length && ts.isBlock(outer.body));
  const returned = outer.body.statements.at(-1); assert(ts.isReturnStatement(returned));
  const helperName = blue ? '__bluetsEsDecorate' : '__esDecorate';
  const calls = walk(ts, core.parent === returned ? core : outer, n => ts.isCallExpression(n) && ts.isIdentifier(n.expression) && n.expression.text === helperName);
  assert.equal(calls.length, cap.members.length, 'one exact private decorator event per original private callable');
  const entries = calls.map(call => {
    assert.equal(call.arguments.length, 6);
    const context = properties(ts, call.arguments[3], ['kind', 'name', 'static', 'private', 'access', 'metadata']);
    assert(ts.isStringLiteral(context.kind) && ts.isStringLiteral(context.name));
    assert.equal(context.private.kind, ts.SyntaxKind.TrueKeyword);
    assert([ts.SyntaxKind.TrueKeyword, ts.SyntaxKind.FalseKeyword].includes(context.static.kind));
    const matches = cap.members.filter(m => m.kind === context.kind.text && m.key === context.name.text && m.static === (context.static.kind === ts.SyntaxKind.TrueKeyword));
    assert.equal(matches.length, 1, 'exact original kind/key/static identity owns this event'); const member = matches[0];
    assert(ts.isIdentifier(call.arguments[2]) && ts.isIdentifier(call.arguments[5]));
    assert.equal(call.arguments[4].kind, ts.SyntaxKind.NullKeyword);
    let descriptor = call.arguments[1], assignment;
    if (blue) {
      assert(ts.isIdentifier(descriptor));
      const found = walk(ts, outer, n => ts.isBinaryExpression(n) && n.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(n.left) && n.left.text === descriptor.text && ts.isObjectLiteralExpression(n.right));
      assert.equal(found.length, 1); assignment = found[0];
    } else { assignment = descriptor; assert(ts.isBinaryExpression(assignment) && assignment.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(assignment.left)); descriptor = assignment.left; }
    const property = member.kind === 'method' ? 'value' : member.kind === 'getter' ? 'get' : 'set';
    const definition = properties(ts, assignment.right, [property])[property]; assert(ts.isCallExpression(definition));
    const fn = definition.arguments[0]; assert(ts.isFunctionExpression(fn));
    const expectedName = (member.kind === 'method' ? '' : member.kind === 'getter' ? 'get ' : 'set ') + member.key;
    if (blue) {
      assert.equal(definition.expression.getText(view.source), 'Object.defineProperty'); assert.equal(definition.arguments.length, 3);
      assert(ts.isStringLiteral(definition.arguments[1]) && definition.arguments[1].text === 'name');
      const descriptor = properties(ts, definition.arguments[2], ['value', 'configurable']);
      assert(ts.isStringLiteral(descriptor.value) && descriptor.value.text === expectedName); assert.equal(descriptor.configurable.kind, ts.SyntaxKind.TrueKeyword);
      assert(fn.name && !walk(ts, cap.original, n => ts.isIdentifier(n) && n.text === fn.name.text).length, 'private callable synthetic name is fresh and overwritten by the proved exact source name');
    } else {
      assert(ts.isIdentifier(definition.expression) && definition.expression.text === '__setFunctionName');
      assert.equal(definition.arguments.length, member.kind === 'method' ? 2 : 3);
      assert(ts.isStringLiteral(definition.arguments[1]) && definition.arguments[1].text === member.key);
      if (member.kind !== 'method') assert(ts.isStringLiteral(definition.arguments[2]) && definition.arguments[2].text === property);
      assert(!fn.name);
    }
    const effective = member.node.parameters.filter(p => p.name.getText(cap.original) !== 'this');
    assert.equal(fn.parameters.length, effective.length, 'private callable original effective arity');
    assert.equal(!!fn.asteriskToken, !!member.node.asteriskToken, 'private callable generator kind');
    return {member, call, context, descriptor, assignment, definition, fn, property};
  });
  assert.deepEqual(entries.map(e => e.member.index), cap.members.map(m => m.index), 'complete ordered original private decoration inventory');
  let metadataBody = calls[0].parent; while (metadataBody && !ts.isBlock(metadataBody)) metadataBody = metadataBody.parent;
  assert(metadataBody && calls.every(c => { for (let n = c; n; n = n.parent) if (n === metadataBody) return true; return false; }));
  const metadata = metadataBody.statements.filter(ts.isVariableStatement).flatMap(s => [...s.declarationList.declarations]).filter(d => ts.isIdentifier(d.name) && d.initializer && ts.isConditionalExpression(d.initializer));
  assert.equal(metadata.length, 1, 'one exact inherited metadata allocation');
  const classSuper = outer.body.statements.filter(ts.isVariableStatement).flatMap(s => [...s.declarationList.declarations]).filter(d => ts.isIdentifier(d.name) && d.initializer && ts.isIdentifier(d.initializer) && d.initializer.text === cap.base.text);
  assert.equal(classSuper.length, 1, 'one exact original heritage value capture');
  let classValue = core.parent; while (ts.isParenthesizedExpression(classValue)) classValue = classValue.parent;
  const receiver = ts.isBinaryExpression(classValue) && classValue.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(classValue.left) ? classValue.left.text : name;
  assert(entries.every(e => e.call.arguments[5].text === entries[0].call.arguments[5].text), 'one exact shared private extra-initializer queue');
  const slots = [classSuper[0].name.text, metadata[0].name.text, entries[0].call.arguments[5].text, ...entries.flatMap(e => [e.descriptor.text, e.call.arguments[2].text])];
  assert.equal(new Set(slots).size, slots.length, 'distinct complete generated private evaluation bindings');
  const originalNames = new Set(walk(ts, cap.original, ts.isIdentifier).map(id => id.text));
  assert(slots.every(name => !originalNames.has(name)), 'every private evaluation slot is fresh in the original source');
  const aliases = new Map([[receiver, '$class.value'], [metadata[0].name.text, '$class.metadata'], [classSuper[0].name.text, '$class.super']]);
  for (const e of entries) { aliases.set(e.descriptor.text, '$private.' + e.member.index + '.descriptor'); aliases.set(e.call.arguments[2].text, '$private.' + e.member.index + '.decorators'); aliases.set(e.call.arguments[5].text, '$class.extra'); }
  return {view, blue, cap, core, binding, outerCall, outer, returned, entries, metadataBody, metadata: metadata[0], classSuper: classSuper[0], receiver, aliases};
}
module.exports = {source, read, walk, properties, unwrap};
