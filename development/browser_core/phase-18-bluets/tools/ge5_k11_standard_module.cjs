// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Bounded complete Native-derived modules for retained public identifier methods.
// The original AST supplies every class, callable, decorator and queue inventory.
const assert = require('node:assert/strict');
const tools = __dirname + '/';
const helpers = require(tools + 'ge5_decorator_helpers.cjs');
const plans = require('./ge5_decorator_member_plan.cjs');
const binding = require(tools + 'ge5_class_decorator_binding.cjs');
const frames = require(tools + 'ge5_decorator_replacement_frame.cjs');
const classFrame = require(tools + 'ge5_class_expression_frame.cjs');
const accounting = require(tools + 'ge5_class_accounting.cjs');
const { compareGap } = require(tools + 'ge5_role_stream.cjs');
const { comments } = require(tools + 'ge5_decorator_preserved.cjs');

function walk(ts, node, predicate) {
  const result = [];
  function visit(node) { if (predicate(node)) result.push(node); ts.forEachChild(node, visit); }
  visit(node); return result;
}
function inside(node, root) { return root.getStart() <= node.getStart() && node.end <= root.end; }
function same(a, b) { return JSON.stringify(a) === JSON.stringify(b); }
function property(ts, node, name) {
  assert(ts.isObjectLiteralExpression(node));
  const found = node.properties.filter(p => ts.isPropertyAssignment(p) && p.name.getText() === name);
  assert.equal(found.length, 1, 'unique context property ' + name); return found[0].initializer;
}
function expression(ts, node) {
  while (ts.isParenthesizedExpression(node) || ts.isAsExpression(node)) {
    if (ts.isAsExpression(node)) assert.equal(node.type.kind, ts.SyntaxKind.AnyKeyword, 'bounded erased decorator assertion');
    node = node.expression;
  }
  assert(ts.isIdentifier(node), 'bounded identifier decorator expression'); return node;
}
function sourceInventory(ts, original) {
  assert.equal(original.parseDiagnostics.length, 0, 'original TypeScript syntax');
  const classes = original.statements.filter(ts.isClassDeclaration);
  assert(classes.length > 0, 'at least one original decorated class');
  assert(original.statements.every(n => ts.isFunctionDeclaration(n) || ts.isClassDeclaration(n) || ts.isExpressionStatement(n)), 'bounded original module statement grammar');
  assert.equal(walk(ts, original, n => ts.isClassDeclaration(n) || ts.isClassExpression(n)).length, classes.length, 'complete original top-level class inventory');
  for (const source of classes) {
    assert(source.name && !source.heritageClauses?.length && !source.typeParameters?.length);
    assert(source.members.length > 0 && source.members.every(n => ts.isMethodDeclaration(n) && ts.isIdentifier(n.name) && !n.questionToken && !n.asteriskToken && !n.typeParameters?.length && !n.modifiers?.some(m => m.kind !== ts.SyntaxKind.Decorator)), 'bounded public instance identifier methods');
    const decorated = [...(ts.getDecorators(source) || []), ...source.members.flatMap(n => ts.getDecorators(n) || [])];
    assert(decorated.length > 0); for (const decorator of decorated) expression(ts, decorator.expression);
  }
  return { classes, functions: original.statements.filter(ts.isFunctionDeclaration), statements: [...original.statements] };
}

