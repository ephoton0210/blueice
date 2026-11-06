// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test'), assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const root = path.resolve(__dirname, '../../../..'), ts = require(root + '/backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE);
const ir = require('./ge5_safe_home_ir.cjs'), graph = require('./ge5_inputs.cjs'), authority = require('./ge5_native_authority.cjs'), {stream} = require('./ge5_role_stream.cjs');
const input = require(root + '/backend/bluets/tests/fixtures/ge5_source_maps/cases.json'), reference = require(root + '/backend/bluets/tests/fixtures/ge5_source_maps/reference.json'), parents = graph.parents(root, input);
for (const family of ['private-super-method', 'private-super-accessors', 'private-static-super', 'private-super-async-generator']) test('original safe-home identities reject unsafe lexical super: ' + family, () => {
  const index = input.cases.findIndex(row => row.form === 'ge3_decorator_compositions/' + family + '-safe-home-object'), original = graph.resolve(input, input.cases[index], parents), cap = ir.source(ts, original.files['main.ts']);
  assert(cap.members.length);
  const changed = original.files['main.ts'].replace(/Reflect\.get\(Object\.getPrototypeOf\(C(?:\.prototype)?\), 'method', this\)/, 'super.method');
  if (changed !== original.files['main.ts']) assert.throws(() => ir.source(ts, changed), /unsafe lowered lexical super/);
  const row = reference.cases[index], name = Object.keys(row.maps).find(name => row.maps[name].kind === 'javascript'), native = stream(ts, reference.blobs[row.outputs[row.maps[name].output]], authority.raw(reference, row, name));
  const parsed = ir.read(ts, native, cap, false); assert.equal(parsed.entries.length, cap.members.length);
});
const observationsFile = process.env.BLUEICE_GE5_SAFE_HOME_OBSERVATIONS;
test('public private callable header carries its independently derived Native source role', {skip: !observationsFile}, () => {
  const rows = JSON.parse(fs.readFileSync(observationsFile, 'utf8'));
  for (const row of rows) {
    const cap = ir.source(ts, row.files['main.ts']), expected = reference.cases[row.index];
    const name = Object.keys(expected.maps).find(name => expected.maps[name].kind === 'javascript');
    const raw = name.endsWith('#inline') ? Buffer.from(row.outputs['out/main.js'].split('base64,')[1], 'base64').toString() : row.outputs[name];
    const n = ir.read(ts, stream(ts, reference.blobs[expected.outputs['out/main.js']], authority.raw(reference, expected, name)), cap, false);
    const a = ir.read(ts, stream(ts, row.outputs['out/main.js'], raw), cap, true);
    const classHeader = p => {const tokens = p.view.tokens(p.core), open = tokens.findIndex(t => t.text === '{'); return tokens.slice(0, open + 1).map(t => [t.text, t.owners, p.view.owner(t.end)]);};
    assert.deepEqual(classHeader(a), classHeader(n), 'complete private derived class header '+row.index);
    for (const member of n.core.members.filter(m => (ts.isGetAccessorDeclaration(m) || ts.isSetAccessorDeclaration(m)) && ts.isPrivateIdentifier(m.name))) {
      const observed = a.core.members.filter(m => m.kind === member.kind && m.name?.text === member.name.text); assert.equal(observed.length, 1);
      const signature = (p, m) => p.view.tokens(m).filter(t => t.end <= m.body.getStart(p.view.source) + 1).map(t => [t.text, t.owners, p.view.owner(t.end)]);
      assert.deepEqual(signature(a, observed[0]), signature(n, member), 'complete generated private dispatcher header '+row.index+'/'+member.name.text);
    }
    for (let index = 0; index < cap.members.length; index++) {
      const native = n.view.tokens(n.entries[index].fn).find(token => token.text === 'function');
      const actual = a.view.tokens(a.entries[index].fn).find(token => token.text === 'function');
      assert.deepEqual(actual.owners, native.owners, 'exact private callable function header '+row.index+'/'+index);
      if (!(row.options.target === 'ES2015' && cap.members[index].node.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword))) {
        const headers = (p, entry) => p.view.tokens(entry.fn).filter(t => t.end <= entry.fn.body.getStart(p.view.source) + 1 && !(entry.fn.name && entry.fn.name.getStart(p.view.source) <= t.start && t.end <= entry.fn.name.end));
        assert.deepEqual(headers(a, a.entries[index]).map(t => [t.text, t.owners]), headers(n, n.entries[index]).map(t => [t.text, t.owners]), 'complete source private callable signature '+row.index+'/'+index);
      }
    }
  }
});

test('all frozen safe-home modules reject fresh hidden grants, comment additions and driver endpoints', {skip: !observationsFile}, () => {
  const {mutate, encode} = require('./ge5_role_mutations.cjs'), {coherent} = require('./ge5_decorator_preserved_mutations.cjs');
  const check = require('./ge5_safe_home_module.cjs').compare;
  for (const row of JSON.parse(fs.readFileSync(observationsFile, 'utf8'))) {
    const expected = reference.cases[row.index], name = Object.keys(expected.maps).find(name => expected.maps[name].kind === 'javascript');
    const raw = name.endsWith('#inline') ? Buffer.from(row.outputs['out/main.js'].split('base64,')[1], 'base64').toString() : row.outputs[name];
    const ncode = reference.blobs[expected.outputs['out/main.js']], nraw = authority.raw(reference, expected, name);
    const native = stream(ts, ncode, nraw), actual = stream(ts, row.outputs['out/main.js'], raw);
    const proof = check(ts, native, actual, row.options, row.files['main.ts']);
    function rejects(changed, label) {
      const value = coherent(ts, changed);
      assert.throws(() => check(ts, stream(ts, ncode, nraw), stream(ts, value.code, value.raw), row.options, row.files['main.ts']), undefined, label + '/' + row.index);
    }
    rejects(stream(ts, actual.code + '\n/* unproved comment */\n', actual.raw), 'complete generated comment inventory');
    const footer = actual.comments().find(comment => comment.text.startsWith('//# sourceMappingURL='));
    const pos = actual.source.getLineAndCharacterOfPosition(footer.start + 4), segments = actual.decoded.segments.map(segment => [...segment]);
    let at = segments.findIndex(segment => segment[0] > pos.line || segment[0] === pos.line && segment[1] >= pos.character); if (at < 0) at = segments.length;
    segments.splice(at, 0, [pos.line, pos.character, 0, 0, 0], [pos.line, pos.character]);
    rejects(stream(ts, actual.code, JSON.stringify({...JSON.parse(actual.raw), mappings: encode(segments)})), 'hidden zero-width source grant');
    for (const point of proof.endings.unique.filter(point => point.kind === 'AsyncDriverReturn')) rejects(mutate(ts, actual, point.actual, [0, 0, 0]), 'independent async driver endpoint');
    for (const comment of actual.comments().filter(comment => comment.text.startsWith('/* G-E5'))) rejects(mutate(ts, actual, comment.end, [0, 0, 0]), 'surviving original comment end');
  }
});
