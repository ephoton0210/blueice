// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test'), assert = require('node:assert/strict'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const {stream, compareTokens} = require('./ge5_role_stream.cjs');
const {compareClass, readClass} = require('./ge5_class_roles.cjs');
const {create, coversComment} = require('./ge5_class_method_holes.cjs');
function fixture(target = ts.ScriptTarget.ES2022, useDefineForClassFields = false) {
  const text = 'class Box { async read(n: number) { return await Promise.resolve(n); } other() { return 2; } }\n';
  const source = ts.createSourceFile('original.ts', text, ts.ScriptTarget.Latest, true), box = source.statements[0];
  const output = ts.transpileModule(text, {fileName: 'original.ts', compilerOptions: {target, useDefineForClassFields, sourceMap: true, inlineSources: true}});
  const native = stream(ts, output.outputText, output.sourceMapText), actual = stream(ts, output.outputText, output.sourceMapText);
  const node = view => view.source.statements.find(node => ts.isClassDeclaration(node)
    || ts.isVariableStatement(node) && node.declarationList.declarations.some(declaration => declaration.name.getText(view.source) === 'Box'));
  const nn = node(native), an = node(actual), n = readClass(ts, native, nn, box), a = readClass(ts, actual, an, box);
  const method = item => item.native ? item.node.members[0] : item.methods[0].fn;
  return {native, actual, nn, an, n, a, box, pair: {source: box.members[0], native: method(n), actual: method(a)}};
}
function prove(f) {
  compareTokens(f.native, f.native.tokens(f.pair.native), f.actual, f.actual.tokens(f.pair.actual), 'complete independently verified async method');
}
function delegate(f, pairs = [f.pair]) {return create(ts, f.native, f.actual, f.n, f.a, f.box, pairs);}
test('a completely proved retained method composes with strict surrounding class syntax', () => {
  const f = fixture(); prove(f);
  const proof = compareClass(ts, f.native, f.nn, f.actual, f.an, f.box, {classRoles: {pairedCallables: [f.pair]}});
  assert.equal(proof.delegatedCallables.items.length, 1);
});
test('unproved and partial methods cannot delegate any source role', () => {
  const f = fixture(); assert.throws(() => delegate(f), /every UTF-16/);
  f.native.cover(f.pair.native.getStart(f.native.source), f.pair.native.end);
  f.actual.cover(f.pair.actual.getStart(f.actual.source), f.pair.actual.end - 1);
  assert.throws(() => delegate(f), /every UTF-16/);
});
test('original method identity, async modifier and one proof per method are mandatory', () => {
  const f = fixture(); prove(f);
  assert.throws(() => delegate(f, [f.pair, f.pair]), /one delegated/);
  assert.throws(() => delegate(f, [{...f.pair, source: f.box.members[1]}]), /original non-generator async/);
  assert.throws(() => delegate(f, [{...f.pair, source: {...f.pair.source}}]), /index >= 0/);
});
test('exact emitted AST identity cannot be replaced by a sibling or an offset record', () => {
  const f = fixture(); prove(f);
  assert.throws(() => delegate(f, [{...f.pair, actual: f.an.members[1]}]), /actual emitted method identity/);
  assert.throws(() => delegate(f, [{...f.pair, actual: {start: f.pair.actual.pos, end: f.pair.actual.end}}]), /actual emitted method identity/);
});
test('projection accepts the complete method subtree and rejects a partial token slice', () => {
  const f = fixture(); prove(f); const proof = delegate(f), tokens = f.actual.tokens(f.pair.actual);
  assert.equal(proof.project(f.actual, tokens).length, 1);
  assert.throws(() => proof.project(f.actual, tokens.slice(1)), /complete syntax subtree/);
  assert.throws(() => proof.project(f.actual, tokens.slice(0, -1)), /complete syntax subtree/);
  const nn = proof.project(f.native, f.native.tokens(f.pair.native))[0], an = proof.project(f.actual, tokens)[0];
  assert.throws(() => proof.tokensPaired(nn, {...an, methodHole: {...an.methodHole}}), /same exact method proof/);
});
test('an ES5 method proof is tied to the exact original class prototype member', () => {
  const f = fixture(ts.ScriptTarget.ES5); prove(f); assert(delegate(f).paired(f.pair.native, f.pair.actual));
  assert.throws(() => delegate(f, [{...f.pair, actual: f.a.methods[1].fn}]), /actual emitted method identity/);
});
test('generated suspension comments require the exact typed proof, view and scanned comment', () => {
  const f = fixture(ts.ScriptTarget.ES5); prove(f); const proof = delegate(f);
  const comment = f.native.comments().find(comment => f.pair.native.getStart(f.native.source) <= comment.start && comment.end <= f.pair.native.end);
  assert(comment); assert(coversComment(proof, f.native, comment));
  assert.throws(() => coversComment({...proof}, f.native, comment), /exact typed/);
  assert.throws(() => coversComment(proof, f.native, {...comment}), /exact scanned/);
  assert.throws(() => coversComment(proof, fixture().native, comment), /exact proved method view/);
});