function compare(ts, native, actual, options, originalText) {
  assert.equal(ts.version, '5.9.3');
  assert.equal(options.target, 'ES2022'); assert.equal(options.module, 'ES2022');
  assert.equal(options.strict, true, 'bounded strict original K11 configurations');
  assert(!options.experimentalDecorators && options.useDefineForClassFields !== false, 'bounded standard ES2022 define lowering');
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata, 'complete source-map metadata tables');
  let previous;
  for (const segment of actual.decoded.segments) {
    assert(!previous || segment[0] > previous[0] || segment[0] === previous[0] && segment[1] > previous[1], 'ordered unique Blue map segments'); previous = segment;
  }
  const original = ts.createSourceFile('main.ts', originalText, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const inventory = sourceInventory(ts, original), contract = accounting.create(ts, native, actual, [originalText]);
  const sourceWidths = originalText.split('\n').map(line => line.replace(/\r$/, '').length);
  for (const view of [native, actual]) {
    assert.equal(view.decoded.metadata.sources.length, 1, 'exact complete original source inventory');
    if (view.decoded.metadata.sourcesContent) assert.deepEqual(view.decoded.metadata.sourcesContent, [originalText], 'exact original UTF-16 source contents');
    for (const segment of view.decoded.segments) if (segment.length > 2) {
      assert.equal(segment[2], 0, 'known original source index');
      assert(segment[3] < sourceWidths.length && segment[4] <= sourceWidths[segment[3]], 'every decoded source coordinate is within the original UTF-16 source');
    }
  }
  const helperProof = helpers.compare(ts, native, actual, [originalText]);
  const classProofs = [], excluded = new Map([[native, [...helperProof.native.definitions]], [actual, [...helperProof.actual.definitions]]]);
  const aliases = new Map([[native, new Map()], [actual, new Map()]]), framesByView = new Map([[native, []], [actual, []]]);
  for (const [view, state] of [[native, helperProof.native], [actual, helperProof.actual]]) for (const [name, value] of state.bindings) aliases.get(view).set(name, '$helper.' + value.kind);
  for (const view of [native, actual]) {
    const directives = view.source.statements.filter(node => ts.isExpressionStatement(node) && ts.isStringLiteral(node.expression));
    if (view === native) assert.equal(directives.length, 1, 'Native strict script directive');
    assert(directives.length <= 1, 'one generated strict directive');
    for (const directive of directives) {
      assert.equal(directive.expression.text, 'use strict');
      assert(!inventory.statements.some(node => ts.isExpressionStatement(node) && ts.isStringLiteral(node.expression)), 'source directives require their retained-source contract');
      view.noSource(directive.getStart(view.source), directive.end, 'complete generated strict directive'); excluded.get(view).push(directive);
    }
  }

  function readClass(view, state, source, blue) {
    const found = view.source.statements.filter(n => ts.isVariableStatement(n) && n.declarationList.declarations.length === 1 && n.declarationList.declarations[0].name.getText() === source.name.text);
    assert.equal(found.length, 1, 'one emitted binding for each original class');
    const root = found[0], declaration = root.declarationList.declarations[0];
    assert(root.declarationList.flags & ts.NodeFlags.Let, 'standard declaration binding uses let');
    const outer = frames.invocation(ts, view, declaration.initializer);
    const hasClassDecorators = (ts.getDecorators(source) || []).length > 0;
    const classBinding = hasClassDecorators ? binding.read(ts, view, source, { blue }) : null;
    if (classBinding) assert(binding.owns(classBinding, ts, view, source));
    const localCalls = state.calls.filter(call => inside(call, root) && call !== classBinding?.call);
    const memberState = { ...state, calls: localCalls.map(node => ({ kind: state.bindings.get(node.expression.text).kind, node })) };
    const originalPlans = plans.original(ts, source), receivers = new Set([source.name.text, classBinding?.receiver.text].filter(Boolean));
    const member = plans.read(ts, view, memberState, source, originalPlans, { receivers, scope: root });
    const alias = aliases.get(view);
    for (const [name, role] of member.aliases) alias.set(name, '$class.' + source.name.text + role);
    if (classBinding) for (const [name, role] of [[classBinding.evaluation.name.text, 'decorators'], [classBinding.descriptor.text, 'descriptor'], [classBinding.extra.name.text, 'extra'], [classBinding.receiver.text, 'receiver']]) alias.set(name, '$class.' + source.name.text + '.' + role);
    const classValues = walk(ts, outer.body, ts.isClassExpression);
    assert.equal(classValues.length, 1, 'one complete original class value');
    const value = classValues[0], blocks = value.members.filter(ts.isClassStaticBlockDeclaration), methods = value.members.filter(ts.isMethodDeclaration), ctors = value.members.filter(ts.isConstructorDeclaration);
    assert.equal(methods.length, source.members.length, 'complete original method inventory');
    assert.deepEqual(methods.map(n => n.name.getText()), source.members.map(n => n.name.getText()));
    assert(value.members.every(n => blocks.includes(n) || methods.includes(n) || ctors.includes(n)), 'no unproved emitted class members');
    const calls = [...member.entries.values()].map(entry => entry.call), bodies = new Set(calls.map(call => call.parent.parent));
    assert.equal(bodies.size, 1, 'one source member decoration phase'); const metadata = [...bodies][0];
    assert(ts.isBlock(metadata) && blocks.some(block => block.body === metadata));
    assert.equal(blocks.length, 1 + Number(hasClassDecorators), 'complete source-derived static phase inventory');
    const metadataVariables = metadata.statements.filter(ts.isVariableStatement);
    assert.equal(metadataVariables.length, 1); assert.equal(metadataVariables[0].declarationList.declarations.length, 1);
    const metadataDeclaration = metadataVariables[0].declarationList.declarations[0];
    assert(ts.isIdentifier(metadataDeclaration.name)); alias.set(metadataDeclaration.name.text, '$class.' + source.name.text + '.metadata');
    const decoratorAssignments = [];
    for (const [plan, entry] of member.entries) {
      const assignments = metadata.statements.filter(n => ts.isExpressionStatement(n) && ts.isBinaryExpression(n.expression) && n.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken && n.expression.left.getText() === entry.decorators.text);
      assert.equal(assignments.length, 1, 'one complete method decorator evaluation');
      const array = assignments[0].expression.right; assert(ts.isArrayLiteralExpression(array));
      assert.equal(array.elements.length, plan.decorators.length);
      for (let i = 0; i < array.elements.length; i++) assert.equal(expression(ts, array.elements[i]).text, expression(ts, plan.decorators[i].expression).text, 'original decorator expression spelling and order');
      decoratorAssignments.push(assignments[0]);
    }
    const publication = metadata.statements.find(n => ts.isIfStatement(n)); assert(publication);
    assert(!publication.elseStatement && publication.expression.getText() === metadataDeclaration.name.text);
    const publish = publication.thenStatement.expression;
    assert(ts.isCallExpression(publish) && publish.expression.getText() === 'Object.defineProperty');
    assert.equal(publish.arguments.length, 3); assert.equal(publish.arguments[1].getText(), 'Symbol.metadata');
    const descriptors = plans.properties(ts, publish.arguments[2], ['enumerable', 'configurable', 'writable', 'value']);
    for (const flag of descriptors.slice(0, 3)) assert.equal(flag.kind, ts.SyntaxKind.TrueKeyword);
    assert.equal(descriptors[3].getText(), metadataDeclaration.name.text);
    const instanceQueue = [...member.entries.values()].map(entry => entry.extra.text);
    assert.equal(new Set(instanceQueue).size, Number(originalPlans.length > 0), 'one instance method extra queue');
    const instanceCalls = localCalls.filter(call => state.bindings.get(call.expression.text).kind === 'runInitializers' && call.arguments[0]?.kind === ts.SyntaxKind.ThisKeyword);
    assert.equal(ctors.length, Number(originalPlans.length > 0));
    if (ctors.length) {
      assert.equal(ctors[0].parameters.length, 0); assert.equal(ctors[0].body.statements.length, 1);
      assert.equal(instanceCalls.length, 1); const extra = instanceCalls[0];
      assert(inside(extra, ctors[0])); assert.equal(extra.arguments.length, 2); assert.equal(extra.arguments[1].getText(), instanceQueue[0]);
    }
    const prefix = [...outer.body.statements]; const returned = prefix.pop(); assert(ts.isReturnStatement(returned));
    const expectedSlots = new Set([...member.entries.values()].flatMap(entry => [entry.decorators.text, entry.extra.text]));
    let nameSetter = null;
    if (classBinding) {
      assert.equal(classBinding.root, root); assert.equal(classBinding.metadata, metadataDeclaration);
      assert.equal(blocks[1].body, metadata); const namePhase = blocks[0].body;
      assert.equal(namePhase.statements.length, blue ? 2 : 1);
      if (blue) {
        nameSetter = namePhase.statements[0]; classFrame.definedName(ts, view, nameSetter, 'this', source.name.text);
        view.noSource(nameSetter.getStart(view.source), nameSetter.end, 'complete generated source-derived class name setter'); excluded.get(view).push(nameSetter);
      }
      const capture = namePhase.statements.at(-1); assert(ts.isExpressionStatement(capture));
      assert.equal(frames.assign(ts, capture.expression, classBinding.receiver.text).kind, ts.SyntaxKind.ThisKeyword);
      assert.equal(value.name, undefined, 'Native inferred source class name');
      const construction = prefix.pop(); assert(ts.isVariableStatement(construction));
      assert.equal(construction.declarationList.declarations.length, 1);
      const ctorBinding = construction.declarationList.declarations[0]; assert.equal(ctorBinding.name.getText(), source.name.text); assert.equal(ctorBinding.initializer, value);
      const completion = frames.assign(ts, returned.expression, source.name.text); assert(ts.isIdentifier(completion)); assert.equal(completion.text, classBinding.receiver.text);
      for (const slot of [classBinding.evaluation.name.text, classBinding.descriptor.text, classBinding.extra.name.text, classBinding.receiver.text]) expectedSlots.add(slot);
      assert.equal(publish.arguments[0].getText(), classBinding.receiver.text);
      const classExtra = localCalls.filter(call => state.bindings.get(call.expression.text).kind === 'runInitializers' && call.arguments[0]?.getText() === classBinding.receiver.text);
      assert.equal(classExtra.length, 1); assert.equal(classExtra[0].arguments.length, 2); assert.equal(classExtra[0].arguments[1].getText(), classBinding.extra.name.text);
      assert.deepEqual([...metadata.statements], [metadataVariables[0], ...decoratorAssignments, ...calls.map(call => call.parent), classBinding.call.parent, classBinding.assignment.parent.parent, publication, classExtra[0].parent], 'complete class/member decorator evaluation and replacement order');
    } else {
      assert.equal(returned.expression, value); assert.equal(value.name?.text, source.name.text);
      assert.equal(blocks[0].body, metadata); assert.equal(publish.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
      assert.deepEqual([...metadata.statements], [metadataVariables[0], ...decoratorAssignments, ...calls.map(call => call.parent), publication], 'complete method metadata phase statement inventory');
    }
    const declared = [];
    for (const statement of prefix) {
      assert(ts.isVariableStatement(statement) && statement.declarationList.declarations.length === 1);
      const slot = statement.declarationList.declarations[0]; assert(ts.isIdentifier(slot.name)); declared.push(slot.name.text);
    }
    assert.equal(new Set(declared).size, declared.length); assert.deepEqual([...declared].sort(), [...expectedSlots].sort(), 'complete source-derived per-class state inventory');
    return { source, root, declaration, outer, value, methods, constructor: ctors[0], metadata, member, classBinding, nameSetter };
  }

  for (const source of inventory.classes) {
    const n = readClass(native, helperProof.native, source, false), a = readClass(actual, helperProof.actual, source, true);
    framesByView.get(native).push(n); framesByView.get(actual).push(a); classProofs.push({ source, native: { ...n, node: n.root, native: true }, actual: { ...a, node: a.root, native: true } });
  }
  function normalized(view) {
    return view.tokens(view.source).filter(token => !excluded.get(view).some(node => node.getStart(view.source) <= token.start && token.end <= node.end)).map(token => {
      const alias = aliases.get(view).get(token.text); if (!alias) return token;
      const owner = token.owners[0]; assert(token.owners.every(role => same(role, owner)), 'complete bound alias UTF-16 interior');
      return { ...token, text: alias, owners: [owner] };
    });
  }
  const nt = normalized(native), at = normalized(actual);
  contract.pair(nt, at, 'complete K11 standard module semantic tokens');
  const endpoints = [];
  function logicalNodes(view) {
    const all = [];
    function visit(node) {
      if (excluded.get(view).includes(node)) return;
      if (node !== view.source && (!ts.isToken(node) || ts.isIdentifier(node) || ts.isLiteralExpression(node) || [ts.SyntaxKind.ThisKeyword, ts.SyntaxKind.NullKeyword, ts.SyntaxKind.TrueKeyword, ts.SyntaxKind.FalseKeyword].includes(node.kind))) all.push(node);
      ts.forEachChild(node, visit);
    }
    visit(view.source); return all;
  }
  const nn = logicalNodes(native), an = logicalNodes(actual);
  assert.equal(an.length, nn.length, 'complete logical AST-node inventory');
  for (let index = 0; index < nn.length; index++) {
    assert.equal(an[index].kind, nn[index].kind, 'same logical AST-node kind at ' + index);
    assert.deepEqual(actual.owner(an[index].end), native.owner(nn[index].end), 'exact logical AST endpoint ' + index + ' ' + ts.SyntaxKind[nn[index].kind] + ': ' + nn[index].getText(native.source).slice(0, 80));
    endpoints.push({ kind: ts.SyntaxKind[nn[index].kind], native: nn[index].end, actual: an[index].end, owner: native.owner(nn[index].end) });
  }
  for (const view of [native, actual]) for (const generated of excluded.get(view)) {
    assert.equal(view.owner(generated.end), null, 'generated helper/directive/name-setter logical endpoint stays unmapped');
    if (view === actual) endpoints.push({ kind: 'Generated.' + ts.SyntaxKind[generated.kind], native: null, actual: generated.end, owner: null });
  }
  const originalComments = comments(ts, { code: originalText }), nc = comments(ts, native), ac = comments(ts, actual);
  for (const [view, list] of [[native, nc], [actual, ac]]) {
    const source = list.filter(comment => !comment.text.startsWith('//# sourceMappingURL='));
    assert.deepEqual(source.map(comment => comment.text), originalComments.map(comment => comment.text), 'complete original comment inventory');
    const footers = list.filter(comment => comment.text.startsWith('//# sourceMappingURL=')); assert.equal(footers.length, 1);
    assert.equal(footers[0].text, nc.find(comment => comment.text.startsWith('//# sourceMappingURL=')).text, 'exact external map publication');
    view.noSource(footers[0].start, footers[0].end, 'complete generated map publication');
  }
  for (let index = 0; index < originalComments.length; index++) {
    assert.deepEqual(actual.owner(ac[index].start), native.owner(nc[index].start), 'source comment start');
    assert.deepEqual(actual.owner(ac[index].end), native.owner(nc[index].end), 'source comment logical endpoint');
  }
  contract.finish(classProofs);
  native.complete('every Native module UTF-16 role and ordered gap'); actual.complete('every Blue module UTF-16 role and ordered gap');
  return { source: original, inventory, helperProof, classProofs, endpoints, nativeTokens: nt, actualTokens: at,
    nativeCharacters: native.code.length, actualCharacters: actual.code.length, allCharactersClassified: true, completeModuleRoles: true };
}
module.exports = { compare, sourceInventory, expression };
