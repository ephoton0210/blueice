// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Explicit async ABI profiles. No Blue output is used as an expected snapshot.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), { stream, compareTokens, compareGap } = require('./ge5_role_stream.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json')));
const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json')));
const observations = JSON.parse(fs.readFileSync(0, 'utf8'));
const generators = process.argv.includes('--generators'), expectedCount = generators ? 200 : 100;
assert.equal(observations.length, expectedCount);
const compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc')));
for (const row of observations) {
  assert.equal(row.compilerSha256, compilerSha256, 'observation belongs to the measured compiler');
  assert.equal(row.repeatBuilds, 2, 'both public CLI builds are required');
}
assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'cases.json'))), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'reference.json'))), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const forms = (generators ? ['opaque-generator-es5', 'switch-suspension-fallthrough', 'binary-yield-order',
  'logical-yield-order', 'computed-assignment-yield', 'switch-exception-finally', 'finally-overrides-return',
  'loop-abrupt-finally', 'delegate-external-return-finally', 'template-yield-order']
  : ['conditional-await', 'conditional-expression-await', 'template-await-order', 'nested-await-call', 'helper-name-collisions'])
  .map(name => 'ge1_downlevel/' + name);
const parents = graph.parents(root, input), selected = [];
for (const [index, specification] of input.cases.entries()) {
  if (!forms.includes(specification.form)) continue;
  const row = graph.resolve(input, specification, parents);
  if (row.options.target !== 'ES5' && ['CommonJS', 'ESNext'].includes(row.options.module)) selected.push(index);
}
assert.deepEqual(observations.map(row => row.index), selected, 'complete frozen configuration selection');

