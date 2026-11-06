// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// This component proves storage/publication; complete class/module roles remain required.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs');
const {stream, compareTokens, compareGap} = require('./ge5_role_stream.cjs');
const root = path.resolve(__dirname, '../../../..'), fixture = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(fixture, 'cases.json')), referenceBytes = fs.readFileSync(path.join(fixture, 'reference.json'));
assert.equal(graph.sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input = JSON.parse(inputBytes), reference = JSON.parse(referenceBytes), parents = graph.parents(root, input);
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const observations = JSON.parse(fs.readFileSync(0, 'utf8'));
const forms = new Set(['class-expression-named', 'class-expression-anonymous', 'class-expression-computed-heritage'].map(name => 'ge3_decorator_compositions/' + name));
assert.deepEqual(observations.map(row => row.index), input.cases.flatMap((row, index) => forms.has(row.form)
  && graph.resolve(input, row, parents).options.module === 'CommonJS' ? [index] : []));
assert.equal(observations.length, 57);
const compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc')));
const start = (view, node) => node.getStart(view.source);
function compare(native, actual, original, options) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  const source = ts.createSourceFile('original.ts', original, ts.ScriptTarget.Latest, true);
  const declarations = source.statements.filter(ts.isVariableStatement).flatMap(node => [...node.declarationList.declarations]);
  const declaration = declarations.find(node => ts.isIdentifier(node.name) && ts.isClassExpression(node.initializer)); assert(declaration);
  const name = declaration.name.text;
  function assignment(view, statement) {
    if (!ts.isExpressionStatement(statement) || !ts.isBinaryExpression(statement.expression)) return null;
    const value = statement.expression;
    return value.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isPropertyAccessExpression(value.left)
      && value.left.expression.getText(view.source) === 'exports' && value.left.name.text === name ? value : null;
  }
  function local(view) {
    const statements = view.source.statements.filter(node => ts.isVariableStatement(node)
      && node.declarationList.declarations.some(node => ts.isIdentifier(node.name) && node.name.text === name));
    assert.equal(statements.length, 1, 'one retained original class-valued const');
    const statement = statements[0]; assert(statement.declarationList.flags & ts.NodeFlags.Const);
    assert.equal(statement.declarationList.declarations.length, 1);
    const variable = statement.declarationList.declarations[0]; assert(ts.isClassExpression(variable.initializer));
    const published = view.source.statements[view.source.statements.indexOf(statement) + 1], value = assignment(view, published);
    assert(value && ts.isIdentifier(value.right) && value.right.text === name, 'immediate publication of the exact local const');
    return {statement, variable, published, value};
  }
  function direct(view) {
    assert(!view.source.statements.some(node => ts.isVariableStatement(node)
      && node.declarationList.declarations.some(node => node.name.getText(view.source) === name)), 'lowered class value uses direct export storage');
    const statements = view.source.statements.filter(node => {
      const value = assignment(view, node); return value && ts.isCallExpression(value.right);
    });
    assert.equal(statements.length, 1, 'one complete direct class-valued export initializer');
    return {statement: statements[0], value: assignment(view, statements[0])};
  }
  const preserved = options.target === 'ESNext' && options.useDefineForClassFields;
  let n, a;
  if (preserved) {
    n = local(native); a = local(actual);
    const prefix = (view, item) => view.tokens(item.statement).filter(token => token.end <= start(view, item.variable.initializer));
    compareTokens(native, prefix(native, n), actual, prefix(actual, a), 'preserved class-valued const prefix');
    compareTokens(native, native.tokens(n.published), actual, actual.tokens(a.published), 'complete mapped immediate class publication');
    compareGap(native, n.statement.end, start(native, n.published), actual, a.statement.end, start(actual, a.published), 'class declaration/publication boundary');
    compareTokens(native, [native.tokens(n.statement).at(-1)], actual, [actual.tokens(a.statement).at(-1)], 'class-valued declaration terminator');
    assert.deepEqual(actual.owner(a.statement.end), native.owner(n.statement.end), 'class-valued declaration endpoint');
    assert.deepEqual(actual.owner(a.published.end), native.owner(n.published.end), 'mapped class publication endpoint');
  } else {
    n = direct(native); a = direct(actual);
    const prefix = (view, item) => view.tokens(item.statement).filter(token => token.end <= start(view, item.value.right));
    compareTokens(native, prefix(native, n), actual, prefix(actual, a), 'lowered class-valued direct storage prefix');
  }
  function bare(view) {
    const refs = [];
    function visit(node) {
      if (ts.isIdentifier(node) && node.text === name
        && !(ts.isPropertyAccessExpression(node.parent) && node.parent.name === node)
        && !(ts.isPropertyAssignment(node.parent) && node.parent.name === node)) refs.push(node);
      ts.forEachChild(node, visit);
    }
    visit(view.source); return refs;
  }
  const nr = bare(native), ar = bare(actual);
  assert.equal(ar.length, nr.length, 'complete original local binding/reference inventory');
  assert.equal(nr.length, preserved ? 2 : 0, 'only preserved const declaration and immediate publication use a bare binding');
  if (preserved) assert(ar.includes(a.variable.name) && ar.includes(a.value.right));
  const references = (view, after) => {
    const refs = [];
    function visit(node) {
      if (ts.isPropertyAccessExpression(node) && node.expression.getText(view.source) === 'exports' && node.name.text === name
        && start(view, node) > after) refs.push(node);
      ts.forEachChild(node, visit);
    }
    visit(view.source); return refs;
  };
  const nn = references(native, preserved ? n.published.end : n.statement.end), aa = references(actual, preserved ? a.published.end : a.statement.end);
  assert.equal(aa.length, nn.length, 'complete later live export-reference inventory');
  for (let index = 0; index < nn.length; index++) compareTokens(native, native.tokens(nn[index]), actual, actual.tokens(aa[index]), 'later original class export read');
  return {preserved, laterReferences: nn.length};
}
const verified = [], failures = [];
for (const row of observations) {
  const spec = graph.resolve(input, input.cases[row.index], parents), expected = reference.cases[row.index];
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2); assert.equal(row.id, expected.id);
  for (const [name, entry] of Object.entries(expected.maps)) if (entry.kind === 'javascript') {
    const code = row.outputs[entry.output], raw = name.endsWith('#inline') ? Buffer.from(code.split('base64,')[1].trim(), 'base64').toString() : row.outputs[name];
    try {
      const native = stream(ts, reference.blobs[expected.outputs[entry.output]], authority.raw(reference, expected, name)), actual = stream(ts, code, raw);
      verified.push({index: row.index, id: row.id, ...compare(native, actual, spec.files['main.ts'], row.options), actualJavaScriptSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw)});
    } catch (error) {failures.push({index: row.index, id: row.id, error: error.message, stack: error.stack});}
  }
}
const report = {scope: 'CJS class-value storage/publication component; full class/module roles remain required', compilerSha256,
  configurations: observations.length, repeatBuilds: 2, completeFamily: false, verified, failures};
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'class-value-publication.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify(report, null, 2)); if (failures.length) process.exitCode = 1;
