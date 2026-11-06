// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Whole module dependency and lexical identities precede generator capabilities.
const assert = require('node:assert/strict'), registry = new WeakMap();
function create(ts, item, options, origin) {
  const view = item.view, blue = item.blue, lowered = options.target === 'ES5';
  const external = lowered && Boolean(options.importHelpers || options.noEmitHelpers), imported = lowered && Boolean(options.importHelpers);
  const members = lowered ? ['__generator', '__values'] : [], bindings = new Map();
  const generated = [], helpers = [], remaining = [];
  let namespace = item.parameterBindings[0]?.binding || null, importCount = 0;
  const fresh = name => {assert(!origin.identifiers.has(name), 'helper identity cannot alias the original AST'); return name;};
  function generatedNode(node, label) {view.noSource(node.getStart(view.source), node.end, label); generated.push(node);}
  if (imported && item.format === 'system') {
    assert.equal(item.setterNodes.length, 1);
    const setter = item.setterNodes[0], parameter = setter.parameters[0].name.text;
    const statement = setter.body.statements[blue ? 1 : 0];
    assert(ts.isExpressionStatement(statement) && ts.isBinaryExpression(statement.expression) && ts.isIdentifier(statement.expression.left));
    namespace = fresh(statement.expression.left.text);
    item.exact(setter, blue
      ? `function(${parameter}){${item.system.stem}_imports['tslib']=${parameter};${namespace}=${parameter};${item.system.stem}_refresh();}`
      : `function(${parameter}){${namespace}=${parameter};}`);
    generatedNode(setter, 'complete synchronous generator helper setter');
  }
  for (const node of item.sourceNodes) {
    if (ts.isImportDeclaration(node)) {
      assert(imported && ts.isStringLiteral(node.moduleSpecifier) && node.moduleSpecifier.text === 'tslib');
      assert.equal(++importCount, 1);
      if (blue) {
        assert(ts.isNamespaceImport(node.importClause.namedBindings)); namespace = fresh(node.importClause.namedBindings.name.text);
        item.exact(node, `import * as ${namespace} from 'tslib';`);
      } else {
        assert(ts.isNamedImports(node.importClause.namedBindings));
        const elements = node.importClause.namedBindings.elements;
        assert.deepEqual(elements.map(e => e.propertyName?.text || e.name.text), members);
        for (const element of elements) bindings.set(element.propertyName?.text || element.name.text, fresh(element.name.text));
      }
      generatedNode(node, 'complete synchronous generator helper import'); continue;
    }
    if (ts.isVariableStatement(node) && node.declarationList.declarations.length === 1) {
      const declaration = node.declarationList.declarations[0], init = declaration.initializer;
      if (init && ts.isCallExpression(init) && init.expression.getText(view.source) === 'require') {
        assert(imported && ts.isIdentifier(declaration.name) && init.arguments.length === 1
          && ts.isStringLiteral(init.arguments[0]) && init.arguments[0].text === 'tslib');
        assert.equal(++importCount, 1); namespace = fresh(declaration.name.text);
        item.exact(node, `var ${namespace}=require('tslib');`);
        generatedNode(node, 'complete synchronous generator helper require'); continue;
      }
    }
    const name = ts.isFunctionDeclaration(node) ? node.name?.text
      : ts.isVariableStatement(node) && node.declarationList.declarations.length === 1
        ? node.declarationList.declarations[0].name.getText(view.source) : null;
    if (name && (members.includes(name) || /^__blueice_target_/.test(name))) {
      assert(lowered && !external); fresh(name); helpers.push(node);
    } else remaining.push(node);
  }
  assert.equal(importCount, imported && item.format !== 'system' && (blue || !item.parameterBindings.length) ? 1 : 0);
  if (namespace) fresh(namespace);
  if (external && !imported) for (const member of members) bindings.set(member, member);
  const functions = remaining.filter(n => ts.isFunctionDeclaration(n) && n.name?.text === origin.fn.name.text);
  assert.equal(functions.length, 1, 'one entire original generator'); const fn = functions[0];
  let privateGenerator = null;
  if (lowered && !external) {
    if (blue) {
      assert.equal(helpers.length, 3, 'complete generator/delegate protocol dependency group');
      privateGenerator = helpers.find(n => ts.isFunctionDeclaration(n) && /^__blueice_target_generator_\d+$/.test(n.name?.text));
      assert(privateGenerator);
      require('./ge5_binding_helpers.cjs').validate(ts, view, helpers, origin.identifiers);
    } else {
      const text = ts.transpileModule(origin.file.text, {compilerOptions: {target: ts.ScriptTarget.ES5, module: ts.ModuleKind.CommonJS}}).outputText;
      const source = ts.createSourceFile('native-helper-contract.js', text, ts.ScriptTarget.Latest, true);
      const specs = new Map(source.statements.filter(ts.isVariableStatement).map(n => [n.declarationList.declarations[0].name.getText(source), n.getText(source)]));
      assert.equal(helpers.length, 2);
      for (const helper of helpers) {
        const name = helper.declarationList.declarations[0].name.text; assert(members.includes(name)); item.exact(helper, specs.get(name));
      }
    }
  } else assert.equal(helpers.length, 0);
  for (const helper of helpers) generatedNode(helper, 'complete private synchronous generator protocol');
  const references = new Map(members.map(member => [member, Object.freeze({member, abi: blue ? 'tslib-adapter' : 'native',
    ...namespace ? {namespace} : {binding: bindings.get(member) || member}})]));
  function isReference(node, member) {
    if (!external && blue) return member === '__generator' && ts.isIdentifier(node) && node.text === privateGenerator.name.text;
    const reference = references.get(member); assert(reference);
    return reference.namespace ? ts.isPropertyAccessExpression(node) && !node.questionDotToken
      && ts.isIdentifier(node.expression) && node.expression.text === reference.namespace && node.name.text === member
      : ts.isIdentifier(node) && node.text === reference.binding;
  }
  if (external) {
    function shadow(node) {
      if ((ts.isParameter(node) || ts.isVariableDeclaration(node)) && ts.isIdentifier(node.name)) {
        assert(![...references.values()].some(r => (r.namespace || r.binding) === node.name.text), 'no nested helper shadow');
      }
      ts.forEachChild(node, shadow);
    }
    shadow(fn);
  }
  const proof = Object.freeze({view, item, fn, lowered, external, imported, members, namespace, generated, helpers, remaining,
    reference: member => {assert(references.has(member)); return references.get(member);}, isReference});
  const capability = {}; registry.set(capability, proof); return capability;
}
function checked(capability, view) {const proof = registry.get(capability); assert(proof && proof.view === view); return proof;}
module.exports = {create, checked};
