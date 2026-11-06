// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ordinary AST-bound regression for preserved decorated class-expression keywords.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs');
const root=path.resolve(__dirname,'../../../..'),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,16);
function keyword(view) {
  const expressions=[];function visit(node){if(ts.isClassExpression(node)&&(ts.getDecorators(node)||[]).length)expressions.push(node);ts.forEachChild(node,visit);}visit(view.source);
  assert.equal(expressions.length,1,'complete preserved decorated expression inventory');
  const node=expressions[0],keyword=node.getChildren(view.source).find(child=>child.kind===ts.SyntaxKind.ClassKeyword);assert(keyword);return keyword;
}
for(const row of rows) {
  const expected=reference.cases[row.index],native=stream(ts,reference.blobs[expected.outputs['out/main.js']],authority.raw(reference,expected,'out/main.js.map')),actual=stream(ts,row.outputs['out/main.js'],row.outputs['out/main.js.map']);
  const n=keyword(native),a=keyword(actual);assert.equal(n.end-n.getStart(native.source),5);assert.equal(a.end-a.getStart(actual.source),5);
  for(let unit=0;unit<5;unit++){assert.equal(native.owner(n.getStart(native.source)+unit),null,'Native class keyword is generated');assert.equal(actual.owner(a.getStart(actual.source)+unit),null,'preserved expression class keyword UTF-16 unit '+unit);}
  assert.deepEqual(actual.owner(a.end),native.owner(n.end),'complete class keyword endpoint');
  const nb=n.parent.getChildren(native.source).find(child=>child.kind===ts.SyntaxKind.OpenBraceToken),ab=a.parent.getChildren(actual.source).find(child=>child.kind===ts.SyntaxKind.OpenBraceToken);assert(nb&&ab);
  assert.deepEqual(actual.owner(ab.getStart(actual.source)),native.owner(nb.getStart(native.source)),'complete retained class-expression body opening');assert.deepEqual(actual.owner(ab.end),native.owner(nb.end),'complete retained class-expression body opening endpoint');
  if(n.parent.heritageClauses?.length){const ne=n.parent.heritageClauses[0].types[0],ae=a.parent.heritageClauses[0].types[0];assert.deepEqual(actual.owner(ae.end),native.owner(ne.end),'complete heritage expression logical endpoint');}
}
console.log(JSON.stringify({ordinaryConfigurations:16,repeatBuilds:2,astBoundKeywordInteriorsAndEndpoint:true}));