// Reordering switch clauses preserves dispatch when every clause returns.
// Copy each UTF-16 character and its owner together, then prove those exact
// regions independently before delegating the enclosing method subtree.
function reorderedDescriptor() {
  const f = fixture(ts.ScriptTarget.ES5, true), switches = [];
  function visit(node) {if (ts.isSwitchStatement(node)) switches.push(node); ts.forEachChild(node, visit);}
  visit(f.pair.native);
  assert.equal(switches.length, 1);
  const clauses = [...switches[0].caseBlock.clauses]; assert.equal(clauses.length, 2);
  for (const clause of clauses) assert(ts.isReturnStatement(clause.statements.at(-1)));
  const first = clauses[0].getStart(f.native.source), middle = clauses[1].getStart(f.native.source), last = clauses[1].end;
  const offsets = Array.from({length: f.native.code.length}, (_, index) => index);
  offsets.splice(first, last - first, ...offsets.slice(middle, last), ...offsets.slice(first, middle));
  const code = offsets.map(offset => f.native.code[offset]).join(''), file = ts.createSourceFile('moved.js', code, 99, true);
  const segments = offsets.map((offset, at) => {
    const {line, character} = file.getLineAndCharacterOfPosition(at);
    return [line, character, ...(f.native.owner(offset) || [])];
  });
  const {encode} = require('./ge5_role_mutations.cjs');
  f.actual = stream(ts, code, JSON.stringify({...JSON.parse(f.native.raw), mappings: encode(segments)}));
  f.an = f.actual.source.statements.find(node => ts.isVariableStatement(node)
    && node.declarationList.declarations.some(declaration => declaration.name.getText(f.actual.source) === 'Box'));
  f.a = readClass(ts, f.actual, f.an, f.box); f.pair.actual = f.a.methods[0].fn;
  function proveRegions() {
    const start = f.pair.actual.getStart(f.actual.source), end = f.pair.actual.end;
    for (let at = start; at < end; at++) {
      assert.equal(f.actual.code[at], f.native.code[offsets[at]], 'exact copied method character');
      assert.deepEqual(f.actual.owner(at), f.native.owner(offsets[at]), 'exact copied method source role');
    }
    assert.deepEqual(f.actual.owner(end), f.native.owner(f.pair.native.end), 'independently proved method endpoint');
    f.native.cover(f.pair.native.getStart(f.native.source), f.pair.native.end); f.actual.cover(start, end);
  }
  return {...f, proveRegions};
}
test('descriptor transport composes a proved method whose state clauses have a different physical order', () => {
  const f = reorderedDescriptor(); f.proveRegions();
  const proof = compareClass(ts, f.native, f.nn, f.actual, f.an, f.box, {classRoles: {pairedCallables: [f.pair]}});
  assert.equal(proof.delegatedCallables.items.length, 1);
});
for (const side of ['preceding', 'following']) test('a delegated descriptor method rejects source grants in its ' + side + ' gap', () => {
  const f = reorderedDescriptor(), {mutate} = require('./ge5_role_mutations.cjs');
  const at = side === 'preceding' ? f.pair.actual.getStart(f.actual.source) - 1 : f.pair.actual.end;
  assert.notDeepEqual(f.actual.owner(at), [0, 0, 0]);
  const changed = mutate(ts, f.actual, at, [0, 0, 0]);
  const an = changed.source.statements.find(node => ts.isVariableStatement(node)
    && node.declarationList.declarations.some(declaration => declaration.name.getText(changed.source) === 'Box'));
  const a = readClass(ts, changed, an, f.box), pair = {...f.pair, actual: a.methods[0].fn};
  f.proveRegions();
  compareTokens(f.actual, f.actual.tokens(f.pair.actual), changed, changed.tokens(pair.actual), 'unchanged independently proved method');
  assert.throws(() => compareClass(ts, f.native, f.nn, changed, an, f.box,
    {classRoles: {pairedCallables: [pair]}}), /ordered gap/);
});
