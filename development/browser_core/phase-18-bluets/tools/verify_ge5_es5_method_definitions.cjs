// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');
const assert = require('node:assert/strict'), root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const {stream} = require('./ge5_role_stream.cjs'), classRoles = require('./ge5_class_roles.cjs');
const rows = JSON.parse(fs.readFileSync(0, 'utf8'));

function execute(code) {
  const exports = {}, context = vm.createContext({exports});
  vm.runInContext(`var descriptorLayouts = [], define = Object.defineProperty;
    Object.defineProperty = function(object, key, descriptor) {
      if (typeof descriptor.value === 'function') descriptorLayouts.push([String(key), Object.keys(descriptor)]);
      return define(object, key, descriptor);
    };`, context);
  vm.runInContext(code, context);
  const C = exports.C;
  const methods = [['plain', C.prototype], ['a-b', C.prototype], ['16', C.prototype], ['staticMethod', C]];
  return {
    descriptorLayouts: JSON.parse(JSON.stringify(context.descriptorLayouts)),
    instanceKeys: Object.keys(C.prototype), staticKeys: Object.keys(C),
    methods: methods.map(([key, object]) => {
      const d = Object.getOwnPropertyDescriptor(object, key); assert.equal(typeof d.value, 'function');
      return {key, enumerable: d.enumerable, writable: d.writable, configurable: d.configurable,
        name: d.value.name, value: d.value.call(object)};
    })
  };
}

function mappings(row, native) {
  const source = ts.createSourceFile('original.ts', row.source, 99, true).statements.find(ts.isClassDeclaration);
  function read(view) {
    const nodes = view.source.statements.filter(node => ts.isVariableStatement(node)
      && node.declarationList.declarations.length === 1 && node.declarationList.declarations[0].name.text === 'C'
      && node.declarationList.declarations[0].initializer);
    assert.equal(nodes.length, 1); return classRoles.readClass(ts, view, nodes[0], source);
  }
  const n = stream(ts, native.outputText, native.sourceMapText), a = stream(ts, row.code, row.raw);
  const nn = read(n), aa = read(a); assert.equal(aa.methods.length, nn.methods.length);
  for (let index = 0; index < nn.methods.length; index++) {
    const x = nn.methods[index], y = aa.methods[index]; assert.equal(y.key.text, x.key.text);
    assert.equal(y.descriptor, x.descriptor, 'Native assignment/descriptor mode');
    if (x.descriptor) {
      const nd = x.statement.expression.arguments[2], ad = y.statement.expression.arguments[2];
      for (let flag = 0; flag < 3; flag++) {
        const nt = n.tokens(nd.properties[flag]), at = a.tokens(ad.properties[flag]);
        assert.deepEqual(at.map(token => token.text), nt.map(token => token.text));
        assert.deepEqual(at.map(token => token.owners), nt.map(token => token.owners),
          'complete generated descriptor flag source roles');
      }
    }
    for (const [label, left, right] of [['key', x.key, y.key], ['definition', x.statement, y.statement],
      ['function', x.fn, y.fn], ['body', x.fn.body, y.fn.body]]) {
      assert.deepEqual(a.owner(right.end), n.owner(left.end), 'Native method logical endpoint ' + x.key.text + '/' + label);
    }
  }
}

for (const row of rows) {
  const native = ts.transpileModule(row.source, {fileName: 'main.ts', compilerOptions: {
    target: ts.ScriptTarget.ES5, module: ts.ModuleKind.CommonJS,
    useDefineForClassFields: row.define, sourceMap: row.maps
  }});
  assert.deepEqual(execute(row.code), execute(native.outputText),
    'complete Native descriptor/name/runtime profile ' + JSON.stringify({define: row.define, maps: row.maps}));
  if (row.maps) mappings(row, native);
}
for (let index = 0; index < rows.length; index += 2) {
  const strip = code => code.split('\n').filter(line => !line.startsWith('//# sourceMappingURL=')).join('\n');
  assert.equal(strip(rows[index].code), strip(rows[index + 1].code), 'maps preserve method ABI bytes');
}
console.log(JSON.stringify({configurations: rows.length, executions: rows.length * 2, passed: true}));
