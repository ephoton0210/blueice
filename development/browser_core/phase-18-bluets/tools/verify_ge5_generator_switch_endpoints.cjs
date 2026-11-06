// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete public products expose operand, selector and case-completion boundaries.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const {stream, compareTokens} = require('./ge5_role_stream.cjs');
const authority = require('./ge5_native_authority.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json'));
const reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const rows = process.argv[2]
  ? require('./ge5_callable_observe.cjs').collect(path.resolve(process.argv[2]), process.argv[3], 'ge2_module_helpers/generator-switch-delegation', 363)
  : JSON.parse(fs.readFileSync(0, 'utf8'));
assert.deepEqual(rows.map(row => row.index), input.cases.flatMap((row, index) => row.form === 'ge2_module_helpers/generator-switch-delegation' ? [index] : []));
if (process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY, {recursive: true});
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY, 'generator-switch-delegation.json'), JSON.stringify(rows) + '\n');
}
const verified = [], failures = [];
function find(node, predicate) {
  const result = [];
  function visit(n) {if (predicate(n)) result.push(n); ts.forEachChild(n, visit);}
  visit(node); return result;
}
function unique(nodes, label) {assert.equal(nodes.length, 1, label); return nodes[0];}
for (const row of rows) {
  const expected = reference.cases[row.index];
  assert.equal(row.id, expected.id); assert.equal(row.repeatBuilds, 2);
  for (const [name, map] of Object.entries(expected.maps)) if (map.kind === 'javascript') {
    const code = row.outputs[map.output], raw = name.endsWith('#inline')
      ? Buffer.from(code.split('base64,')[1], 'base64').toString() : row.outputs[name];
    const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, name));
    const actual = stream(ts, code, raw);
    const fn = view => unique(find(view.source, node => ts.isFunctionDeclaration(node) && node.name?.text === 'sequence'), 'original generator declaration');
    const nf = fn(native), af = fn(actual), lowered = row.options.target === 'ES5';
    function check(role, compare) {
      try {compare(); verified.push({id: row.id, map: name, role});}
      catch (error) {failures.push({id: row.id, map: name, role, error: error.message});}
    }
    check('complete generator header', () => compareTokens(native, native.tokens(nf).filter(t => t.end <= nf.body.getStart(native.source) + 1),
      actual, actual.tokens(af).filter(t => t.end <= af.body.getStart(actual.source) + 1), 'complete generator signature'));
    const dense = (view, node) => unique(find(node, n => ts.isArrayLiteralExpression(n) && n.elements.length === 2
      && n.elements.every(ts.isNumericLiteral) && n.elements.map(e => e.text).join(',') === '20,30'), 'exact source delegated dense array');
    const na = dense(native, nf), aa = dense(actual, af);
    check('complete dense-array operand', () => compareTokens(native, native.tokens(na), actual, actual.tokens(aa), 'complete delegated dense array'));
    check('dense-array logical ending', () => assert.deepEqual(actual.owner(aa.end), native.owner(na.end)));
    if (!lowered) {
      check('complete retained generator', () => compareTokens(native, native.tokens(nf), actual, actual.tokens(af), 'complete retained switch/delegation generator'));
      continue;
    }
    for (const value of ['10', '0']) {
      const nx = unique(find(nf, n => ts.isReturnStatement(n) && ts.isArrayLiteralExpression(n.expression)
        && n.expression.elements.length === 2 && n.expression.elements[0].getText(native.source) === '4'
        && ts.isNumericLiteral(n.expression.elements[1]) && n.expression.elements[1].text === value), 'source yield operand ' + value).expression.elements[1];
      const ax = unique(find(af, n => ts.isBinaryExpression(n) && n.operatorToken.kind === ts.SyntaxKind.EqualsToken
        && ts.isIdentifier(n.left) && ts.isNumericLiteral(n.right) && n.right.text === value), 'captured source yield operand ' + value).right;
      check('yield ' + value + ' complete operand', () => compareTokens(native, native.tokens(nx), actual, actual.tokens(ax), 'source yield literal'));
      check('yield ' + value + ' logical ending', () => assert.deepEqual(actual.owner(ax.end), native.owner(nx.end)));
    }
    const selector = (view, fn) => unique(find(fn, n => ts.isBinaryExpression(n) && n.operatorToken.kind === ts.SyntaxKind.EqualsToken
      && ts.isIdentifier(n.left) && ts.isIdentifier(n.right) && n.right.text === 'value'), 'one original switch-selector evaluation');
    const ns = selector(native, nf), as = selector(actual, af);
    check('switch-selector operand', () => compareTokens(native, native.tokens(ns.right), actual, actual.tokens(as.right), 'switch selector'));
    check('switch-selector logical ending', () => assert.deepEqual(actual.owner(as.right.end), native.owner(ns.right.end)));
    const sw = (view, fn) => find(fn, n => ts.isSwitchStatement(n) && ts.isIdentifier(n.expression));
    const nw = unique(sw(native, nf), 'one Native source switch'), aws = sw(actual, af);
    const external = row.options.importHelpers || row.options.noEmitHelpers;
    assert.equal(aws.length, external ? 0 : 1, 'complete source-switch lowering inventory');
    for (const clause of nw.caseBlock.clauses) if (ts.isCaseClause(clause)) {
      const value = clause.expression.text;
      const colon = native.tokens(clause).find(t => t.text === ':'); assert(colon);
      if (external) {
        const captured = unique(find(af, n => ts.isBinaryExpression(n) && n.operatorToken.kind === ts.SyntaxKind.EqualsToken
          && ts.isIdentifier(n.left) && ts.isNumericLiteral(n.right) && n.right.text === value), 'one captured source case test ' + value);
        check('case ' + value + ' test operand', () => compareTokens(native, native.tokens(clause.expression), actual, actual.tokens(captured.right), 'ordered source case test'));
        check('case ' + value + ' test ending', () => assert.deepEqual(actual.owner(captured.right.end), native.owner(clause.expression.end)));
        check('case ' + value + ' punctuation ending', () => assert.deepEqual(actual.owner(captured.parent.end), native.owner(colon.end)));
        const branch = unique(find(af, n => ts.isBinaryExpression(n) && ts.isConditionalExpression(n.right)
          && ts.isBinaryExpression(n.right.condition) && ts.isIdentifier(n.right.condition.right)
          && n.right.condition.right.text === captured.left.text), 'one selection branch for the original case test');
        const opcode = clause.statements[0];
        check('case ' + value + ' selection phase', () => {
          const wanted = native.owner(opcode.getStart(native.source)); assert(wanted);
          for (let at = branch.getStart(actual.source); at < branch.parent.end; at++) assert.deepEqual(actual.owner(at), wanted);
          assert.deepEqual(actual.owner(branch.parent.end), native.owner(opcode.end));
        });
      } else {
        const peer = unique(aws[0].caseBlock.clauses.filter(c => ts.isCaseClause(c) && c.expression.text === value), 'ordered source case ' + value);
        const ac = actual.tokens(peer).find(t => t.text === ':'); assert(ac);
        check('case ' + value + ' punctuation ending', () => assert.deepEqual(actual.owner(ac.end), native.owner(colon.end)));
      }
    }
  }
}
const report = {scope: 'public source endpoints; independent wholemodule closure required', completeFamily: false,
  configurations: rows.length, repeatBuilds: 2, compilerSha256: rows[0].compilerSha256, verified, failures};
if (process.env.BLUEICE_GE5_REPORT) fs.writeFileSync(process.env.BLUEICE_GE5_REPORT, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({...report, verified: verified.length, failures: failures.slice(0, 8)}, null, 2));
assert.equal(failures.length, 0, 'all frozen generator source endpoints');
