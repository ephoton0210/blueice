// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// API controls use the same complete physical proof as the actual ES5 contract.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../../../..'), graph = require('./ge5_inputs.cjs'), member = require('./ge5_async_super_member_roles.cjs');
const {stream} = require('./ge5_role_stream.cjs'), authority = require('./ge5_native_authority.cjs'), machine = require('./ge5_continuation_machine.cjs');
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')), reference = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const rows = JSON.parse(fs.readFileSync(0, 'utf8')), row = rows.find(row => row.options.target === 'ES5'); assert(row);
const expected = reference.cases[row.index], mapName = Object.keys(expected.maps).find(name => expected.maps[name].kind === 'javascript'), map = expected.maps[mapName];
const code = row.outputs[map.output], raw = mapName.endsWith('#inline') ? Buffer.from(code.split('base64,')[1].trim(), 'base64').toString() : row.outputs[mapName];
const native = stream(ts, reference.blobs[expected.outputs[map.output]], authority.raw(reference, expected, mapName)), actual = stream(ts, code, raw);
const original = graph.resolve(input, input.cases[row.index], graph.parents(root, input)), source = ts.createSourceFile('original.ts', original.files['main.ts'], ts.ScriptTarget.Latest, true);
function read(view, blue) {
  const env = require('./ge5_module_envelope.cjs').read(ts, view, row.options, {blue, originals: [source], dependencies: []});
  return require('./ge5_async_super_method_ir.cjs').read(ts, view, row.options, source, blue, env);
}
const n = read(native, false), a = read(actual, true), proof = member.prove(ts, native, n, actual, a, row.options);
const nm = machine.read(ts, native, n.callback, 'native', [source.text], {superMember: proof}), am = machine.read(ts, actual, a.callback, 'blue', [source.text], {superMember: proof});
member.paired(nm.superMember, am.superMember); assert.deepEqual(member.entry(am.superMember, a.superCall), native.owner(n.superMember.getStart(native.source)));
const controls = [];
function reject(label, action) {assert.throws(action, undefined, label); controls.push(label);}
reject('copied descriptor cannot forge a physical proof', () => member.bind(nm, {...proof}));
reject('wrong independent view', () => member.bind({...nm, view: Object.create(native)}, proof));
reject('outer method cannot replace callback', () => member.bind({...nm, node: n.method}, proof));
reject('source must belong to the actual machine', () => member.bind({...am, originals: []}, proof));
reject('dispatcher must own the exact original super expression', () => member.bind({...am, callback: n.callback}, proof));
reject('bound capability cannot be forged', () => member.entry({...am.superMember}, a.superCall));
reject('Native binding cannot replace actual proof', () => member.paired(nm.superMember, nm.superMember));
reject('Native and actual sides cannot be reversed', () => member.paired(am.superMember, nm.superMember));
const nativePath = machine.paths(ts, nm)[0], actualPath = machine.paths(ts, am)[0];
const phase = trace => trace.reads.find(read => read.value.kind === 'super-member');
assert.deepEqual([phase(nativePath).yielded, phase(nativePath).resumed], [1, 1]);
assert.deepEqual([phase(actualPath).yielded, phase(actualPath).resumed], [1, 1]);
console.log(JSON.stringify({scope: 'exact physically proved super-member capability API', compilerSha256: row.compilerSha256,
  positive: 1, rejected: controls.length, controls, superLookupAfterAwait: true}, null, 2));
