// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), test = require('node:test'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const graph = require('./ge5_inputs.cjs'), source = require('./ge5_interop_source.cjs');
const envelope = require('./ge5_interop_envelope.cjs'), imports = require('./ge5_interop_imports.cjs');
const references = require('./ge5_interop_references.cjs');
const {stream} = require('./ge5_role_stream.cjs'), authority = require('./ge5_native_authority.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const parents = graph.parents(root, input);

function fixture(index, filename = 'main.ts', change = code => code) {
  const original = graph.resolve(input, input.cases[index], parents), expected = reference.cases[index];
  const mapName = 'out/' + filename.replace(/\.ts$/, '.js.map'), map = expected.maps[mapName];
  assert(map, mapName);
  const view = stream(ts, change(reference.blobs[expected.outputs[map.output]]), authority.raw(reference, expected, mapName));
  const cap = source.read(ts, original.files, original.entry), module = source.checked(cap).modules[filename];
  const item = envelope.read(ts, view, original.options, cap, filename, false);
  return {view, module, item, binding: imports.read(ts, item, cap, filename, false)};
}

function initializer(f, name) {
  for (const node of f.item.body) {
    if (ts.isVariableStatement(node)) {
      const declaration = node.declarationList.declarations.find(declaration =>
        ts.isIdentifier(declaration.name) && declaration.name.text === name);
      if (declaration?.initializer) return declaration.initializer;
    }
    if (!ts.isExpressionStatement(node)) continue;
    const expression = node.expression;
    if (ts.isBinaryExpression(expression) && ts.isPropertyAccessExpression(expression.left)
      && expression.left.expression.getText(f.view.source) === 'exports' && expression.left.name.text === name) {
      if (ts.isVoidExpression(expression.right)) continue;
      return expression.right;
    }
    if (f.item.system && ts.isCallExpression(expression) && expression.expression.getText(f.view.source) === f.item.system.publisher
      && ts.isStringLiteral(expression.arguments[0]) && expression.arguments[0].text === name
      && ts.isBinaryExpression(expression.arguments[1])) return expression.arguments[1].right;
  }
  assert.fail('missing original variable initializer: ' + name);
}

function compareModule(f) {
  const pairs = [];
  for (const declaration of f.module.declarations) {
    if (declaration.kind === 'variable') {
      pairs.push(...references.compare(ts, f.binding, f.view, declaration.node.initializer, initializer(f, declaration.name)));
      continue;
    }
    const emitted = f.item.sourceNodes.filter(node => ts.isFunctionDeclaration(node) && node.name?.text === declaration.name);
    assert.equal(emitted.length, 1);
    const original = declaration.node.body.statements, actual = emitted[0].body.statements;
    assert.equal(actual.length, original.length);
    for (let index = 0; index < original.length; index++) {
      assert.equal(actual[index].kind, original[index].kind);
      pairs.push(...references.compare(ts, f.binding, f.view, original[index].expression, actual[index].expression));
    }
  }
  return pairs;
}

test('all four source modules retain expression bindings through seven Native module ABIs', () => {
  for (const index of [4884, 4924, 4964, 5004, 5044, 5084, 5164]) {
    for (const filename of ['main.ts', 'dep.ts', 'left.ts', 'right.ts']) {
      assert(compareModule(fixture(index, filename)).length > 0);
    }
  }
});

test('a changed import namespace or member cannot retain its source binding', () => {
  for (const [before, after] of [['dep_js_1.default', 'dep_js_9.default'], ['left_js_1.read', 'left_js_1.reed']]) {
    const f = fixture(4884, 'main.ts', code => { assert(code.includes(before)); return code.replace(before, after); });
    assert.throws(() => compareModule(f), /independently bound imported reference/);
  }
});

test('the imported-call comma transport must preserve the unbound call', () => {
  const f = fixture(4884, 'main.ts', code => {
    assert(code.includes('(0, dep_js_1.default)')); return code.replace('(0, dep_js_1.default)', '(1, dep_js_1.default)');
  });
  assert.throws(() => compareModule(f), /exact unbound imported-call transport/);
});

test('a function parameter shadows the equally named exported module variable', () => {
  const f = fixture(4884, 'dep.ts'), pairs = compareModule(f);
  assert(pairs.some(pair => pair.role === 'parameter' && pair.original.text === 'value'));
  const changed = fixture(4884, 'dep.ts', code => {
    assert(code.includes('return value + 1')); return code.replace('return value + 1', 'return other + 1');
  });
  assert.throws(() => compareModule(changed), /lexically shadowed parameter reference/);
});

test('System mutations publish the exact original live binding', () => {
  const f = fixture(5044, 'left.ts', code => {
    const before = 'exports_1("leftValue", leftValue = value)';
    assert(code.includes(before)); return code.replace(before, 'exports_1("xxxxValue", leftValue = value)');
  });
  assert.throws(() => compareModule(f), /exact System live-binding publication name/);
});

test('copied AST nodes and copied import capabilities cannot approve references', () => {
  const f = fixture(4884), original = f.module.declarations[0].node.initializer, actual = initializer(f, 'result');
  assert.throws(() => references.compare(ts, {...f.binding}, f.view, original, actual), /owned import binding proof/);
  assert.throws(() => references.compare(ts, f.binding, f.view, {...original}, actual), /exact original and emitted AST identities/);
  assert.throws(() => references.compare(ts, f.binding, f.view, original, {...actual}), /exact original and emitted AST identities/);
});
