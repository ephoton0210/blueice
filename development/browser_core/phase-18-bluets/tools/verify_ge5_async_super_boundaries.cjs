// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../../../..'), graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream} = require('./ge5_role_stream.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')), reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const observations = JSON.parse(fs.readFileSync(0, 'utf8')), compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc')));
const targets = observations.every(row => row.options.target === 'ES5') ? ['ES5'] : ['ES2015', 'ES2016'];
assert.deepEqual(observations.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge1_downlevel/async-super-method' && targets.includes(graph.resolve(input, row, parents).options.target) ? [index] : []));
const verified = [], failures = [];
for (const row of observations) {
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  const expected = reference.cases[row.index], original = graph.resolve(input, input.cases[row.index], parents), source = ts.createSourceFile('original.ts', original.files['main.ts'], ts.ScriptTarget.Latest, true);
  for (const [name, map] of Object.entries(expected.maps)) if (map.kind === 'javascript') {
    try {
      const code = row.outputs[map.output], raw = name.endsWith('#inline') ? Buffer.from(code.split('base64,')[1].trim(), 'base64').toString() : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name)), actual = stream(ts, code, raw);
      const read = (view, blue) => {
        const env = require('./ge5_module_envelope.cjs').read(ts, view, row.options, {blue, originals: [source], dependencies: []});
        return require('./ge5_async_super_method_ir.cjs').read(ts, view, row.options, source, blue, env);
      };
      const n = read(native, false), a = read(actual, true);
      require('./ge5_async_super_member_roles.cjs').prove(ts, native, n, actual, a, row.options);
      if (row.options.target === 'ES5') {
        const header = (view, node) => view.tokens(node).filter(token => token.end <= node.body.getStart(view.source) + 1);
        require('./ge5_role_stream.cjs').compareTokens(native, header(native, n.method), actual, header(actual, a.method), 'complete empty async method signature');
        require('./ge5_role_stream.cjs').compareTokens(native, header(native, n.method).slice(1), actual, header(actual, a.callback).slice(1), 'complete relocated empty callback signature');
      }
      verified.push({id: row.id});
    } catch (error) {failures.push({id: row.id, error: error.message, stack: error.stack});}
  }
}
console.log(JSON.stringify({scope: 'async-super member and call component', completeFamily: false, compilerSha256, configurations: observations.length, verified, failures}, null, 2));
if (failures.length) process.exitCode = 1;
