// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{stream,compareTokens}=require('./ge5_role_stream.cjs'),authority=require('./ge5_native_authority.cjs');
const root=path.resolve(__dirname,'../../../..'),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,32);
function prefix(view){const matches=[];function visit(node){if((ts.isArrowFunction(node)||ts.isFunctionExpression(node))&&ts.isBlock(node.body)){const first=node.body.statements[0];if(first&&ts.isVariableStatement(first)){const d=first.declarationList.declarations[0];if(ts.isIdentifier(d.name)&&d.name.text.startsWith('_classDecorators')&&d.initializer&&ts.isArrayLiteralExpression(d.initializer)&&d.initializer.elements[0]?.getText(view.source)==='replace'){assert(node.parent&&ts.isParenthesizedExpression(node.parent)&&ts.isCallExpression(node.parent.parent));const open=node.body.getChildren(view.source).find(child=>child.kind===ts.SyntaxKind.OpenBraceToken);matches.push(view.tokens(node.parent.parent).filter(token=>token.end<=open.end));}}}ts.forEachChild(node,visit);}visit(view.source);assert.equal(matches.length,1,'one source decorated class-value evaluation factory');return matches[0];}
for(const row of rows){const e=reference.cases[row.index],n=stream(ts,reference.blobs[e.outputs['out/main.js']],authority.raw(reference,e,'out/main.js.map')),a=stream(ts,row.outputs['out/main.js'],row.outputs['out/main.js.map']);compareTokens(n,prefix(n),a,prefix(a),'complete decorated expression evaluation-factory prefix');}
console.log(JSON.stringify({ordinaryConfigurations:rows.length,repeatBuilds:2,allFourFamiliesAndBothLoweredTargets:true}));
