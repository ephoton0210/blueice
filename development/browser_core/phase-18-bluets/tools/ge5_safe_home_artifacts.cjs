// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Comment and publication inventories derive from the source and the exact owned helper assets.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
function compare(ts,n,a,options){
  function comments(text){const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,text),result=[];for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if(kind===ts.SyntaxKind.SingleLineCommentTrivia||kind===ts.SyntaxKind.MultiLineCommentTrivia)result.push({text:scanner.getTokenText(),start:scanner.getTokenPos(),end:scanner.getTextPos()});return result;}
  const original=comments(n.cap.original.text),texts=new Set(original.map(c=>c.text));
  const asset=name=>comments(fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets',name),'utf8')).map(c=>c.text);
  const asyncCount=options.target==='ES2015'?n.cap.cls.members.filter(m=>ts.isMethodDeclaration(m)&&m.modifiers?.some(x=>x.kind===ts.SyntaxKind.AsyncKeyword)).length:0;
  const expected=options.target==='ES2015'?[...Array.from({length:asyncCount},()=>asset('async_helpers.v1.js')).flat(),...asset('helpers.v1.js'),'/* bluets-class-helper-v1 */']:[];
  let nativeOriginal;
  const footers=[];
  for(const[p,generated]of[[n,[]],[a,expected]]){const all=p.view.comments(),foot=all.filter(c=>c.text.startsWith('//# sourceMappingURL='));assert.equal(foot.length,1,'one exact complete safe-home map publication');footers.push(foot[0]);const payload=foot[0].text.slice('//# sourceMappingURL='.length);if(payload.startsWith('data:application/json;base64,'))assert.equal(Buffer.from(payload.split('base64,')[1],'base64').toString(),p.view.raw,'inline footer carries this entire observed map');const originals=all.filter(c=>texts.has(c.text));assert.deepEqual(originals.map(c=>c.text),original.map(c=>c.text),'complete original comment inventory');if(p===n)nativeOriginal=originals;else for(let index=0;index<originals.length;index++)assert.deepEqual(a.view.owner(originals[index].end),n.view.owner(nativeOriginal[index].end),'exact surviving original comment logical end');assert.deepEqual(all.filter(c=>c!==foot[0]&&!texts.has(c.text)).map(c=>c.text),generated,'complete source-derived safe-home helper comment inventory');}
  if(!footers[0].text.includes('base64,'))assert.equal(footers[1].text,footers[0].text,'exact external safe-home map publication');
  return {originalComments:original.length,generatedComments:expected.length};
}
module.exports={compare};
