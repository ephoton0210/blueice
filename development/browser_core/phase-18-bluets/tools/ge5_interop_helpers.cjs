// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each complete helper algorithm or external provider precedes reference use.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const source = require('./ge5_interop_source.cjs'), registry = new WeakMap();
function read(ts, item, original, filename, options, blue) {
  const module = source.checked(original).modules[filename], view = item.view;
  assert(module && view.source.text === view.code);
  const names = new Set();
  function visit(node) { if (ts.isIdentifier(node)) names.add(node.text); ts.forEachChild(node, visit); }
  visit(module.source);
  const preserved = item.body.some(ts.isImportDeclaration), system = item.format === 'system';
  const members = new Set();
  if (!preserved && !system) for (const edge of module.imports) {
    if (edge.star) members.add('__exportStar');
    if (options.esModuleInterop) for (const binding of edge.bindings) {
      if (binding.imported === 'default') members.add('__importDefault');
      if (binding.imported === '*') members.add('__importStar');
    }
  }
  const imported = members.size > 0 && Boolean(options.importHelpers);
  const ambient = members.size > 0 && !imported && Boolean(options.noEmitHelpers);
  const generated = [], helpers = [], declarations = [], seen = new Set();
  let namespace = null;
  function fresh(name) { assert(!names.has(name), 'fresh interop helper/provider binding'); return name; }
  function cover(node, label) { view.noSource(node.getStart(view.source), node.end, label); generated.push(node); }
  if (imported && !blue && String(item.module).toLowerCase() === 'amd') {
    const parameters = item.parameterBindings.filter(binding => binding.dependency === 'tslib');
    assert.equal(parameters.length, 1); namespace = fresh(parameters[0].binding);
  }
  for (const node of item.sourceNodes) {
    if (!ts.isVariableStatement(node) || node.declarationList.declarations.length !== 1) continue;
    const declaration = node.declarationList.declarations[0];
    if (!ts.isIdentifier(declaration.name)) continue;
    const name = declaration.name.text, init = declaration.initializer;
    if (init && ts.isCallExpression(init) && ts.isIdentifier(init.expression) && init.expression.text === 'require'
      && init.arguments.length === 1 && ts.isStringLiteral(init.arguments[0]) && init.arguments[0].text === 'tslib') {
      assert(imported && !namespace, 'one complete imported interop helper namespace');
      namespace = fresh(name);
      item.exact(node, `${blue || options.target === 'ES5' ? 'var' : 'const'} ${name}=require('tslib');`);
      declarations.push(node); cover(node, 'complete generated interop helper acquisition');
    } else if (/^__(?:createBinding|setModuleDefault|importDefault|importStar|exportStar)$/.test(name)) {
      assert(members.size && !imported && !ambient, 'private helper belongs to the selected provider');
      fresh(name); assert(!seen.has(name), 'unique complete interop helper algorithm'); seen.add(name); helpers.push(node);
    }
  }
  assert.equal(Boolean(namespace), imported, 'exact imported helper provider inventory');
  if (!imported && !ambient && members.size) {
    if (blue) {
      const directory = path.resolve(__dirname, '../../../../backend/bluets/src/emitter');
      const rust = fs.readFileSync(path.join(directory, 'commonjs.rs'), 'utf8');
      const literal = /const IMPORT_DEFAULT_HELPER: &str = ("(?:[^"\\]|\\.)*");/.exec(rust);
      assert(literal, 'one complete canonical default import helper');
      const spec = new Map([
        ['__importDefault', JSON.parse(literal[1])],
        ['__importStar', fs.readFileSync(path.join(directory, 'commonjs/import_star.v1.js'), 'utf8')],
      ]);
      assert.deepEqual([...seen].sort(), [...members].filter(member => member !== '__exportStar').sort(),
        'complete private interop helper inventory; star forwarding is separately verified');
      for (const node of helpers) item.exact(node, spec.get(node.declarationList.declarations[0].name.text));
    } else {
      const emitted = ts.transpileModule(module.source.text, {fileName: filename, compilerOptions: {
        target: ts.ScriptTarget[options.target], module: ts.ModuleKind.CommonJS, esModuleInterop: Boolean(options.esModuleInterop),
      }}).outputText;
      const parsed = ts.createSourceFile('native-interop-helpers.js', emitted, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
      const specs = new Map(parsed.statements.filter(ts.isVariableStatement).flatMap(node => {
        if (node.declarationList.declarations.length !== 1) return [];
        const declaration = node.declarationList.declarations[0];
        return ts.isIdentifier(declaration.name) && /^__(?:createBinding|setModuleDefault|importDefault|importStar|exportStar)$/.test(declaration.name.text)
          ? [[declaration.name.text, node.getText(parsed)]] : [];
      }));
      assert.deepEqual([...seen].sort(), [...specs.keys()].sort(), 'complete pinned Native helper dependency group');
      for (const node of helpers) item.exact(node, specs.get(node.declarationList.declarations[0].name.text));
    }
    for (const node of helpers) cover(node, 'complete generated interop helper algorithm');
  } else assert.equal(helpers.length, 0);
  const proof = Object.freeze({item, module, members: Object.freeze([...members]), imported, ambient, namespace,
    helpers: Object.freeze(helpers), declarations: Object.freeze(declarations), generated: Object.freeze(generated)});
  const capability = Object.freeze({}); registry.set(capability, {view, original, proof}); return capability;
}
function checked(capability, view) {
  const record = registry.get(capability);
  assert(record && record.view === view, 'owned complete interop helper proof'); source.checked(record.original);
  return record.proof;
}
function reference(ts, capability, view, node, member) {
  const proof = checked(capability, view); assert(proof.members.includes(member));
  assert.equal(node.getSourceFile(), view.source);
  if (proof.namespace) assert(ts.isPropertyAccessExpression(node) && !node.questionDotToken
    && ts.isIdentifier(node.expression) && node.expression.text === proof.namespace && node.name.text === member,
  'exact independently bound imported helper reference');
  else assert(ts.isIdentifier(node) && node.text === member, 'exact private or ambient helper reference');
  return true;
}
module.exports = {read, checked, reference};
