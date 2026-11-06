// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete, finite continuation ABI for the frozen opaque-generator program.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const graph = require('./ge5_inputs.cjs'), { stream, compareTokens, compareGap } = require('./ge5_role_stream.cjs');
const root = path.resolve(__dirname, '../../../..'), directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'))), reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json')));
const observations = JSON.parse(fs.readFileSync(0, 'utf8')), parents = graph.parents(root, input);
const selected = input.cases.flatMap((row, index) => row.form === 'ge1_downlevel/opaque-generator-es5'
  && graph.resolve(input, row, parents).options.target === 'ES5' ? [index] : []);
assert.deepEqual(observations.map(row => row.index), selected); assert.equal(selected.length, 7);
assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'cases.json'))), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(fs.readFileSync(path.join(directory, 'reference.json'))), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const compilerSha256 = graph.sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc')));

function property(node, name) {
  assert(ts.isObjectLiteralExpression(node));
  assert.equal(node.properties.length, 2);
  const item = node.properties.find(item => ts.isPropertyAssignment(item) && item.name.getText() === name);
  assert(item, 'completion field ' + name); return item.initializer;
}
function label(statement, context) {
  if (!ts.isExpressionStatement(statement) || !ts.isBinaryExpression(statement.expression)) return null;
  const { left, right, operatorToken } = statement.expression;
  if (left.getText() !== context + '.label') return null;
  assert.equal(operatorToken.kind, ts.SyntaxKind.EqualsToken); assert(ts.isNumericLiteral(right)); return Number(right.text);
}
function nativeFunction(view, node) {
  assert.equal(node.body.statements.length, 1); const returned = node.body.statements[0];
  assert(ts.isReturnStatement(returned) && ts.isCallExpression(returned.expression));
  const call = returned.expression; assert.match(call.expression.getText(), /^__generator(?:_\d+)?$/);
  assert.equal(call.arguments.length, 2); assert.equal(call.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
  const callback = call.arguments[1]; assert(ts.isFunctionExpression(callback)); assert.equal(callback.parameters.length, 1);
  assert(!callback.asteriskToken && !callback.modifiers?.length);
  assert.equal(callback.body.statements.length, 1); const dispatcher = callback.body.statements[0]; assert(ts.isSwitchStatement(dispatcher));
  const context = callback.parameters[0].name.getText(); assert.equal(dispatcher.expression.getText(), context + '.label');
  const clauses = dispatcher.caseBlock.clauses; assert.equal(clauses.length, 2);
  clauses.forEach((clause, index) => { assert(ts.isCaseClause(clause)); assert.equal(clause.expression.getText(), String(index)); });
  const first = clauses[0].statements, last = clauses[1].statements;
  assert.equal(first.length, 3); assert(ts.isSwitchStatement(first[0]) && ts.isEmptyStatement(first[1]));
  const yielded = first[2]; assert(ts.isReturnStatement(yielded) && ts.isArrayLiteralExpression(yielded.expression));
  assert.equal(yielded.expression.elements.length, 2); assert.equal(yielded.expression.elements[0].getText(), '4');
  const operand = yielded.expression.elements[1]; assert(ts.isNumericLiteral(operand));
  assert.equal(last.length, 2); assert(ts.isExpressionStatement(last[0]) && ts.isReturnStatement(last[1]));
  assert.equal(last[1].expression.getText().replace(/\/\*.*?\*\//g, '').replace(/\s/g, ''), '[2]');
  return { node, helper: call.expression.getText(), context, dispatcher, callback,
    control: first.slice(0, 2), operand, yielded, resume: last[0], terminal: last[1] };
}
function blueFunction(view, node) {
  assert.equal(node.body.statements.length, 3);
  assert(node.body.statements.slice(0, 2).every(ts.isVariableStatement));
  const returned = node.body.statements[2]; assert(ts.isReturnStatement(returned) && ts.isCallExpression(returned.expression));
  const call = returned.expression; assert.match(call.expression.getText(), /^__blueice_target_generator_\d+$/);
  assert.equal(call.arguments.length, 3); assert.equal(call.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
  assert(ts.isNumericLiteral(call.arguments[1]) && ts.isFunctionExpression(call.arguments[2]));
  const callback = call.arguments[2]; assert.equal(callback.parameters.length, 1);
  const context = callback.parameters[0].name.getText(); assert.match(context, /^__blueice_target_generator_context_\d+$/);
  assert(!callback.asteriskToken && !callback.modifiers?.length);
  const capture = node.body.statements[0].declarationList.declarations, locals = node.body.statements[1].declarationList.declarations;
  assert.equal(capture.length, 1); assert.match(capture[0].name.getText(), /^__blueice_target_generator_arguments_\d+$/);
  assert.equal(capture[0].initializer.getText(), 'arguments');
  assert.equal(locals.length, 1); assert.equal(locals[0].initializer, undefined);
  assert.match(locals[0].name.getText(), new RegExp('^' + context + '_value_\\d+$'));
  assert.equal(callback.body.statements.length, 1); const loop = callback.body.statements[0];
  assert(ts.isWhileStatement(loop) && loop.expression.kind === ts.SyntaxKind.TrueKeyword && ts.isBlock(loop.statement));
  assert.equal(loop.statement.statements.length, 1); const dispatcher = loop.statement.statements[0]; assert(ts.isSwitchStatement(dispatcher));
  assert.equal(dispatcher.expression.getText(), context + '.label');
  const states = new Map(); let defaults = 0;
  for (const clause of dispatcher.caseBlock.clauses) {
    if (ts.isDefaultClause(clause)) {
      defaults++; assert.equal(clause.statements.map(node => node.getText()).join('').replace(/\s/g, ''), 'thrownewError("Invalidgeneratorcontinuation");'); continue;
    }
    assert(ts.isCaseClause(clause) && ts.isNumericLiteral(clause.expression)); const key = Number(clause.expression.text);
    assert(!states.has(key)); states.set(key, clause.statements);
  }
  assert.equal(defaults, 1); assert.equal(states.size, 5);
  let cursor = Number(call.arguments[1].text); const visited = new Set(), result = { node, helper: call.expression.getText(), context, dispatcher, callback };
  const order = [];
  while (true) {
    assert(states.has(cursor) && !visited.has(cursor), 'finite reachable continuation'); visited.add(cursor);
    const statements = states.get(cursor);
    if (statements.length === 1 && ts.isReturnStatement(statements[0])) {
      result.terminal = statements[0]; assert.equal(property(result.terminal.expression, 'kind').text, 'return');
      assert.equal(property(result.terminal.expression, 'value').getText().replace(/\s/g, ''), 'void0'); order.push('complete'); break;
    }
    if (statements.length === 2 && ts.isReturnStatement(statements[1])) {
      const next = label(statements[0], context); assert.notEqual(next, null);
      result.yielded = statements[1]; assert.equal(property(result.yielded.expression, 'kind').text, 'yield');
      const value = property(result.yielded.expression, 'value'); assert(ts.isParenthesizedExpression(value) && ts.isIdentifier(value.expression));
      assert.equal(value.expression.text, result.temporary, 'yield uses the previously evaluated operand'); result.value = value;
      order.push('yield'); cursor = next; continue;
    }
    assert(ts.isContinueStatement(statements.at(-1))); const next = label(statements.at(-2), context); assert.notEqual(next, null);
    const retained = statements.slice(0, -2);
    if (ts.isSwitchStatement(retained[0])) {
      assert.equal(retained.length, 2); assert(ts.isEmptyStatement(retained[1])); result.control = retained; order.push('control');
    } else if (retained.length === 1 && ts.isExpressionStatement(retained[0]) && ts.isBinaryExpression(retained[0].expression)) {
      const assignment = retained[0].expression; assert.equal(assignment.operatorToken.kind, ts.SyntaxKind.EqualsToken);
      assert(ts.isIdentifier(assignment.left) && ts.isNumericLiteral(assignment.right));
      result.temporary = assignment.left.text; assert(result.temporary.startsWith(context + '_value_')); result.operand = assignment.right;
      result.operandCapture = retained[0]; order.push('operand');
    } else {
      assert.equal(retained.length, 1); assert(ts.isExpressionStatement(retained[0]) && ts.isCallExpression(retained[0].expression));
      result.resume = retained[0]; order.push('resume');
    }
    cursor = next;
  }
  assert.equal(visited.size, states.size, 'no unreviewed unreachable states');
  assert.equal(result.temporary, locals[0].name.getText(), 'operand uses the declared local slot');
  assert.deepEqual(order, ['control', 'operand', 'yield', 'resume', 'complete']); return result;
}
function compareFunction(native, n, actual, a) {
  const roles = new Map([[native, []], [actual, []]]);
  const span = (view, start, end, trailing) => { view.cover(start, end); roles.get(view).push({ start, end, trailing }); };
  function same(wanted, observed, name) {
    compareTokens(native, wanted, actual, observed, name);
    if (wanted.length) {
      const trailing = native.owner(wanted.at(-1).end);
      span(native, wanted[0].start, wanted.at(-1).end, trailing);
      span(actual, observed[0].start, observed.at(-1).end, trailing);
    }
  }
  const header = (view, fn) => view.tokens(fn.node).filter(token => token.end <= fn.node.body.getStart(view.source) + 1);
  same(header(native, n), header(actual, a), 'complete generator signature');
  same(n.control.flatMap(node => native.tokens(node)), a.control.flatMap(node => actual.tokens(node)), 'retained non-suspending switch');
  same([native.token(n.operand)], [actual.token(a.operand)], 'yield operand');
  const owned = (view, start, end, owner, label) => {
    for (let offset = start; offset < end; offset++) assert.deepEqual(view.owner(offset), owner, label);
    span(view, start, end, owner);
  };
  const entry = native.owner(n.yielded.getStart(native.source)), completed = native.owner(n.operand.end);
  assert(entry && completed, 'Native suspension boundaries');
  actual.noSource(a.operandCapture.getStart(actual.source), a.operand.getStart(actual.source), 'generated operand capture assignment');
  owned(actual, a.operand.end, a.operandCapture.end, completed, 'captured yield operand logical endpoint');
  owned(native, n.yielded.getStart(native.source), n.operand.getStart(native.source), entry, 'Native yield opcode');
  owned(actual, a.yielded.getStart(actual.source), a.value.getStart(actual.source), entry, 'Blue yield opcode');
  actual.noSource(a.value.getStart(actual.source), a.value.expression.end, 'generated yield temporary/grouping');
  owned(native, n.operand.end, n.yielded.end, completed, 'Native yield endpoint');
  owned(actual, a.value.expression.end, a.yielded.end, completed, 'Blue yield endpoint');
  function sent(view, fn) {
    const call = fn.resume.expression; assert(ts.isCallExpression(call) && call.arguments.length === 0);
    assert.equal(call.expression.getText(), fn.context + '.sent');
    return view.tokens(call).map(token => {
      if (token.text !== fn.context) return token;
      assert(token.owners.every(owner => JSON.stringify(owner) === JSON.stringify(token.owners[0])), 'uniform fresh context alias');
      return { ...token, text: '$continuation', owners: [token.owners[0]] };
    });
  }
  same(sent(native, n), sent(actual, a), 'resumed suspension value');
  same([native.tokens(n.resume).at(-1)], [actual.tokens(a.resume).at(-1)], 'original yield terminator');
  native.noSource(n.terminal.getStart(native.source), n.terminal.end, 'Native implicit completion');
  actual.noSource(a.terminal.getStart(actual.source), a.terminal.end, 'Blue implicit completion');
  // Native prints its driver setup on the signature line. Its only permissible
  // borrowed owner is that exact signature boundary; Blue emits it unmapped.
  const signature = native.owner(n.node.body.getStart(native.source));
  for (let offset = n.node.body.getStart(native.source) + 1; offset < n.dispatcher.getStart(native.source); offset++) {
    const owner = native.owner(offset); assert(owner === null || JSON.stringify(owner) === JSON.stringify(signature), 'Native driver signature boundary');
  }
  native.cover(n.node.body.getStart(native.source) + 1, n.dispatcher.getStart(native.source));
  for (const [view, fn] of [[native, n], [actual, a]]) {
    const ordered = roles.get(view).sort((left, right) => left.start - right.start);
    for (let offset = fn.node.getStart(view.source); offset < fn.node.end; offset++) {
      if (view.covered(offset)) continue;
      const previous = ordered.findLast(role => role.end <= offset), owner = view.owner(offset);
      assert(owner === null || /\s/.test(view.code[offset]) && previous && JSON.stringify(owner) === JSON.stringify(previous.trailing),
        'unclassified source grant in continuation scaffold at ' + offset + ': ' + JSON.stringify(owner));
      view.cover(offset, offset + 1);
    }
  }
}
function parts(view, dialect, originals) {
  const result = []; let functions = 0;
  for (const node of view.source.statements) {
    const name = ts.isFunctionDeclaration(node) ? node.name?.text : ts.isVariableStatement(node)
      && node.declarationList.declarations.length === 1 && node.declarationList.declarations[0].name.getText();
    if (name === 'sequence') {
      assert(ts.isFunctionDeclaration(node)); functions++;
      const generator = dialect === 'native' ? nativeFunction(view, node) : blueFunction(view, node);
      assert(!originals.some(source => source.includes(generator.context)), 'only fresh continuation bindings may be renamed');
      result.push({ start: node.getStart(view.source), end: node.end, generator }); continue;
    }
    if (name && (dialect === 'native' ? /^__generator(?:_\d+)?$/ : /^__blueice_target_(?:generator|delegate_iterator|delegate_step)_\d+$/).test(name)) {
      assert(!originals.some(source => source.includes(name)), 'only fresh generated helpers are classified');
      view.noSource(node.getFullStart(), node.end, 'complete generator helper definition'); continue;
    }
    result.push({ start: node.getStart(view.source), end: node.end, tokens: view.tokens(node) });
  }
  assert.equal(functions, 1); return result;
}
function check(native, actual, originals) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  const wanted = parts(native, 'native', originals), observed = parts(actual, 'blue', originals);
  assert.equal(observed.length, wanted.length);
  for (let index = 0; index < wanted.length; index++) {
    const n = wanted[index], a = observed[index]; assert.equal(Boolean(a.generator), Boolean(n.generator));
    if (n.generator) compareFunction(native, n.generator, actual, a.generator);
    else compareTokens(native, n.tokens, actual, a.tokens, 'module statement ' + index);
    compareGap(native, index ? wanted[index - 1].end : 0, n.start, actual, index ? observed[index - 1].end : 0, a.start, 'module gap');
  }
  compareGap(native, wanted.at(-1).end, native.code.length, actual, observed.at(-1).end, actual.code.length, 'module trailing gap');
  assert.deepEqual(actual.owner(actual.code.length), native.owner(native.code.length)); native.complete('Native module'); actual.complete('Blue module');
}

function negativeControls(native, actual, originals) {
  const { mutate } = require('./ge5_role_mutations.cjs');
  const fn = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'sequence');
  const profile = blueFunction(actual, fn);
  const helper = actual.source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === profile.helper);
  assert(helper);
  const tests = [
    ['retarget control token interior', profile.control[0].getStart(actual.source) + 1, [0, 0, 0]],
    ['erase control token interior', profile.control[0].getStart(actual.source) + 1, null],
    ['retarget yield operand', profile.operand.getStart(actual.source), [0, 0, 0]],
    ['erase captured yield operand endpoint', profile.operand.end, null],
    ['retarget captured yield operand endpoint', profile.operand.end, [0, 0, 0]],
    ['erase yield opcode', profile.yielded.getStart(actual.source), null],
    ['erase yield endpoint', profile.value.end - 1, null],
    ['erase resumed context interior', profile.resume.expression.getStart(actual.source) + 1, null],
    ['erase original yield terminator', profile.resume.end - 1, null],
    ['grant dispatcher source', profile.dispatcher.getStart(actual.source), [0, 0, 0]],
    ['grant helper definition source', helper.name.getStart(actual.source), [0, 0, 0]],
  ];
  const controls = tests.map(([label, offset, owner]) => {
    const changed = mutate(ts, actual, offset, owner);
    assert.throws(() => check(native, changed, originals), undefined, label); return { label, rejected: true };
  });
  const state = profile.dispatcher.caseBlock.clauses.find(clause => ts.isCaseClause(clause)
    && clause.statements.some(statement => ts.isReturnStatement(statement) && statement === profile.yielded));
  const target = state.statements[0].expression.right;
  const code = actual.code.slice(0, target.getStart(actual.source)) + '0' + actual.code.slice(target.end);
  assert.equal(code.length, actual.code.length, 'isolated same-width control-flow mutation');
  const changed = stream(ts, code, actual.raw);
  assert.throws(() => check(native, changed, originals), undefined, 'skip resume state');
  controls.push({ label: 'skip reachable resume state', rejected: true });
  return controls;
}

const verified = [], failures = [], mutations = [];
for (const row of observations) {
  assert.equal(row.compilerSha256, compilerSha256); assert.equal(row.repeatBuilds, 2);
  const expected = reference.cases[row.index]; assert.equal(row.id, expected.id);
  assert.deepEqual(row.files, graph.resolve(input, input.cases[row.index], parents).files);
  for (const [name, artifact] of Object.entries(expected.maps)) {
    if (artifact.kind !== 'javascript') continue;
    try {
      const code = row.outputs[artifact.output], raw = name.endsWith('#inline')
        ? Buffer.from(code.split('\n').find(line => line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1], 'base64').toString('utf8') : row.outputs[name];
      const native = stream(ts, reference.blobs[expected.outputs[artifact.output]], reference.blobs[artifact.raw]);
      const actual = stream(ts, code, raw); assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
      check(native, actual, Object.values(row.files));
      mutations.push({ id: row.id, controls: negativeControls(native, actual, Object.values(row.files)) });
      verified.push({ index: row.index, id: row.id, map: name, actualJavaScriptSha256: graph.sha256(code), actualMapSha256: graph.sha256(raw),
        nativeJavaScriptSha256: expected.outputs[artifact.output], nativeMapSha256: artifact.raw, allCharactersClassified: true });
    } catch (error) { failures.push({ id: row.id, map: name, error: error.message }); }
  }
}
const report = { configurations: observations.length, maps: verified.length + failures.length, compilerSha256, repeatBuilds: 2, verified, mutations, failures };
if (!failures.length) assert.equal(mutations.reduce((sum, row) => sum + row.controls.length, 0), 84);
if (process.env.BLUEICE_GE5_CONTINUATION_ROLE_REPORT) fs.writeFileSync(process.env.BLUEICE_GE5_CONTINUATION_ROLE_REPORT, JSON.stringify(report, null, 2) + '\n');
if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
  fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, { recursive: true });
  fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'continuations.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify(report, null, 2)); if (failures.length) process.exitCode = 1;
