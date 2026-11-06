// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete source callable trees compose only with already proved class-value expressions.
const assert=require('node:assert/strict'),{compareGap}=require('./ge5_role_stream.cjs'),rest=require('./ge5_decorator_rest.cjs');
function compare(ts,native,nf,actual,af,original,options,{pairedExpressions=[],extraNativeNodes=[]}={}) {
  assert(ts.isFunctionDeclaration(original)&&ts.isFunctionDeclaration(nf)&&ts.isFunctionDeclaration(af));assert.equal(nf.name.text,original.name.text);assert.equal(af.name.text,original.name.text);
  const sourceFile=original.getSourceFile();
  function inside(node,root){for(let cursor=node;cursor;cursor=cursor.parent)if(cursor===root)return true;return false;}
  function complete(view,node){assert.equal(node.getSourceFile(),view.source,'same emitted callable syntax tree');for(let offset=node.getStart(view.source);offset<node.end;offset++)assert(view.covered(offset),'callable delegates only complete already-proved UTF-16 regions');}
  for(const pair of pairedExpressions) {
    assert(ts.isClassExpression(pair.source)&&inside(pair.source,original)&&pair.source.getSourceFile()===sourceFile,'typed class-value source belongs to this callable');
    assert(inside(pair.native,nf)&&inside(pair.actual,af),'paired emitted expressions belong to the same original callable');complete(native,pair.native);complete(actual,pair.actual);assert.deepEqual(actual.owner(pair.actual.end),native.owner(pair.native.end),'delegated class-value endpoint');
  }
  for(const node of extraNativeNodes){assert(ts.isVariableStatement(node)&&node.parent===nf.body,'class-value transport is a direct callable variable declaration');complete(native,node);}
  function callables(root,side) {
    const result=[];function visit(node){if(pairedExpressions.some(pair=>pair[side]===node))return;if(node!==root&&ts.isClassLike(node))return;if(ts.isFunctionExpression(node)||ts.isArrowFunction(node))result.push(node);ts.forEachChild(node,visit);}visit(root);return result;
  }
  const sources=[];function originals(node){if(pairedExpressions.some(pair=>pair.source===node)||ts.isClassLike(node))return;if(ts.isFunctionExpression(node)||ts.isArrowFunction(node))sources.push(node);ts.forEachChild(node,originals);}originals(original);
  const nn=callables(nf,'native'),aa=callables(af,'actual');assert.equal(nn.length,sources.length,'complete original Native callable inventory');assert.equal(aa.length,sources.length,'complete original callable inventory');
  const sourceNames=new Set();function names(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,names);}names(sourceFile);
  function catches(root,side) {const found=[];function visit(node){if(pairedExpressions.some(pair=>pair[side]===node)||node!==root&&ts.isClassLike(node))return;if(ts.isCatchClause(node))found.push(node);ts.forEachChild(node,visit);}visit(root);return found;}
  const sourceCatches=catches(original,'source'),nativeCatches=catches(nf,'native'),actualCatches=catches(af,'actual'),catchReferences={native:new Map(),actual:new Map()};
  assert.equal(nativeCatches.length,sourceCatches.length,'complete source catch binding inventory');assert.equal(actualCatches.length,sourceCatches.length,'complete actual catch binding inventory');
  for(let index=0;index<sourceCatches.length;index++) {
    const source=sourceCatches[index];assert(source.variableDeclaration&&ts.isIdentifier(source.variableDeclaration.name),'closed original catch binding');
    function references(clause) {assert(clause.variableDeclaration&&ts.isIdentifier(clause.variableDeclaration.name));const name=clause.variableDeclaration.name.text,result=[];function visit(node){if(ts.isIdentifier(node)&&node.text===name&&!(ts.isPropertyAccessExpression(node.parent)&&node.parent.name===node))result.push(node);ts.forEachChild(node,visit);}visit(clause);return result;}
    const originalReferences=references(source),nn=references(nativeCatches[index]),aa=references(actualCatches[index]);
    assert.equal(nativeCatches[index].variableDeclaration.name.text,source.variableDeclaration.name.text,'Native source catch binding identity');
    const alias=actualCatches[index].variableDeclaration.name.text;assert(alias===source.variableDeclaration.name.text||!sourceNames.has(alias),'lowered catch binding is fresh in the original program');
    assert.equal(nn.length,originalReferences.length,'complete Native catch binding references');assert.equal(aa.length,nn.length,'complete lowered catch binding references');
    for(const [side,view,refs]of [['native',native,nn],['actual',actual,aa]])for(const node of refs)catchReferences[side].set(node.getStart(view.source),{end:node.end,text:'$source.catch.'+index});
  }
  const prologues={native:[],actual:[]};
  const rootParameters=original.parameters.filter(parameter=>!(ts.isIdentifier(parameter.name)&&parameter.name.text==='this'));
  if(options.target==='ES5'&&rootParameters.some(parameter=>parameter.dotDotDotToken)) {
    assert.equal(rootParameters.length,1,'closed sole effective root rest parameter');
    prologues.native.push({fn:nf,...rest.projectCallableRest(ts,native,nf,original)});
    prologues.actual.push({fn:af,...rest.projectCallableRest(ts,actual,af,original)});
  }
  for(let index=0;index<sources.length;index++) {
    const source=sources[index],n=nn[index],a=aa[index],arrow=ts.isArrowFunction(source),effective=source.parameters.filter(parameter=>!(ts.isIdentifier(parameter.name)&&parameter.name.text==='this'));
    if(options.target==='ES5')assert(ts.isFunctionExpression(n)&&ts.isFunctionExpression(a),'lowered source callable kind');else assert.equal(ts.isArrowFunction(n)&&ts.isArrowFunction(a),arrow,'retained original callable kind');
    assert(!n.name&&!a.name&&!n.asteriskToken&&!a.asteriskToken,'source anonymous callable identity');
    const restParameter=effective.find(parameter=>parameter.dotDotDotToken);
    if(restParameter&&options.target==='ES5') {
      assert.equal(effective.length,1,'closed zero-position rest callable');assert.equal(n.parameters.length,0);assert.equal(a.parameters.length,0);
      prologues.native.push({fn:n,...rest.projectCallableRest(ts,native,n,source)});prologues.actual.push({fn:a,...rest.projectCallableRest(ts,actual,a,source)});
    }else {assert.equal(n.parameters.length,effective.length);assert.equal(a.parameters.length,effective.length);}
  }
  function tokens(view,fn,side) {
    const projected=[];
    for(let token of view.tokens(fn)) {
      if(side==='native'&&extraNativeNodes.some(node=>node.getStart(view.source)<=token.start&&token.end<=node.end))continue;
      const hole=pairedExpressions.find(pair=>pair[side].getStart(view.source)<=token.start&&token.end<=pair[side].end);
      if(hole){if(projected.at(-1)?.hole!==hole)projected.push({text:'$source.class.expression.'+pairedExpressions.indexOf(hole),start:hole[side].getStart(view.source),end:hole[side].end,hole});continue;}
      const catchReference=catchReferences[side].get(token.start);
      if(catchReference){assert.equal(token.end,catchReference.end);const owner=token.owners[0];assert(token.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),'complete catch binding UTF-16 source role');token={...token,text:catchReference.text,owners:[owner]};}
      const scoped=prologues[side].filter(item=>item.fn.getStart(view.source)<=token.start&&token.end<=item.fn.end&&token.text===item.prologue.index.name.text).sort((left,right)=>(left.fn.end-left.fn.getStart(view.source))-(right.fn.end-right.fn.getStart(view.source)))[0];
      if(scoped){const owner=token.owners[0];assert(token.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),'complete fresh rest-index UTF-16 role');token={...token,text:'$source.rest.index.'+prologues[side].indexOf(scoped),owners:[owner]};}
      projected.push(token);
    }
    return projected;
  }
  const wanted=tokens(native,nf,'native'),observed=tokens(actual,af,'actual');assert.equal(observed.length,wanted.length,'complete source callable token inventory');
  for(let index=0;index<wanted.length;index++) {
    const n=wanted[index],a=observed[index];assert.equal(a.text,n.text,'complete source callable token '+index);
    if(n.hole||a.hole)assert(n.hole&&n.hole===a.hole,'same typed class-value delegate');else {assert.deepEqual(a.owners,n.owners,'complete source callable UTF-16 role '+index+' '+n.text);native.cover(n.start,n.end);actual.cover(a.start,a.end);}
    (native.roleStarts??=new Map()).set(n.start,native.owner(n.start));(actual.roleStarts??=new Map()).set(a.start,native.owner(n.start));(native.roleEnds??=new Map()).set(n.end,native.owner(n.end));(actual.roleEnds??=new Map()).set(a.end,native.owner(n.end));(actual.nativeStarts??=new Map()).set(a.start,n.start);(actual.nativeEnds??=new Map()).set(a.end,n.end);
    if(index)compareGap(native,wanted[index-1].end,n.start,actual,observed[index-1].end,a.start,'complete source callable ordered gap '+index);
  }
  assert.deepEqual(actual.owner(af.end),native.owner(nf.end),'complete source callable endpoint');return {original,native:nf,actual:af,prologues,pairedExpressions,allTokenInteriorsAndGaps:true};
}
module.exports={compare};
