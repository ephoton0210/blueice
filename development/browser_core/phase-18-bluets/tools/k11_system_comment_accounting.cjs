// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict');
const {compareGap}=require("./ge5_role_stream.cjs");
function create(ts,native,actual,originals,{erasedImportComments=[],generatedComments=[]}={}) {
  const erased=erasedImportComments.length?require("./ge5_erased_import_comments.cjs").project(native,actual,originals,erasedImportComments):{source:[],retained:[]};
  actual.nativeStarts??=new Map();actual.nativeEnds??=new Map();
  for(const view of [native,actual]) {
    view.roleEnds??=new Map();view.roleStarts??=new Map();
    const noSource=view.noSource;
    view.noSource=(start,end,label)=>{noSource(start,end,label);view.roleEnds.set(end,view===native?native.owner(end):null);view.roleStarts.set(start,null);};
  }
  // Caller supplies only exact canonical System-helper comment ranges, independently AST-bounded.
  for(const comment of generatedComments){assert.equal(actual.code.slice(comment.start,comment.end),comment.text);actual.noSource(comment.start,comment.end,'exact canonical System asset comment');}
  const excludedGenerated = comment => generatedComments.some(item => item.start === comment.start && item.end === comment.end && item.text === comment.text);
  const nativeTokens=native.tokens(native.source),actualTokens=actual.tokens(actual.source);
  function pair(wanted,observed,label,aliases=new Map()) {
    assert.equal(observed.length,wanted.length,label+' complete token inventory');
    for(let index=0;index<wanted.length;index++) {
      const n=wanted[index],a=observed[index],alias=aliases.get(a.text);
      assert.equal(alias||a.text,n.text,label+' token '+index);
      if(alias) {
        const owner=n.owners[0];assert(n.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),'uniform Native generated binding role');
        assert(a.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),label+' complete fresh-binding interior');
      }else assert.deepEqual(a.owners,n.owners,label+' UTF-16 token '+index+' '+a.text);
      native.cover(n.start,n.end);actual.cover(a.start,a.end);native.roleEnds.set(n.end,native.owner(n.end));actual.roleEnds.set(a.end,native.owner(n.end));native.roleStarts.set(n.start,native.owner(n.start));actual.roleStarts.set(a.start,native.owner(n.start));actual.nativeStarts.set(a.start,n.start);actual.nativeEnds.set(a.end,n.end);
      if(index&&wanted[index-1].end<=n.start&&observed[index-1].end<=a.start) {
        const movedNative=nativeTokens.some(token=>wanted[index-1].end<=token.start&&token.end<=n.start&&native.covered(token.start));
        const movedActual=actualTokens.some(token=>observed[index-1].end<=token.start&&token.end<=a.start&&actual.covered(token.start));
        // Separately proved class bodies and generated ABI may move relative
        // to retained module statements. Their physical gaps are checked by
        // finish() against each semantic region's Native endpoint.
        if(movedNative||movedActual)continue;
        const omitted=[];
        for(let offset=observed[index-1].end;offset<a.start;) {
          if(!actual.covered(offset)){offset++;continue;}
          const start=offset;while(offset<a.start&&actual.covered(offset))offset++;omitted.push([start,offset]);
        }
        compareGap(native,wanted[index-1].end,n.start,actual,observed[index-1].end,a.start,label+' ordered gap '+index,omitted);
      }
    }
  }
  function comments(code) {
    const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,code),items=[];
    for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if(kind===ts.SyntaxKind.SingleLineCommentTrivia||kind===ts.SyntaxKind.MultiLineCommentTrivia)items.push({text:scanner.getTokenText(),start:scanner.getTokenPos(),end:scanner.getTextPos()});return items;
  }
  function closeRegion(view,start,end) {
      const tokens=view.tokens(view.source).filter(token=>start<=token.start&&token.end<=end);for(const token of tokens)for(let offset=token.start;offset<token.end;offset++)assert(view.covered(offset),'unclassified class/module token at '+offset+' '+token.text);
      const ends=[...view.roleEnds].sort((a,b)=>a[0]-b[0]),starts=[...view.roleStarts].sort((a,b)=>a[0]-b[0]);let cursor=0,expected=null,followingCursor=0,enteredFollowing=false,gapLimit=0,gapBreak=false;
      for(let offset=start;offset<end;offset++) {
        while(cursor<ends.length&&ends[cursor][0]<=offset){expected=ends[cursor][1];cursor++;}
        while(followingCursor<starts.length&&starts[followingCursor][0]<=offset)followingCursor++;
        if(view.covered(offset)){enteredFollowing=false;continue;}assert(/\s/.test(view.code[offset]),'unclassified non-gap character '+offset);
        const following=starts[followingCursor]?.[1],owner=view.owner(offset);
        const atEnd=JSON.stringify(owner)===JSON.stringify(expected),atStart=following!==undefined&&JSON.stringify(owner)===JSON.stringify(following);
        if(offset>=gapLimit){gapLimit=offset;while(gapLimit<end&&!view.covered(gapLimit)&&/\s/.test(view.code[gapLimit]))gapLimit++;gapBreak=/[\r\n]/.test(view.code.slice(offset,gapLimit));}
        const nativeEnd=view===actual?actual.nativeEnds.get(ends[cursor-1]?.[0]):undefined,nativeStart=view===actual?actual.nativeStarts.get(starts[followingCursor]?.[0]):undefined;
        const nativeGap=Number.isInteger(nativeEnd)&&Number.isInteger(nativeStart)&&nativeEnd<=nativeStart&&(/[\r\n]/.test(native.code.slice(nativeEnd,nativeStart))||Array.from({length:nativeStart-nativeEnd},(_,index)=>native.owner(nativeEnd+index)).some(owner=>owner===null));
        const generatedGap=owner===null&&(view===native||expected===null||following===null||gapBreak||nativeGap);
        assert(generatedGap||atEnd&&!enteredFollowing||atStart,(view===native?'Native':'Blue')+' class/module ordered terminal gap at '+offset+': '+JSON.stringify(owner)+' expected '+JSON.stringify(expected)+' then '+JSON.stringify(following));if(owner!==null&&atStart&&!atEnd)enteredFollowing=true;view.cover(offset,offset+1);
      }
  }
  function region(nn,an) {
    for(const [view,node]of [[native,nn],[actual,an]]) {
      assert(node&&typeof node.getStart==='function'&&node.getSourceFile()===view.source,'exact emitted class-value expression syntax');
      assert(ts.isClassExpression(node)||ts.isCallExpression(node)||ts.isBinaryExpression(node)||ts.isParenthesizedExpression(node),'closed class-value expression syntax kind');
      closeRegion(view,node.getStart(view.source),node.end);
    }
    assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete class-value expression logical endpoint');
    return {native:nn,actual:an,completeUTF16:true};
  }
  function finish(classProofs=[]) {
    const original=originals.flatMap(code=>comments(code).filter(comment=>!erased.source.some(item=>item.original===code&&item.start===comment.start&&item.end===comment.end&&item.text===comment.text))),texts=new Set(original.map(item=>item.text));
    for(const comment of erased.retained)actual.noSource(comment.start,comment.end,'exact proved erased-import attached comment');
    const nc=comments(native.code),ac=comments(actual.code),wanted=nc.filter(c=>texts.has(c.text)),observed=ac.filter(c=>texts.has(c.text)&&!excludedGenerated(c)&&!erased.retained.some(item=>item.start===c.start&&item.end===c.end));
    assert.deepEqual(wanted.map(c=>c.text),original.map(c=>c.text),'Native complete original comment inventory');assert.deepEqual(observed.map(c=>c.text),wanted.map(c=>c.text),'complete original comment inventory');
    for(let i=0;i<wanted.length;i++) {
      const n=wanted[i],a=observed[i];assert.equal(a.text,n.text);
      for(let j=0;j<n.end-n.start;j++)assert.deepEqual(actual.owner(a.start+j),native.owner(n.start+j),'original comment UTF-16 interior');
      native.cover(n.start,n.end);actual.cover(a.start,a.end);native.roleEnds.set(n.end,native.owner(n.end));actual.roleEnds.set(a.end,native.owner(n.end));native.roleStarts.set(n.start,native.owner(n.start));actual.roleStarts.set(a.start,native.owner(n.start));
      const nt=native.tokens(native.source).find(token=>token.start>=n.end),at=actual.tokens(actual.source).find(token=>token.start>=a.end);
      if(nt&&at)compareGap(native,n.end,nt.start,actual,a.end,at.start,'complete original comment ending gap');
    }
    for(const view of [native,actual])for(const comment of view===native?nc:ac)if(!texts.has(comment.text)||view===actual&&excludedGenerated(comment)) {
      const sourceClass=view===native&&comment.text==='/** @class */'&&classProofs.find(proof=>!proof.native.native&&proof.native.node.getStart(native.source)<=comment.start&&comment.end<=proof.native.factory.getStart(native.source));
      if(sourceClass) {
        const owner=native.owner(sourceClass.native.factory.getStart(native.source));
        for(let offset=comment.start;offset<comment.end;offset++)assert.deepEqual(native.owner(offset),owner,'Native class marker retains complete class wrapper role');
        native.cover(comment.start,comment.end);native.roleEnds.set(comment.end,owner);
      }else {
        const scanned=view.comments().find(item=>item.start===comment.start&&item.end===comment.end);
        const delegated=classProofs.some(proof=>proof.delegatedExpressions&&require("./ge5_class_field_holes.cjs").coversComment(proof.delegatedExpressions,view,scanned)
          ||proof.delegatedCallables&&require("./ge5_class_method_holes.cjs").coversComment(proof.delegatedCallables,view,scanned));
        if(delegated){view.roleStarts.set(comment.start,view.owner(comment.start));view.roleEnds.set(comment.end,view.owner(comment.end));}
        else view.noSource(comment.start,comment.end,'generated class/module comment');
      }
    }
    for(const view of [native,actual]) {
      closeRegion(view,0,view.code.length);
      view.complete('complete class/module UTF-16 roles and gaps');
    }
    assert.deepEqual(actual.owner(actual.code.length),native.owner(native.code.length),'complete module final endpoint');
  }
  return {pair,finish,region};
}
module.exports={create};
