// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original edges bind emitted namespaces before any reference can be projected.
const assert = require('node:assert/strict');
const source = require('./ge5_interop_source.cjs'), registry = new WeakMap();
function read(ts, item, original, filename, blue) {
  const module = source.checked(original).modules[filename], view = item.view;
  assert(module && view.source.text === view.code);
  const symbols = {}, acquisitions = [], names = new Set();
  function visit(node) { if (ts.isIdentifier(node)) names.add(node.text); ts.forEachChild(node, visit); }
  visit(module.source);
  function bind(edge, namespace, node, direct = false) {
    assert(edge.bindings.length <= 1, 'reviewed complete original import binding inventory');
    const binding = edge.bindings[0];
    if (binding?.imported === '*') assert.equal(namespace, binding.local, 'original namespace import identity');
    else if (namespace && !direct) assert(!names.has(namespace), 'fresh source-import transport namespace');
    if (binding) {
      assert(!Object.hasOwn(symbols, binding.local));
      symbols[binding.local] = Object.freeze({local: binding.local, imported: binding.imported,
        specifier: edge.specifier, namespace, direct: direct || binding.imported === '*', node, original: edge.node});
    }
    acquisitions.push(Object.freeze({edge, namespace, node}));
  }
  const preserved = item.body.some(node => ts.isImportDeclaration(node));
  if (preserved) {
    const declarations = item.body.filter(ts.isImportDeclaration);
    const expected = module.imports.filter(edge => !edge.star);
    assert.equal(declarations.length, expected.length);
    for (let index = 0; index < expected.length; index++) {
      const edge = expected[index], node = declarations[index], binding = edge.bindings[0];
      assert(ts.isStringLiteral(node.moduleSpecifier)); assert.equal(node.moduleSpecifier.text, edge.specifier);
      const clause = node.importClause; assert(clause && !clause.isTypeOnly && binding);
      if (binding.imported === 'default') assert.equal(clause.name?.text, binding.local);
      else if (binding.imported === '*') assert(ts.isNamespaceImport(clause.namedBindings) && clause.namedBindings.name.text === binding.local);
      else {
        assert(ts.isNamedImports(clause.namedBindings) && clause.namedBindings.elements.length === 1);
        const element = clause.namedBindings.elements[0];
        assert.equal(element.propertyName?.text || element.name.text, binding.imported);
        assert.equal(element.name.text, binding.local); assert(!element.isTypeOnly);
      }
      bind(edge, binding.local, node, true);
    }
    for (const edge of module.imports.filter(edge => edge.star)) {
      const nodes = item.body.filter(node => ts.isExportDeclaration(node) && !node.exportClause
        && ts.isStringLiteral(node.moduleSpecifier) && node.moduleSpecifier.text === edge.specifier);
      assert.equal(nodes.length, 1); bind(edge, null, nodes[0]);
    }
  } else if (!blue && String(item.module).toLowerCase() === 'amd') {
    for (const edge of module.imports) {
      const matches = item.parameterBindings.filter(binding => binding.original === edge);
      assert.equal(matches.length, 1, 'one exact original AMD dependency slot');
      const binding = matches[0]; assert.equal(binding.dependency, edge.specifier);
      const parameter = item.factory.parameters[item.parameterBindings.indexOf(binding) + 2];
      assert(ts.isIdentifier(parameter.name) && parameter.name.text === binding.binding);
      bind(edge, binding.binding, parameter);
    }
  } else if (item.format === 'system') {
    assert.equal(item.setterNodes.length, item.dependencies.length);
    for (const [index, setter] of item.setterNodes.entries()) {
      const edges = module.imports.filter(edge => edge.specifier === item.dependencies[index] && (blue || !edge.star));
      if (!edges.length) continue;
      const parameter = setter.parameters[0].name.text;
      const assignments = setter.body.statements.filter(statement => ts.isExpressionStatement(statement)
        && ts.isBinaryExpression(statement.expression) && statement.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken
        && ts.isIdentifier(statement.expression.left));
      assert.equal(assignments.length, edges.length, 'complete original System import namespace assignments');
      for (let offset = 0; offset < edges.length; offset++) {
        const statement = assignments[offset], assignment = statement.expression;
        assert(ts.isIdentifier(assignment.right) && assignment.right.text === parameter);
        bind(edges[offset], assignment.left.text, statement);
      }
    }
    if (!blue) for (const edge of module.imports.filter(edge => edge.star)) bind(edge, null, null);
  } else {
    const emitted = [];
    for (const statement of item.body) {
      if (!ts.isVariableStatement(statement) && !ts.isExpressionStatement(statement)) continue;
      const calls = [];
      function requires(node) {
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'require'
          && node.arguments.length === 1 && ts.isStringLiteral(node.arguments[0])
          && module.imports.some(edge => edge.specifier === node.arguments[0].text)) calls.push(node);
        ts.forEachChild(node, requires);
      }
      requires(statement);
      for (const call of calls) emitted.push({statement, call});
    }
    assert.equal(emitted.length, module.imports.length, 'complete original import/re-export acquisitions');
    for (let index = 0; index < module.imports.length; index++) {
      const edge = module.imports[index], {statement, call} = emitted[index];
      assert.equal(call.arguments[0].text, edge.specifier, 'exact original dependency acquisition order');
      let namespace = null;
      if (ts.isVariableStatement(statement)) {
        assert.equal(statement.declarationList.declarations.length, 1);
        const declaration = statement.declarationList.declarations[0];
        assert(ts.isIdentifier(declaration.name)); namespace = declaration.name.text;
      } else assert(edge.star, 'only a source star export can acquire without a local namespace');
      bind(edge, namespace, statement);
    }
  }
  assert.equal(acquisitions.length, module.imports.length, 'every original source edge is represented');
  const model = Object.freeze({module, item, symbols: Object.freeze(symbols), acquisitions: Object.freeze(acquisitions),
    helperAlgorithmsValidated: false});
  const capability = Object.freeze({}); registry.set(capability, {original, view, model}); return capability;
}
function checked(capability, view) {
  const record = registry.get(capability);
  assert(record && record.view === view, 'owned import binding proof for this exact emitted view');
  source.checked(record.original); assert.equal(view.source.text, view.code);
  return record.model;
}
module.exports = {read, checked};
