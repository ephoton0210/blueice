// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Shared physical-role accounting for independently parsed module families.
const assert=require('node:assert/strict');
const abi=require('./ge5_module_abi.cjs');
const {compareTokens,compareGap}=require('./ge5_role_stream.cjs');
function create(ts,native,actual,options,originals,profile) {
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
  if(native.code===actual.code)assert.deepEqual(actual.decoded,native.decoded,'identical output requires every decoded segment');
  const n=abi.read(ts,native,options,false,originals,profile), a=abi.read(ts,actual,options,true,originals,profile);
  assert.equal(a.format,n.format);assert.equal(a.user.length,n.user.length);
  const ranges=new Map([[native,[]],[actual,[]]]);
  function cover(view,start,end,trailing) {view.cover(start,end);ranges.get(view).push({start,end,trailing});}
  function uniform(view,start,end,owner,label) {
    for(let i=start;i<end;i++)assert.deepEqual(view.owner(i),owner,label+' at '+i);
    cover(view,start,end,owner);
  }
  function pair(wanted,observed,label) {
    compareTokens(native,wanted,actual,observed,label);
    for(let index=0;index<wanted.length;index++) {
      const x=wanted[index],y=observed[index],trailing=native.owner(x.end);
      cover(native,x.start,x.end,trailing);cover(actual,y.start,y.end,trailing);
    }
  }
  function boundary(view,node,trailing) {ranges.get(view).push({start:node.end,end:node.end,trailing});}
  for(const item of [n,a])for(const node of item.generated)item.view.noSource(node.getStart(item.view.source),node.end,'generated module statement');
  function binding(item,index,wanted,exported=false) {
    const statement=item.user[index], view=item.view;
    if(ts.isVariableStatement(statement)) {
      assert.equal(statement.declarationList.declarations.length,1);const declaration=statement.declarationList.declarations[0];
      assert.equal(declaration.name.getText(),wanted);return {statement,expression:declaration.initializer,name:declaration.name};
    }
    assert(ts.isExpressionStatement(statement));let assignment=statement.expression, publication=null;
    if(exported&&item.format==='system'&&!item.system.stem) {
      publication=assignment;assert(ts.isCallExpression(publication)&&publication.arguments.length===2);
      assert.equal(publication.expression.getText(),item.system.publisher);assert(ts.isStringLiteral(publication.arguments[0])&&publication.arguments[0].text===wanted);assignment=publication.arguments[1];
    }
    assert(ts.isBinaryExpression(assignment)&&assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken);
    const left=assignment.left;
    if(publication||!exported)assert(ts.isIdentifier(left)&&left.text===wanted);
    else assert(ts.isPropertyAccessExpression(left)&&left.expression.getText()==='exports'&&left.name.text===wanted);
    return {statement,expression:assignment.right,name:publication||!exported?left:left.name,assignment,publication};
  }
  function prefix(nb,ab) {
    if(nb.publication) {
      const start=native.owner(nb.publication.getStart(native.source));assert(start);
      uniform(native,nb.publication.getStart(native.source),nb.name.getStart(native.source),start,'Native publication transport');
      uniform(actual,ab.assignment.left.getStart(actual.source),ab.name.getStart(actual.source),start,'System publication transport');
      pair([native.token(nb.name),native.token(nb.assignment.operatorToken)],[actual.token(ab.name),actual.token(ab.assignment.operatorToken)],'System published binding');
    } else {
      const prefix=(view,item)=>view.tokens(item.statement).filter(token=>token.end<=item.expression.getStart(view.source));
      pair(prefix(native,nb),prefix(actual,ab),'published binding prefix');
    }
  }
  function suffix(nb,ab) {
    const endpoint=native.owner(nb.expression.end),tail=native.owner(nb.statement.end);
    uniform(native,nb.expression.end,nb.statement.end,endpoint,'Native published statement endpoint');
    uniform(actual,ab.expression.end,ab.statement.end,endpoint,'published statement endpoint');
    assert.deepEqual(actual.owner(ab.statement.end),tail,'published statement end');
    boundary(native,nb.statement,tail);boundary(actual,ab.statement,tail);
  }
  function finish() {
    function comments(code) {
      const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,code),result=[];
      for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if([ts.SyntaxKind.SingleLineCommentTrivia,ts.SyntaxKind.MultiLineCommentTrivia].includes(kind))result.push({text:scanner.getTokenText(),start:scanner.getTokenPos(),end:scanner.getTextPos()});
      return result;
    }
    const originalComments=comments(originals[0]);
    const originalText=new Set(originalComments.map(comment=>comment.text));
    const nc=comments(native.code).filter(comment=>originalText.has(comment.text)),ac=comments(actual.code).filter(comment=>originalText.has(comment.text));
    for(const view of [native,actual])for(const comment of comments(view.code))if(!originalText.has(comment.text))view.noSource(comment.start,comment.end,'generated module comment');
    assert.equal(nc.length,originalComments.length);assert.equal(ac.length,nc.length);
    assert.deepEqual(ac.map(comment=>comment.text),nc.map(comment=>comment.text),'complete original comment inventory');
    compareGap(native,n.user[0].getFullStart(),n.user[0].getStart(native.source),actual,a.user[0].getFullStart(),a.user[0].getStart(actual.source),'original leading module trivia');
    for(const view of [native,actual]) {
      const roles=ranges.get(view).sort((x,y)=>x.end-y.end||x.start-y.start);
      for(let offset=0;offset<view.code.length;offset++) {
        if(view.covered(offset))continue;
        const owner=view.owner(offset),previous=roles.findLast(role=>role.end<=offset);
        assert(owner===null||/\s/.test(view.code[offset])&&previous&&JSON.stringify(owner)===JSON.stringify(previous.trailing),(view===native?'Native':'Blue')+' unclassified module source at '+offset+': '+JSON.stringify(owner)+': '+JSON.stringify(view.code.slice(offset-15,offset+25)));
        view.cover(offset,offset+1);
      }
      view.complete('complete module ABI and original statements');
    }
    assert.deepEqual(actual.owner(actual.code.length),native.owner(native.code.length));
  }
  return {n,a,cover,uniform,pair,boundary,binding,prefix,suffix,finish};
}
module.exports={create};
