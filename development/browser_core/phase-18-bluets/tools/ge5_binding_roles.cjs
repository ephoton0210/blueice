// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Compare complete binding token interiors and every physical generated gap.
const assert=require('node:assert/strict');
const {compareTokens,compareGap}=require('./ge5_role_stream.cjs');
function compare(ts,native,actual,options,original) {
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
  if(actual.code===native.code)assert.deepEqual(actual.decoded,native.decoded);
  const source=ts.createSourceFile('main.ts',original,ts.ScriptTarget.Latest,true),names=new Set();
  function visit(node){if(ts.isIdentifier(node))names.add(node.text);ts.forEachChild(node,visit);}visit(source);
  function read(view,blue) {
    const generated=[],body=[];
    for(const node of view.source.statements) {
      if(blue&&ts.isFunctionDeclaration(node)&&!names.has(node.name?.text)||!blue&&ts.isVariableStatement(node)&&node.declarationList.declarations.every(d=>ts.isIdentifier(d.name)&&['__read','__generator'].includes(d.name.text)))generated.push(node);
      else body.push(node);
    }
    const aliases=new Map();let serial=0,parameters=0;
    function scan(node) {
      if(ts.isParameter(node)&&ts.isIdentifier(node.name)&&!names.has(node.name.text)){assert(!aliases.has(node.name.text));aliases.set(node.name.text,'$parameter'+parameters++);}
      if(ts.isVariableDeclaration(node)&&ts.isIdentifier(node.name)&&!names.has(node.name.text)) {
        assert(!aliases.has(node.name.text),'unique lowered binding');aliases.set(node.name.text,'$capture'+serial++);
      }
      ts.forEachChild(node,scan);
    }
    for(const node of body)if(!(options.target==='ES5'&&ts.isFunctionDeclaration(node)&&node.name?.text==='values'))scan(node);
    for(const node of generated) {
      assert(node.name||ts.isVariableStatement(node));
      view.noSource(node.getStart(view.source),node.end,'complete generated binding helper');
      if(ts.isFunctionDeclaration(node)&&node.name.text.includes('binding_read'))aliases.set(node.name.text,'$read');
      if(!blue)aliases.set('__read','$read');
    }
    return {body,generated,aliases};
  }
  const n=read(native,false),a=read(actual,true);
  require('./ge5_binding_helpers.cjs').validate(ts,actual,a.generated,names);
  assert.equal(a.body.length,n.body.length,'complete retained statement inventory');
  assert.equal([...a.aliases.keys()].filter(x=>a.aliases.get(x)!=='$read').length,[...n.aliases.keys()].filter(x=>n.aliases.get(x)!=='$read').length,'complete capture inventory');
  function tokens(view,item,nodes=item.body){return nodes.flatMap(node=>view.tokens(node)).map(token=>{const alias=item.aliases.get(token.text);if(!alias)return token;for(const owner of token.owners)assert.deepEqual(owner,token.owners[0],'complete fresh binding token interior');return {...token,text:alias,owners:Array(alias.length).fill(token.owners[0])};});}
  const nt=tokens(native,n),at=tokens(actual,a),continuations=new Map([[native,[]],[actual,[]]]);
  for(let index=0;index<n.body.length;index++) {const x=n.body[index],y=a.body[index];
    if(options.target==='ES5'&&ts.isFunctionDeclaration(x)&&x.name?.text==='values') {assert(ts.isFunctionDeclaration(y)&&y.name?.text==='values');const machine=require('./ge5_continuation_machine.cjs'),nm=machine.read(ts,native,x,'native',[original],{exceptions:'normal-finally'}),am=machine.read(ts,actual,y,'blue',[original],{exceptions:'normal-finally'});require('./ge5_linear_roles.cjs').compareFunction(ts,native,nm,actual,am);continuations.get(native).push(x);continuations.get(actual).push(y);}
    else compareTokens(native,tokens(native,n,[x]),actual,tokens(actual,a,[y]),'complete destructuring evaluation statement '+index);
    if(ts.isExpressionStatement(x)&&ts.isBinaryExpression(x.expression)&&x.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken
      &&ts.isPropertyAccessExpression(x.expression.left)&&x.expression.left.expression.getText(native.source)==='exports'
      &&ts.isIdentifier(x.expression.right)&&x.expression.left.name.text===x.expression.right.text
      &&source.statements.some(node=>ts.isFunctionDeclaration(node)&&node.name?.text===x.expression.right.text&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword))) {
      assert.deepEqual(actual.owner(y.end),native.owner(x.end),'exact original exported function publication endpoint');
    }
    if(index)compareGap(native,n.body[index-1].end,x.getStart(native.source),actual,a.body[index-1].end,y.getStart(actual.source),'complete inter-statement binding gaps');
  }
  compareGap(native,nt.at(-1).end,native.code.length,actual,at.at(-1).end,actual.code.length,'complete trailing binding gaps');
  const nfirst=nt[0]?.start||0,afirst=at[0]?.start||0;
  compareGap(native,0,nfirst,actual,0,afirst,'leading source trivia',a.generated.map(node=>[node.getStart(actual.source),node.end]));
  const comments=view=>{const s=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,view.code),result=[];for(let k=s.scan();k!==ts.SyntaxKind.EndOfFileToken;k=s.scan())if(k===ts.SyntaxKind.SingleLineCommentTrivia||k===ts.SyntaxKind.MultiLineCommentTrivia)result.push({start:s.getTokenPos(),end:s.getTextPos(),text:s.getTokenText()});return result;};
  const originalComments=comments({code:original}).map(x=>x.text);
  const nc=comments(native).filter(x=>originalComments.includes(x.text)),ac=comments(actual).filter(x=>originalComments.includes(x.text));
  assert.deepEqual(nc.map(x=>x.text),originalComments,'complete Native original comments');assert.deepEqual(ac.map(x=>x.text),originalComments,'complete Blue original comments');
  for(let i=0;i<nc.length;i++){const x=nc[i],y=ac[i];for(let j=0;j<x.end-x.start;j++)assert.deepEqual(actual.owner(y.start+j),native.owner(x.start+j),'original comment UTF-16 owner');native.cover(x.start,x.end);actual.cover(y.start,y.end);}
  for(const view of [native,actual]) {
    for(const comment of comments(view))if(!originalComments.includes(comment.text)){const fn=continuations.get(view).find(node=>node.getStart(view.source)<=comment.start&&comment.end<=node.end);if(fn){for(let offset=comment.start;offset<comment.end;offset++)assert(view.covered(offset),'opcode comment belongs to the complete continuation contract');}else view.noSource(comment.start,comment.end,'generated binding comment');}
    for(let offset=0;offset<view.code.length;offset++)if(!view.covered(offset)) {
      assert(/\s/.test(view.code[offset])||originalComments.some(text=>view.code.slice(offset).startsWith(text)),'unclassified source character '+offset);
      assert.equal(view.owner(offset),null,'unclassified gap source grant '+offset);view.cover(offset,offset+1);
    }
    view.complete('complete binding module');
  }
  return {native:n,actual:a};
}
module.exports={compare};
