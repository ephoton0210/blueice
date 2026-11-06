// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original method completion ends before generated dynamic receiver transport.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..'), graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream, compareTokens} = require('./ge5_role_stream.cjs');
const fixture = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const input = JSON.parse(fs.readFileSync(path.join(fixture, 'cases.json'))), reference = JSON.parse(fs.readFileSync(path.join(fixture, 'reference.json'))), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const observations = JSON.parse(fs.readFileSync(0, 'utf8')), compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc')));
assert.deepEqual(observations.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge1_downlevel/async-class-method'
  && ['ES2015', 'ES2016'].includes(graph.resolve(input, row, parents).options.target) ? [index] : []));
assert.equal(observations.length, 16);
const start = (view, node) => node.getStart(view.source);
function unwrap(node) {while (ts.isParenthesizedExpression(node)) node = node.expression; return node;}
function read(view, blue) {
  const box = view.source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'Box'); assert(box);
  const method = box.members.find(node => ts.isMethodDeclaration(node) && node.name.getText(view.source) === 'read'); assert(method && method.body.statements.length === 1);
  const returned = method.body.statements[0]; assert(ts.isReturnStatement(returned));
  let driver = returned.expression, entry = method, apply = null;
  if (blue) {
    apply = driver;
    assert(ts.isCallExpression(apply) && ts.isPropertyAccessExpression(apply.expression) && apply.expression.name.text === 'apply');
    assert.equal(apply.arguments.length, 2); assert.equal(apply.arguments[0].kind, ts.SyntaxKind.ThisKeyword); assert.equal(apply.arguments[1].getText(view.source), 'arguments');
    entry = unwrap(apply.expression.expression); assert(ts.isFunctionExpression(entry));
    const body = entry.body.statements; assert.equal(body.length, 2); assert(ts.isIfStatement(body[0]) && ts.isReturnStatement(body[1])); driver = body[1].expression;
  }
  assert(ts.isCallExpression(driver) && ts.isIdentifier(driver.expression));
  assert.equal(driver.arguments.length, blue ? 3 : 4); assert.equal(driver.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
  const callback = driver.arguments.at(-1); assert(ts.isFunctionExpression(callback) && callback.asteriskToken);
  return {box, method, returned: blue ? entry.body.statements[1] : returned, driver, callback, entry, apply};
}
const verified = [], failures = [];
for (const row of observations) {
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  const expected = reference.cases[row.index];
  for (const [name, entry] of Object.entries(expected.maps)) if (entry.kind === 'javascript') {
    try {
      const native = stream(ts, reference.blobs[expected.outputs[entry.output]], authority.raw(reference, expected, name));
      const actual = stream(ts, row.outputs[entry.output], row.outputs[name]);
      assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
      const n = read(native, false), a = read(actual, true), endpoint = native.owner(n.callback.end);
      assert(endpoint, 'Native class method has a mapped driver completion');
      for (const [view, item] of [[native, n], [actual, a]]) {
        for (let offset = item.callback.end; offset < item.returned.end; offset++) assert.deepEqual(view.owner(offset), endpoint, 'complete class method driver ending');
      }
      actual.noSource(a.entry.end, a.method.end, 'generated class method apply transport');
      const calls = (view, item) => {let found; function visit(node) {if (ts.isCallExpression(node) && node.expression.getText(view.source) === 'Promise.resolve') found = node; ts.forEachChild(node, visit);} visit(item.callback); assert(found); return view.tokens(found);};
      compareTokens(native, calls(native, n), actual, calls(actual, a), 'complete original class-method await operand');
      verified.push({id: row.id, actualJavaScriptSha256: graph.sha256(actual.code), actualMapSha256: graph.sha256(actual.raw)});
    } catch (error) {failures.push({id: row.id, error: error.message, stack: error.stack});}
  }
}
const report = {scope: 'class-method driver ending and generated apply transport; complete family roles remain required', compilerSha256,
  configurations: observations.length, repeatBuilds: 2, completeFamily: false, verified, failures};
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'async-class-method-boundaries.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify(report, null, 2)); if (failures.length) process.exitCode = 1;