function wrapper(node, dialect) {
  if (!ts.isFunctionDeclaration(node) || !node.body) return null;
  const statements = node.body.statements, returned = statements.at(-1);
  if (!ts.isReturnStatement(returned) || !returned.expression || !ts.isCallExpression(returned.expression)) return null;
  const call = returned.expression, expected = dialect === 'native' ? /^__awaiter(?:_\d+)?$/ : /^__blueice_target_async_\d+$/;
  if (!ts.isIdentifier(call.expression) || !expected.test(call.expression.text)) return null;
  const callback = call.arguments.at(-1);
  assert(ts.isFunctionExpression(callback) && callback.asteriskToken && callback.body);
  assert.equal(call.arguments.length, dialect === 'native' ? 4 : 3);
  assert.equal(call.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
  if (dialect === 'blue') assert.equal(call.arguments[1].getText(), 'arguments');
  else assert.equal(call.arguments[2].getText().replaceAll(' ', ''), 'void0');
  assert.equal(statements.length, dialect === 'native' ? 1 : 2);
  if (dialect === 'blue') {
    assert.equal(statements[0].getText().replace(/\s/g, ''), 'if(new.target)thrownewTypeError("Asyncfunctionisnotaconstructor");');
  }
  return { node, call, callback, helper: call.expression.text, guard: dialect === 'blue' ? statements[0] : null };
}

function originalCharacter(view, owner) {
  if (!owner) return null;
  const text = view.decoded.metadata.sourcesContent[owner[0]];
  return text.split('\n')[owner[1]]?.[owner[2]];
}

function bodyTokens(view, body, dialect) {
  const result = [], extra = [];
  function visit(node) {
    if (ts.isParenthesizedExpression(node)
        && (ts.isYieldExpression(node.expression) || ts.isYieldExpression(node.parent))
        && originalCharacter(view, view.owner(node.getStart(view.source))) !== '(') {
      const children = node.getChildren(view.source), open = children[0], close = children.at(-1);
      extra.push([open.getStart(view.source), open.end], [close.getStart(view.source), close.end]);
      visit(node.expression); return;
    }
    if (ts.isToken(node)) { if (node.kind !== ts.SyntaxKind.EndOfFileToken) result.push(view.token(node)); return; }
    for (const child of node.getChildren(view.source)) visit(child);
  }
  for (const statement of body.statements) visit(statement);
  // Every removed grouping delimiter still needs a precise source role. It
  // must point to an await node boundary, never an arbitrary enclosing span.
  const borrowed = new Set(result.flatMap(token => [...token.owners, view.owner(token.end)]).map(JSON.stringify));
  for (const [start, end] of extra) {
    if (dialect === 'blue') view.noSource(start, end, 'additional yield grouping');
    else for (let offset = start; offset < end; offset++) assert(borrowed.has(JSON.stringify(view.owner(offset))),
      'Native precedence grouping must retain an already represented operand boundary');
  }
  return { tokens: result, grouping: extra };
}

function parts(view, dialect, lowered) {
  const wrapped = new Map();
  for (const node of view.source.statements) { const found = wrapper(node, dialect); if (found) wrapped.set(node, found); }
  assert.equal(wrapped.size > 0, lowered, 'async target profile');
  const helpers = new Set([...wrapped.values()].map(item => item.helper));
  const result = [];
  for (const node of view.source.statements) {
    const helper = ts.isFunctionDeclaration(node) && node.name?.text;
    const variable = ts.isVariableStatement(node) && node.declarationList.declarations.length === 1 && node.declarationList.declarations[0].name.getText();
    if (helpers.has(helper || variable)) { view.noSource(node.getFullStart(), node.end, 'async helper definition'); continue; }
    const item = wrapped.get(node);
    if (!item) { result.push({ kind: 'statement', start: node.getStart(view.source), end: node.end, tokens: view.tokens(node) }); continue; }
    const header = view.tokens(node).filter(token => token.end <= node.body.getStart(view.source));
    const open = header.findIndex(token => token.text === '('), close = header.map(token => token.text).lastIndexOf(')');
    const parameters = dialect === 'native' ? header.slice(open, close + 1)
      : view.tokens(item.callback).filter(token => token.start >= item.callback.parameters.pos - 1 && token.end <= item.callback.parameters.end + 1);
    if (item.guard) view.noSource(item.guard.getFullStart(), item.guard.end, 'async constructor guard');
    const nativeBody = bodyTokens(view, item.callback.body, dialect);
    const signatureStart = dialect === 'native' ? header[open].start : parameters[0].start;
    const signatureEnd = dialect === 'native' ? header[close].end : parameters.at(-1).end;
    const bodyStart = item.callback.body.getStart(view.source), bodyEnd = item.callback.body.end;
    if (dialect === 'blue') {
      // A moved signature has Native user ownership. The outer argument slots,
      // constructor guard, driver call and callback declaration are generated.
      view.noSource(header[open].start, node.body.getStart(view.source), 'async argument slots');
      view.noSource(node.body.getStart(view.source) + 1, signatureStart, 'async driver setup');
    } else {
      const start = node.body.getStart(view.source) + 1, end = node.body.statements[0].getStart(view.source);
      for (let offset = start; offset < end; offset++) {
        const owner = view.owner(offset);
        assert(owner === null || JSON.stringify(owner) === JSON.stringify(view.owner(start - 1)), 'Native async entry trivia');
      }
      view.cover(start, end);
      view.noSource(end, bodyStart + 1, 'Native async driver setup');
    }
    const bodyTokensList = view.tokens(item.callback.body), closing = view.tokens(node.body).at(-1);
    const completion = view.tokens(node).filter(token => token.start >= bodyEnd && token.end <= closing.start);
    assert.deepEqual(completion.map(token => token.text), [')', ';'], 'complete async driver completion');
    result.push({ kind: 'async', start: node.getStart(view.source), end: node.end,
      name: node.name.text, prefix: header.slice(0, open), parameters,
      open: [view.tokens(node.body)[0]], body: nativeBody.tokens, grouping: nativeBody.grouping,
      movedEntry: [signatureEnd, bodyStart + 1], bodyOpen: bodyStart + 1,
      bodyClose: [bodyTokensList.at(-1)], completion, close: [closing], outerParameters: header[open].start });
  }
  return result;
}

function compareModule(native, actual, lowered) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  if (actual.code === native.code) assert.deepEqual(actual.decoded, native.decoded, 'identical JavaScript requires every decoded entry');
  const wanted = parts(native, 'native', lowered), observed = parts(actual, 'blue', lowered);
  assert.equal(observed.length, wanted.length, 'complete top-level statement inventory');
  for (let index = 0; index < wanted.length; index++) {
    const a = wanted[index], b = observed[index]; assert.equal(b.kind, a.kind);
    if (a.kind === 'statement') compareTokens(native, a.tokens, actual, b.tokens, 'module statement ' + index);
    else {
      assert.equal(b.name, a.name);
      for (const key of ['prefix', 'parameters', 'open', 'body', 'bodyClose', 'completion', 'close']) compareTokens(native, a[key], actual, b[key], a.name + ':' + key, key === 'body' ? b.grouping : []);
      compareGap(native, a.prefix.at(-1).end, a.parameters[0].start, actual, b.prefix.at(-1).end, b.outerParameters, a.name + ': signature prefix');
      for (let offset = a.parameters.at(-1).end; offset < a.open[0].start; offset++) {
        assert.deepEqual(native.owner(offset), a.open[0].owners[0], a.name + ': Native signature gap');
      }
      native.cover(a.parameters.at(-1).end, a.open[0].start);
      for (let offset = b.movedEntry[0]; offset < b.movedEntry[1]; offset++) {
        assert.deepEqual(actual.owner(offset), a.open[0].owners[0], a.name + ': moved body entry');
      }
      actual.cover(...b.movedEntry);
      const firstA = a.body[0]?.start ?? a.bodyClose[0].start, firstB = b.body[0]?.start ?? b.bodyClose[0].start;
      compareGap(native, a.bodyOpen, firstA, actual, b.bodyOpen, firstB, a.name + ': body leading gap');
      const lastA = a.body.at(-1)?.end ?? a.bodyOpen, lastB = b.body.at(-1)?.end ?? b.bodyOpen;
      compareGap(native, lastA, a.bodyClose[0].start, actual, lastB, b.bodyClose[0].start, a.name + ': body trailing gap');
      compareGap(native, a.bodyClose[0].end, a.completion[0].start, actual, b.bodyClose[0].end, b.completion[0].start, a.name + ': completion prefix gap');
      compareGap(native, a.completion.at(-1).end, a.close[0].start, actual, b.completion.at(-1).end, b.close[0].start, a.name + ': completion trailing gap');
    }
    compareGap(native, index ? wanted[index - 1].end : 0, a.start,
      actual, index ? observed[index - 1].end : 0, b.start, 'module gap ' + index);
  }
  compareGap(native, wanted.at(-1)?.end ?? 0, native.code.length,
    actual, observed.at(-1)?.end ?? 0, actual.code.length, 'module trailing gap');
  native.complete('Native module'); actual.complete('Blue module');
  assert.deepEqual(actual.owner(actual.code.length), native.owner(native.code.length), 'module EOF owner');
}

