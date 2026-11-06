// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// System hoists, setters and star forwarding are bound to the original graph.
const assert = require('node:assert/strict');

function read(ts, model, blue, consumed) {
  const {item, module} = model, view = item.view, system = item.system;
  assert.equal(item.format, 'system');
  const exported = module.exports.map(edge => edge.exported), sourceNames = new Set();
  function visit(node) { if (ts.isIdentifier(node)) sourceNames.add(node.text); ts.forEachChild(node, visit); }
  visit(module.source);
  function generated(node, label) { view.noSource(node.getStart(view.source), node.end, label); consumed.add(node); }
  const namespaces = model.acquisitions.map(edge => edge.namespace).filter(Boolean);
  const variables = blue ? [] : module.declarations.filter(declaration => declaration.kind === 'variable').map(declaration => declaration.name);
  const expected = [...new Set([...namespaces, ...variables])].sort(), hoists = item.sourceNodes.filter(node =>
    !consumed.has(node) && ts.isVariableStatement(node) && node.declarationList.declarations.every(declaration => !declaration.initializer));
  assert.equal(hoists.length, Number(expected.length > 0), 'complete System hoisted binding group');
  if (hoists.length) {
    const node = hoists[0]; assert.equal(node.declarationList.flags & (ts.NodeFlags.Let | ts.NodeFlags.Const), 0);
    assert.deepEqual(node.declarationList.declarations.map(declaration => {
      assert(ts.isIdentifier(declaration.name)); return declaration.name.text;
    }).sort(), expected, 'complete source/transport System hoisted bindings');
    generated(node, 'complete generated System hoists');
  }
  const star = module.imports.filter(edge => edge.star); assert(star.length <= 1);
  let starName = null;
  if (!blue) {
    const names = item.sourceNodes.filter(node => !consumed.has(node) && ts.isVariableStatement(node)
      && node.declarationList.declarations.length === 1 && node.declarationList.declarations[0].name.getText(view.source) === '__moduleName');
    assert.equal(names.length, 1);
    item.exact(names[0], `var __moduleName=${system.context}&&${system.context}.id;`); generated(names[0], 'generated System module identity');
    const records = item.sourceNodes.filter(node => !consumed.has(node) && ts.isVariableStatement(node)
      && node.declarationList.declarations.length === 1 && /^exportedNames_\d+$/.test(node.declarationList.declarations[0].name.getText(view.source)));
    const functions = item.sourceNodes.filter(node => !consumed.has(node) && ts.isFunctionDeclaration(node) && /^exportStar_\d+$/.test(node.name?.text));
    assert.equal(records.length, star.length); assert.equal(functions.length, star.length);
    if (star.length) {
      const record = records[0], name = record.declarationList.declarations[0].name.text, fn = functions[0];
      starName = fn.name.text;
      assert(!sourceNames.has(name) && !sourceNames.has(starName), 'fresh complete System star helper bindings');
      item.exact(record, `var ${name}={${exported.map(name => JSON.stringify(name) + ':true').join(',')}};`);
      item.exact(fn, `function ${starName}(m){var exports={};for(var n in m){if(n!=='default'&&!${name}.hasOwnProperty(n))exports[n]=m[n];}${system.publisher}(exports);}`);
      generated(record, 'complete generated System star exclusion inventory'); generated(fn, 'complete generated System star helper algorithm');
    }
  }
  for (const [index, setter] of item.setterNodes.entries()) {
    const dependency = item.dependencies[index], parameter = setter.parameters[0].name.text;
    const edges = model.acquisitions.filter(acquisition => acquisition.edge.specifier === dependency && acquisition.node);
    const assignments = edges.map(edge => `${edge.namespace}=${parameter};`).join('');
    const body = blue ? `${system.stem}_imports[${JSON.stringify(dependency)}]=${parameter};${assignments}${system.stem}_refresh();`
      : assignments + (star.some(edge => edge.specifier === dependency) ? `${starName}(${parameter});` : '');
    item.exact(setter, `function(${parameter}){${body}}`);
    generated(setter, 'complete generated original System dependency setter');
  }
  const slots = [], refreshes = [];
  for (const node of item.sourceNodes) {
    if (consumed.has(node) || !ts.isExpressionStatement(node) || !ts.isCallExpression(node.expression)) continue;
    const call = node.expression, name = call.expression.getText(view.source);
    if (blue && name === system.stem + '_slot') {
      assert.equal(call.arguments.length, 1); assert(ts.isStringLiteral(call.arguments[0]));
      slots.push(call.arguments[0].text); item.exact(node, `${system.stem}_slot(${JSON.stringify(call.arguments[0].text)});`);
      generated(node, 'complete generated System export slot');
    } else if (blue && name === system.stem + '_refresh') {
      item.exact(node, `${system.stem}_refresh();`); refreshes.push(node); generated(node, 'complete generated System final publication refresh');
    }
  }
  if (blue) {
    assert.deepEqual(slots.slice().sort(), exported.slice().sort(), 'one complete System slot per original export');
    assert.equal(new Set(slots).size, slots.length);
    assert.equal(refreshes.length, 1); assert.equal(item.body.at(-1), refreshes[0], 'final System execute refresh position');
  }
  return {starName, consumed};
}

module.exports = {read};
