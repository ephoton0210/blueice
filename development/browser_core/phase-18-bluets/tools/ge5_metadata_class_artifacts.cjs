// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original and generated comments, publications and zero-width map entries are separate products.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
function comments(ts,text) {const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,text),items=[];for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if(kind===ts.SyntaxKind.SingleLineCommentTrivia||kind===ts.SyntaxKind.MultiLineCommentTrivia)items.push({text:scanner.getTokenText(),start:scanner.getTokenPos(),end:scanner.getTextPos()});return items;}
function compare(ts,native,actual,options,originalText,erasedImportComments=[]) {
  let previous=null;for(const point of actual.decoded.segments){assert(!previous||point[0]>previous[0]||point[0]===previous[0]&&point[1]>=previous[1],'ordered decoded map entries');previous=point;}
  // Zero-width transitions are accepted only with the same ordered Native owners
  // at that exact generated coordinate; additional or retargeted grants reject.
  function duplicates(view){const groups=[];for(const point of view.decoded.segments){const last=groups.at(-1);if(last&&last[0][0]===point[0]&&last[0][1]===point[1])last.push(point);else groups.push([point]);}return groups.filter(group=>group.length>1);}
  const expectedDuplicates=duplicates(native),actualDuplicates=duplicates(actual),expectedByCoordinate=new Map(expectedDuplicates.map(group=>[JSON.stringify(group[0].slice(0,2)),group]));
  for(const group of actualDuplicates)assert.deepEqual(group,expectedByCoordinate.get(JSON.stringify(group[0].slice(0,2))),'exact ordered Native duplicate-coordinate owners');
  if(actual.code.replace(/\n$/,'')===native.code.replace(/\n$/,''))assert.deepEqual(actualDuplicates,expectedDuplicates,'complete Native duplicate-coordinate inventory for identical generated text');
  const source=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true),original=comments(ts,originalText),erased=require('./ge5_erased_import_comments.cjs').project(native,actual,[originalText],erasedImportComments);
  const sourceNative=original.filter(comment=>!erased.source.some(item=>item.start===comment.start&&item.end===comment.end)),sourceActual=original.filter(comment=>!erased.source.some(item=>item.start===comment.start&&item.end===comment.end)||erased.retained.some(item=>item.text===comment.text));
  let count=0;function classes(node){if(ts.isInterfaceDeclaration(node)||ts.isTypeAliasDeclaration(node))return;if(ts.isClassDeclaration(node)||ts.isClassExpression(node))count++;ts.forEachChild(node,classes);}classes(source);
  const asset=comments(ts,fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/class_helpers.v1.js'),'utf8')).map(comment=>comment.text);
  const stock=require('./ge5_class_roles.cjs').nativeFactories(ts,actual,[source]).length;assert(stock<=count,'Native factory inventory is bounded by original runtime classes');
  const publications=[],surviving=[];
  for(const[view,blue]of[[native,false],[actual,true]]) {
    const all=comments(ts,view.code),publication=all.filter(comment=>comment.text.startsWith('//# sourceMappingURL='));assert.equal(publication.length,1,'one complete JavaScript map publication');publications.push(publication[0]);
    const sourceComments=all.filter(comment=>original.some(item=>item.text===comment.text));assert.deepEqual(sourceComments.map(item=>item.text),(blue?sourceActual:sourceNative).map(item=>item.text),'complete original attached/surviving comment inventory');
    surviving.push(sourceComments.filter(comment=>!blue||!erased.retained.some(item=>item.start===comment.start&&item.end===comment.end&&item.text===comment.text)));
    const generated=all.filter(comment=>!sourceComments.includes(comment)&&comment!==publication[0]).map(comment=>comment.text),expected=options.target==='ES5'?blue?[...Array.from({length:count-stock},()=>asset).flat(),...Array(stock).fill('/** @class */')]:Array(count).fill('/** @class */'):[];
    assert.deepEqual(generated,expected,'complete original-AST class/helper generated comment inventory');
    const payload=publication[0].text.slice('//# sourceMappingURL='.length);
    if(payload.startsWith('data:application/json;base64,'))assert.equal(Buffer.from(payload.split('base64,')[1],'base64').toString('utf8'),view.raw,'inline publication embeds this complete decoded map');
  }
  assert.deepEqual(actual.owner(publications[1].end),native.owner(publications[0].end),'exact map-publication logical endpoint');
  assert.equal(surviving[1].length,surviving[0].length,'complete surviving original comment endpoint inventory');
  for(let index=0;index<surviving[0].length;index++){const n=surviving[0][index],a=surviving[1][index];assert.equal(a.text,n.text);assert.deepEqual(actual.owner(a.end),native.owner(n.end),'exact surviving original comment logical endpoint');}
  if(!publications[0].text.includes('base64,'))assert.equal(publications[1].text,publications[0].text,'exact external map publication identity');
  return {sourceNative,sourceActual,generatedClasses:count,publications};
}
module.exports={compare,comments};
