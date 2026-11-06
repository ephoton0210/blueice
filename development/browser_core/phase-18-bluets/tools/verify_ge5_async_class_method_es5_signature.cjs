// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..'), graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream, compareTokens} = require('./ge5_role_stream.cjs');
const fixture = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const input = JSON.parse(fs.readFileSync(path.join(fixture, 'cases.json'))), reference = JSON.parse(fs.readFileSync(path.join(fixture, 'reference.json'))), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const observations = JSON.parse(fs.readFileSync(0, 'utf8')), compilerSha256 = graph.sha256(fs.readFileSync(process.env.BLUEICE_GE5_CLI_BINARY || path.join(root, 'target/debug/bluetsc')));
assert.deepEqual(observations.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge1_downlevel/async-class-method'
  && graph.resolve(input, row, parents).options.target === 'ES5' ? [index] : []));
const verified = [], failures = [];
for (const row of observations) {
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  const expected = reference.cases[row.index], original = graph.resolve(input, input.cases[row.index], parents);
  const source = ts.createSourceFile('original.ts', original.files['main.ts'], ts.ScriptTarget.Latest, true);
  for (const [name, entry] of Object.entries(expected.maps)) if (entry.kind === 'javascript') {
    try {
      const code = row.outputs[entry.output], raw = name.endsWith('#inline') ? Buffer.from(code.split('base64,')[1].trim(), 'base64').toString() : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[entry.output]], authority.raw(reference, expected, name)), actual = stream(ts, code, raw);
      const inspect = (view, blue) => {
        const env = require('./ge5_module_envelope.cjs').read(ts, view, row.options, {blue, originals: [source], dependencies: []});
        return require('./ge5_async_class_method_ir.cjs').read(ts, view, row.options, source, blue, env);
      };
      const n = inspect(native, false), a = inspect(actual, true);
      const header = (view, item) => view.tokens(item.method).filter(token => token.text === 'function' || token.text === '(').slice(0, 2);
      const nh = header(native, n), ah = header(actual, a); assert.equal(nh.length, 2); assert.equal(ah.length, 2);
      for (let index = 0; index < 2; index++) {
        assert.equal(ah[index].text, nh[index].text);
        for (let unit = 0; unit < nh[index].end - nh[index].start; unit++) assert.deepEqual(actual.owner(ah[index].start + unit), native.owner(nh[index].start + unit), 'original async signature UTF-16 unit');
      }
      actual.noSource(a.method.parameters[0].getStart(actual.source), a.method.parameters[0].end, 'generated external arity formal');
      const completeHeader = (view, node) => view.tokens(node).filter(token => token.end <= node.body.getStart(view.source) + 1).slice(1);
      compareTokens(native, completeHeader(native, n.method), actual, completeHeader(actual, a.callback), 'complete relocated class-method formal header');
      actual.noSource(a.entry.end, a.method.end, 'generated ES5 method apply and outer closing');
      verified.push({id: row.id});
    } catch (error) {failures.push({id: row.id, error: error.message, stack: error.stack});}
  }
}
console.log(JSON.stringify({scope: 'ES5 async class method signature component', completeFamily: false, compilerSha256, verified, failures}, null, 2));
if (failures.length) process.exitCode = 1;
