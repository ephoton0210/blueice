// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original and generated comments, publications and zero-width map entries are separate products.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
function comments(ts,text) {const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,text),items=[];for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if(kind===ts.SyntaxKind.SingleLineCommentTrivia||kind===ts.SyntaxKind.MultiLineCommentTrivia)items.push({text:scanner.getTokenText(),start:scanner.getTokenPos(),end:scanner.getTextPos()});return items;}
function compare(ts,native,actual,options,originalText,erasedImportComments=[]) {
  let previous=null;for(const point of actual.decoded.segments){assert(!previous||point[0]>previous[0]||point[0]===previous[0]&&point[1]>previous[1],'strict ordered unique decoded map entries');previous=point;}
  const source=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true),original=comments(ts,originalText),erased=require('./ge5_erased_import_comments.cjs').project(native,actual,[originalText],erasedImportComments);
  const sourceNative=original.filter(comment=>!erased.source.some(item=>item.start===comment.start&&item.end===comment.end)),sourceActual=original.filter(comment=>!erased.source.some(item=>item.start===comment.start&&item.end===comment.end)||erased.retained.some(item=>item.text===comment.text));
  let count=0;function classes(node){if(ts.isInterfaceDeclaration(node)||ts.isTypeAliasDeclaration(node))return;if(ts.isClassDeclaration(node)||ts.isClassExpression(node))count++;ts.forEachChild(node,classes);}classes(source);
  const asset=comments(ts,fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/class_helpers.v1.js'),'utf8')).map(comment=>comment.text);
  const publications=[],surviving=[];
  for(const[view,blue]of[[native,false],[actual,true]]) {
    const all=comments(ts,view.code),publication=all.filter(comment=>comment.text.startsWith('//# sourceMappingURL='));assert.equal(publication.length,1,'one complete JavaScript map publication');publications.push(publication[0]);
    const sourceComments=all.filter(comment=>original.some(item=>item.text===comment.text));assert.deepEqual(sourceComments.map(item=>item.text),(blue?sourceActual:sourceNative).map(item=>item.text),'complete original attached/surviving comment inventory');
    surviving.push(sourceComments.filter(comment=>!blue||!erased.retained.some(item=>item.start===comment.start&&item.end===comment.end&&item.text===comment.text)));
    const generated=all.filter(comment=>!sourceComments.includes(comment)&&comment!==publication[0]).map(comment=>comment.text),expected=options.target==='ES5'?blue?Array.from({length:count},()=>asset).flat():Array(count).fill('/** @class */'):[];
    assert.deepEqual(generated,expected,'complete original-AST class/helper generated comment inventory');
    const payload=publication[0].text.slice('//# sourceMappingURL='.length);
    if(payload.startsWith('data:application/json;base64,'))assert.equal(Buffer.from(payload.split('base64,')[1],'base64').toString('utf8'),view.raw,'inline publication embeds this complete decoded map');
  }
  assert.equal(surviving[1].length,surviving[0].length,'complete surviving original comment endpoint inventory');
  for(let index=0;index<surviving[0].length;index++){const n=surviving[0][index],a=surviving[1][index];assert.equal(a.text,n.text);assert.deepEqual(actual.owner(a.end),native.owner(n.end),'exact surviving original comment logical endpoint');}
  if(!publications[0].text.includes('base64,'))assert.equal(publications[1].text,publications[0].text,'exact external map publication identity');
  return {sourceNative,sourceActual,generatedClasses:count,publications};
}
module.exports={compare,comments};
