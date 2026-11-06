// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// AMD retains namespace source parameters and repeated original dependency slots.
const assert = require('node:assert/strict');
const original = require('./ge5_interop_source.cjs');
function read(ts, view, options, capability, filename, blue) {
  const model = original.checked(capability), module = model.modules[filename];
  assert(module, 'module belongs to the independently owned original import graph');
  assert.equal(view.source.parseDiagnostics.length, 0, 'complete emitted module parses');
  const format = String(options.module).toLowerCase();
  const importsHelpers = options.importHelpers && module.imports.some(edge => edge.star
    || options.esModuleInterop && edge.bindings.some(binding => ['default', '*'].includes(binding.imported)));
  const helperDependencies = importsHelpers && ['amd', 'umd'].includes(format) ? ['tslib'] : [];
  const dependencies = [...helperDependencies, ...original.dependencies(capability, filename, options.module, blue)];
  if (blue || format !== 'amd') {
    return require('./ge5_module_envelope.cjs').read(ts, view, options,
      {blue, originals: [module.source], dependencies});
  }
  const syntax = require('./ge5_module_syntax.cjs').create(ts, view);
  const candidates = view.source.statements.filter(node => ts.isExpressionStatement(node)
    && ts.isCallExpression(node.expression) && node.expression.expression.getText(view.source) === 'define');
  assert.equal(candidates.length, 1, 'one complete AMD registration');
  const registration = candidates[0], call = registration.expression;
  assert.equal(call.arguments.length, 2);
  const array = call.arguments[0];
  assert(ts.isArrayLiteralExpression(array));
  assert.deepEqual(array.elements.map(element => {
    assert(ts.isStringLiteral(element)); return element.text;
  }), ['require', 'exports', ...dependencies], 'exact original AMD dependency slots');
  const factory = call.arguments[1];
  assert(ts.isFunctionExpression(factory));
  assert.equal(factory.parameters.length, dependencies.length + 2);
  const names = factory.parameters.map(parameter => {
    assert(ts.isIdentifier(parameter.name)); return parameter.name.text;
  });
  assert.deepEqual(names.slice(0, 2), ['require', 'exports']);
  assert.equal(new Set(names).size, names.length, 'distinct complete AMD factory bindings');
  syntax.plainFunction(factory, names);
  const identifiers = new Set();
  function visit(node) { if (ts.isIdentifier(node)) identifiers.add(node.text); ts.forEachChild(node, visit); }
  visit(module.source);
  const edges = [...helperDependencies.map(() => null), ...module.imports];
  const parameterBindings = dependencies.map((dependency, index) => {
    const binding = names[index + 2], edge = edges[index];
    const namespace = edge?.bindings.find(binding => binding.imported === '*');
    if (namespace) assert.equal(binding, namespace.local, 'exact original namespace import parameter');
    else assert(!identifiers.has(binding), 'fresh AMD dependency transport binding');
    return {dependency, binding, original: edge};
  });
  const body = [...factory.body.statements], prelude = view.source.statements.filter(node => node !== registration);
  const holes = [[factory.body.getStart(view.source) + 1, factory.body.end - 1]];
  const wrapperTokens = view.tokens(registration).filter(token =>
    !holes.some(([start, end]) => start <= token.start && token.end <= end));
  function classifyTransport() {
    for (const token of wrapperTokens) view.noSource(token.start, token.end, 'generated AMD module syntax');
    for (let index = 1; index < wrapperTokens.length; index++) {
      const before = wrapperTokens[index - 1], after = wrapperTokens[index];
      if (holes.some(([start, end]) => before.end <= start && end <= after.start)) continue;
      view.noSource(before.end, after.start, 'generated AMD module gap');
    }
  }
  return {view, format: 'wrapped', module: options.module, body, prelude,
    sourceNodes: [...prelude, ...body], factory, execute: null, system: null,
    setterNodes: [], parameterBindings, dependencies, generated: [], wrapperTokens, holes,
    registration, classifyTransport, ...syntax};
}
module.exports = {read};
