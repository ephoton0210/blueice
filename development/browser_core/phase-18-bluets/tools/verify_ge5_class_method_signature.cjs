// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// API units exercise class ancestry and exact, preproved relocated headers.
const assert = require('node:assert/strict'), path = require('node:path'), signature = require('./ge5_relocated_callable_signature.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
function parse(text) {return ts.createSourceFile('unit.ts', text, ts.ScriptTarget.Latest, true);}
function find(source, predicate) {const result = []; function visit(node) {if (predicate(node)) result.push(node); ts.forEachChild(node, visit);} visit(source); return result;}
const original = parse('class Box{async read(extra:number){return this.value+await Promise.resolve(extra);}}');
const left = parse('class Box{read(extra){return awaiter(this,void 0,void 0,function(){return generator(this,function(state){return [2,extra];});});}}');
const right = parse('class Box{read(slot){return(function entry(argument){return driver(this,arguments,function(extra){return generator(this,1,function(state){return {kind:"return",value:extra};});});}).apply(this,arguments);}}');
const source = find(original, ts.isMethodDeclaration)[0], nativeOuter = find(left, ts.isMethodDeclaration)[0], actualOuter = find(right, ts.isMethodDeclaration)[0];
const nativeCallback = find(left, ts.isFunctionExpression)[0], actualCallback = find(right, ts.isFunctionExpression)[1];
function view(source) {const covered = new Set(); return {source, owner: () => [0, 0, 0], covered: offset => covered.has(offset), cover(from, to) {for (let offset = from; offset < to; offset++) covered.add(offset);}};}
const native = view(left), actual = view(right), nm = {view: native, node: nativeCallback, originals: [original.text]}, am = {view: actual, node: actualCallback, originals: [original.text]};
const ranges = [[nativeCallback.getStart(left), nativeCallback.body.getStart(left) + 1], [actualCallback.getStart(right), actualCallback.body.getStart(right) + 1]];
const cap = {kind: 'verified-relocated-callable-signature-v1', sourceKind: 'async-class-method', source, nativeOuter, actualOuter,
  nativeCallback, actualCallback, ranges, formalPairs: [{source: source.parameters[0], native: nativeOuter.parameters[0], actual: actualCallback.parameters[0]}]};
native.cover(...ranges[0]); actual.cover(...ranges[1]); signature.verify(ts, native, nm, actual, am, cap);
const controls = [];
function reject(label, changed, n = nm, a = am, av = actual) {assert.throws(() => signature.verify(ts, native, n, av, a, changed), undefined, label); controls.push(label);}
reject('class method cannot claim object ancestry', {...cap, sourceKind: 'async-object-method'});
reject('class method cannot claim field-arrow ancestry', {...cap, sourceKind: 'class-field-async-arrow'});
reject('wrong exact actual callback', {...cap, actualCallback: actualOuter});
reject('partial callback header', {...cap, ranges: [ranges[0], [ranges[1][0] + 1, ranges[1][1]]]});
reject('generated outer slot is not the relocated formal', {...cap, formalPairs: [{...cap.formalPairs[0], actual: actualOuter.parameters[0]}]});
reject('source must belong to both independent machines', cap, nm, {...am, originals: []});
for (const [label, text] of [
  ['object ancestry cannot claim class', 'const object={async read(extra:number){return extra;}};'],
  ['non-async source method', 'class Box{read(extra:number){return extra;}}'],
  ['async generator is not an async method', 'class Box{async *read(extra:number){yield extra;}}'],
]) {
  const replacement = parse(text), method = find(replacement, ts.isMethodDeclaration)[0];
  reject(label, {...cap, source: method, formalPairs: [{...cap.formalPairs[0], source: method.parameters[0]}]}, {...nm, originals: [replacement.text]}, {...am, originals: [replacement.text]});
}
const unproved = {...actual, covered: offset => offset === ranges[1][0] + 1 ? false : actual.covered(offset)};
reject('every header interior must already be proved', cap, nm, {...am, view: unproved}, unproved);
console.log(JSON.stringify({scope: 'class method relocated signature capability API unit', positive: 1, rejected: controls.length, controls}, null, 2));