let maps = 0; const failures = [], mutations = [], verified = [];
for (const row of observations) {
  const expected = reference.cases[row.index]; assert.equal(row.id, expected.id);
  for (const [name, artifact] of Object.entries(expected.maps)) {
    if (artifact.kind !== 'javascript') continue; maps++;
    try {
      const native = stream(ts, reference.blobs[expected.outputs[artifact.output]], reference.blobs[artifact.raw]);
      const actual = stream(ts, row.outputs[artifact.output], row.outputs[name]);
      const lowered = !generators && ['ES2015', 'ES2016'].includes(row.target);
      compareModule(native, actual, lowered);
      if (lowered && row.id.includes('/conditional-await-es2015-commonjs-')) {
        mutations.push({ id: row.id, controls: require('./ge5_role_mutations.cjs').run(ts, native, actual, compareModule) });
      }
      verified.push({ index: row.index, id: row.id, map: name,
        nativeJavaScriptSha256: expected.outputs[artifact.output], nativeMapSha256: artifact.raw,
        actualJavaScriptSha256: graph.sha256(actual.code), actualMapSha256: graph.sha256(actual.raw),
        generatedUtf16Units: actual.code.length, allCharactersClassified: true });
    } catch (error) { failures.push({ id: row.id, map: name, error: error.message }); }
  }
}
assert.equal(maps, expectedCount);
const report = { configurations: observations.length, maps, verified, mutations, failures };
report.compilerSha256 = compilerSha256;
report.repeatBuilds = 2;
report.completeFamilies = false;
report.scope = generators ? 'ES2015 and later native generator syntax' : 'ES2015 and later named async functions';
report.pending = ['ES5 and additional publication profiles', 'remaining lowering families', 'final G-E5 gates'];
if (!failures.length) assert.equal(mutations.reduce((sum, row) => sum + row.controls.length, 0), generators ? 0 : 18);
if (process.env.BLUEICE_GE5_ASYNC_ROLE_REPORT) fs.writeFileSync(process.env.BLUEICE_GE5_ASYNC_ROLE_REPORT, JSON.stringify(report, null, 2) + '\n');
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, { recursive: true });
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, generators ? 'generators.json' : 'asynchronous.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify({ configurations: observations.length, maps, mutations, failures }, null, 2));
if (failures.length) process.exitCode = 1;
