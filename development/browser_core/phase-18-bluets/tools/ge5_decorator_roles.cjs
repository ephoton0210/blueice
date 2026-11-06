// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete decorator-call roles; enclosing class/module roles compose separately.
const assert = require('node:assert/strict');
const helper = require('./ge5_decorator_helpers.cjs');
const {compareGap} = require('./ge5_role_stream.cjs');

function imports(ts, view, originals) {
  const sourceNames = new Set(), aliases = new Map(), tslib = new Set();
  for (const text of originals) {
    const ast = ts.createSourceFile('original.ts', text, ts.ScriptTarget.Latest, true);
    function names(node) { if (ts.isIdentifier(node)) sourceNames.add(node.text); ts.forEachChild(node, names); }
    names(ast);
  }
  function binding(name, module) {
    if (sourceNames.has(name)) return;
    aliases.set(name, '$module.' + module); if (module === 'tslib') tslib.add(name);
  }
  function visit(node) {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      const imports = node.importClause?.namedBindings;
      if (imports && ts.isNamespaceImport(imports)) binding(imports.name.text, node.moduleSpecifier.text);
    }
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer
      && ts.isCallExpression(node.initializer) && node.initializer.expression.getText() === 'require'
      && node.initializer.arguments.length === 1 && ts.isStringLiteral(node.initializer.arguments[0])) {
      binding(node.name.text, node.initializer.arguments[0].text);
    }
    if (ts.isCallExpression(node) && node.arguments.length === 2 && ts.isArrayLiteralExpression(node.arguments[0])
      && ts.isFunctionExpression(node.arguments[1])) {
      const dependencies = node.arguments[0].elements, factory = node.arguments[1];
      if (node.expression.getText() === 'define') {
        dependencies.forEach((dependency, index) => {
          if (ts.isStringLiteral(dependency) && dependency.text === 'tslib') {
            // Blue's envelope acquires dependencies through its checked local
            // require ABI; Native also permits a factory parameter binding.
            if (!factory.parameters[index]) return;
            assert(ts.isIdentifier(factory.parameters[index].name));
            binding(factory.parameters[index].name.text, dependency.text);
          }
        });
      } else if (node.expression.getText() === 'System.register') {
        const returned = factory.body.statements.at(-1);
        assert(ts.isReturnStatement(returned) && ts.isObjectLiteralExpression(returned.expression));
        const setters = returned.expression.properties.find(property => property.name.getText() === 'setters')?.initializer;
        assert(ts.isArrayLiteralExpression(setters));
        dependencies.forEach((dependency, index) => {
          if (!ts.isStringLiteral(dependency) || dependency.text !== 'tslib') return;
          const setter = setters.elements[index]; assert(ts.isFunctionExpression(setter));
          const parameter = setter.parameters[0].name; assert(ts.isIdentifier(parameter));
          const assignments = setter.body.statements.filter(statement => ts.isExpressionStatement(statement)
            && ts.isBinaryExpression(statement.expression) && statement.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken
            && ts.isIdentifier(statement.expression.left) && ts.isIdentifier(statement.expression.right)
            && statement.expression.right.text === parameter.text);
          assert.equal(assignments.length, 1, 'one authorized helper namespace setter');
          binding(assignments[0].expression.left.text, dependency.text);
        });
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(view.source);
  return {aliases, tslib, sourceNames};
}

function read(ts, view, originals, blue) {
  const state = helper.read(ts, view, originals, blue), imported = imports(ts, view, originals);
  function role(expression) {
    if (ts.isIdentifier(expression)) return helper.helpers.get(expression.text);
    if (ts.isPropertyAccessExpression(expression) && ts.isIdentifier(expression.expression)
      && imported.tslib.has(expression.expression.text)) return helper.helpers.get(expression.name.text);
    return undefined;
  }
  const calls = [], references = [];
  function visit(node, inside = false) {
    if (state.definitions.includes(node)) return;
    const kind = ts.isCallExpression(node) && role(node.expression);
    if (kind) {
      references.push({start: node.expression.getStart(view.source), end: node.expression.end, text: '$helper.' + kind});
      if (!inside) calls.push({node, kind});
    }
    ts.forEachChild(node, child => { visit(child, inside || !!kind); });
  }
  visit(view.source);
  references.push(...require('./ge5_decorator_numeric_keys.cjs').read(ts, view, originals, calls));
  function units(node) {
    const result = []; let until = -1;
    for (const token of view.tokens(node)) {
      if (token.start < until) continue;
      const reference = references.find(item => item.start === token.start && item.end <= node.end);
      const alias = imported.aliases.get(token.text), end = reference?.end || token.end;
      if (reference || alias) {
        const owner = view.owner(token.start);
        for (let offset = token.start; offset < end; offset++) {
          assert.deepEqual(view.owner(offset), owner, 'complete generated-reference UTF-16 interior');
        }
        result.push({start: token.start, end, text: reference?.text || alias, owner, reference: true,
          numericMethodKey: !!reference?.numericMethodKey}); until = end;
      } else if (/^["']/.test(token.text) && token.owners.every(owner => owner === null)) {
        const scanner = ts.createScanner(ts.ScriptTarget.Latest, true, ts.LanguageVariant.Standard, token.text);
        assert.equal(scanner.scan(), ts.SyntaxKind.StringLiteral);
        result.push({...token, text: JSON.stringify(scanner.getTokenValue()), owner: null, reference: true});
      } else result.push(token);
    }
    return result;
  }
  return {...state, ...imported, calls, references, units};
}

function compare(ts, native, actual, originals) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata, 'complete map metadata/names');
  if (native.code === actual.code) assert.deepEqual(actual.decoded, native.decoded, 'identical JavaScript requires every decoded entry');
  const n = read(ts, native, originals, false), a = read(ts, actual, originals, true);
  const expected = [...n.bindings.values()].map(item => item.kind).sort();
  assert.deepEqual([...a.bindings.values()].map(item => item.kind).sort(), expected, 'complete decorator helper inventory');
  for (const value of n.bindings.values()) {
    const observed = [...a.bindings.values()].find(item => item.kind === value.kind);
    assert.deepEqual(observed.shape, value.shape, value.kind + ': complete independent helper AST');
  }
  assert.deepEqual(a.calls.map(call => call.kind), n.calls.map(call => call.kind), 'complete ordered decorator-call inventory');
  const pairs = [], statementPairs = [];
  for (let index = 0; index < n.calls.length; index++) {
    const wanted = n.units(n.calls[index].node), observed = a.units(a.calls[index].node);
    assert.equal(observed.length, wanted.length, 'complete decorator-call token inventory');
    for (let at = 0; at < wanted.length; at++) {
      const x = wanted[at], y = observed[at];
      assert.equal(y.text, x.text, 'decorator call ' + index + ': ordered token ' + at);
      assert.equal(!!y.reference, !!x.reference, 'same generated-reference role');
      assert.equal(!!y.numericMethodKey, !!x.numericMethodKey, 'same original numeric method key role');
      if (x.numericMethodKey) assert.deepEqual(actual.owner(y.end), native.owner(x.end), 'numeric method key exact endpoint');
      if (x.reference) assert.deepEqual(y.owner, x.owner, 'Native generated-reference owner: ' + x.text);
      else assert.deepEqual(y.owners, x.owners, 'decorator call ' + index + ': every UTF-16 interior of ' + x.text);
      native.cover(x.start, x.end); actual.cover(y.start, y.end);
      if (at) compareGap(native, wanted[at - 1].end, x.start, actual, observed[at - 1].end, y.start, 'decorator call ' + index + ': ordered generated gap ' + at);
    }
    assert.deepEqual(actual.owner(a.calls[index].node.end), native.owner(n.calls[index].node.end), 'decorator call endpoint');
    pairs.push({kind: n.calls[index].kind, native: n.calls[index].node, actual: a.calls[index].node});
    function statement(call) {
      let node = call;
      if (ts.isBinaryExpression(node.parent) && node.parent.right === node
        && node.parent.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(node.parent.left)) node = node.parent;
      return ts.isExpressionStatement(node.parent) ? node.parent : null;
    }
    const ns = statement(n.calls[index].node), as = statement(a.calls[index].node);
    if (ns && as) {
      const nt = n.units(ns), at = a.units(as);
      assert.equal(at.length, nt.length, 'complete decorator statement inventory');
      for (let position = 0; position < nt.length; position++) {
        const x = nt[position], y = at[position];
        assert.equal(y.text, x.text, 'complete decorator statement token');
        if (x.reference) assert.deepEqual(y.owner, x.owner, 'decorator statement generated reference');
        else assert.deepEqual(y.owners, x.owners, 'decorator statement token interiors');
        native.cover(x.start, x.end); actual.cover(y.start, y.end);
        if (position) compareGap(native, nt[position - 1].end, x.start, actual, at[position - 1].end, y.start, 'complete decorator statement gap');
      }
      statementPairs.push({native: ns, actual: as});
    }
  }
  return {native: n, actual: a, pairs, statementPairs, scope: 'complete decorator helper definitions/calls; enclosing class/module contract required'};
}
module.exports = {read, compare, imports};
