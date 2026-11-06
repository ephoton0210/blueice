// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Only a proved erased original constructor import can lose its attached source comment.
const assert=require('node:assert/strict'),graphs=require('./ge5_metadata_class_graph.cjs'),proofs=new WeakMap();
function create(ts,native,actual,graph) {
  assert(graphs.valid(graph),'exact original type-only dependency graph proof');assert.equal(graph.runtime,false,'runtime constructor dependency cannot erase attached comments');
  const original=graph.original,imported=graph.imported;assert.equal(imported.getSourceFile(),original);assert(original.statements.includes(imported));
  for(const view of[native,actual])if(view.decoded.metadata.sourcesContent)assert(view.decoded.metadata.sourcesContent.includes(original.text),'comment proof uses this map product original source');
  const comments=(ts.getLeadingCommentRanges(original.text,imported.getFullStart())??[]).filter(comment=>comment.pos>=imported.getFullStart()&&comment.end<=imported.getStart(original));
  const items=[];
  for(const source of comments) {
    const text=original.text.slice(source.pos,source.end);assert(!native.comments().some(comment=>comment.text===text),'Native must actually omit this exact attached import comment');
    const matches=actual.comments().filter(comment=>comment.text===text);assert(matches.length<=1,'one exact retained attached comment');
    for(const comment of matches) {
      const first=actual.tokens(actual.source)[0];
      const prior=actual.tokens(actual.source).filter(token=>token.end<=comment.start);
      // CommonJS generated prologue tokens may precede the comment; its next
      // lexical token must be the first surviving original statement.
      const following=actual.tokens(actual.source).find(token=>token.start>=comment.end),nativeFirst=native.tokens(native.source).find(token=>native.owner(token.start)!==null);assert(following&&nativeFirst&&following.text===nativeFirst.text,'retained erased-import comment stays at its exact source entry phase');
      assert(prior.every(token=>actual.owner(token.start)===null),'retained erased-import comment cannot move after source evaluation');
      assert(first);for(let offset=comment.start;offset<comment.end;offset++)assert.equal(actual.owner(offset),null,'retained erased import comment has no source grant');assert.equal(actual.owner(comment.end),null,'retained erased import comment endpoint has no source grant');
    }
    items.push({source:{start:source.pos,end:source.end,text},actual:matches[0]??null});
  }
  const proof={};proofs.set(proof,{native,actual,original:original.text,imported,items});return proof;
}
function project(native,actual,originals,supplied=[]) {
  const source=[],retained=[];
  for(const proof of supplied) {
    const value=proofs.get(proof);assert(value&&value.native===native&&value.actual===actual,'typed erased-import comment proof belongs to these exact decoded views');assert(originals.includes(value.original),'exact original input source');
    for(const item of value.items){source.push({...item.source,original:value.original});if(item.actual)retained.push(item.actual);}
  }
  return {source,retained};
}
module.exports={create,project};
