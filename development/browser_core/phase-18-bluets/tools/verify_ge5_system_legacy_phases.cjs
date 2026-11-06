// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every queried Native tuple is in bounds; unrelated upstream EOF issues are preserved.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),maps=require('./ge5_maps.cjs'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{chain}=require('./ge5_system_legacy_chain.cjs'),rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,320);
const vm=require('node:vm'),sha=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
function execute(code) {
  let registration;const exports={};
  vm.runInNewContext(code,{System:{register(dependencies,factory){assert.deepEqual(Array.from(dependencies),[]);assert.equal(registration,undefined);registration=factory;}}},{timeout:1000});
  assert.equal(typeof registration,'function');const module=registration((name,value)=>{if(typeof name==='object')Object.assign(exports,name);else exports[name]=value;return value;},{id:'main.js'});assert.equal(module.setters.length,0);module.execute();assert.equal(typeof exports.C,'function');return new exports.C(2).method(40);
}
function nativeView(code,raw,original) {
  const source=ts.createSourceFile('emitted.js',code,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS),decoded=maps.decode(raw,code,{recordInvalidCoordinates:true}),widths=original.split('\n').map(line=>line.length);
  return {code,raw,source,decoded,owner(offset){const p=source.getLineAndCharacterOfPosition(offset);let found;for(const point of decoded.segments)if(point[0]===p.line&&point[1]<=p.character)found=point;const owner=found?.length>2?found.slice(2):null;if(owner)assert(owner[0]===0&&owner[1]<widths.length&&owner[2]<=widths[owner[1]],'queried exact Native chain tuple is in original UTF-16 bounds');return owner;}};
}
function compare(n,a) {
  const nn=chain(ts,n),an=chain(ts,a),pairs=[];
  for(const label of new Set([...Object.keys(nn.nodes),...Object.keys(an.nodes)])) {
    let x=nn.nodes[label],y=an.nodes[label];if(!x&&label.startsWith('callee grouping'))x=nn.nodes['factory function'];if(!y&&label.startsWith('callee grouping'))y=an.nodes['factory function'];if(!x&&label.startsWith('initializer grouping'))x=nn.nodes.initializer;if(!y&&label.startsWith('initializer grouping'))y=an.nodes.initializer;assert(x&&y,'paired exact System construction layer '+label);
    assert.deepEqual(a.owner(y.end),n.owner(x.end),'complete System legacy phase chain '+label);pairs.push({label,native:x,actual:y});
  }
  const units=[];
  function unit(label,x,y) {assert.deepEqual(a.owner(y),n.owner(x),label);units.push({label,actual:y});}
  const nb=nn.nodes['binding declaration'],ab=an.nodes['binding declaration'],name=nn.nodes['binding name'],actualName=an.nodes['binding name'];assert(ts.isBinaryExpression(nb)&&ts.isBinaryExpression(ab));assert.equal(actualName.text,name.text);
  for(let i=0;i<name.text.length;i++)unit('complete System binding identifier UTF-16',name.getStart(n.source)+i,actualName.getStart(a.source)+i);
  unit('complete System binding assignment',nb.operatorToken.getStart(n.source),ab.operatorToken.getStart(a.source));
  function gap(label,x1,x2,y1,y2) {
    const text=n.code.slice(x1,x2),annotation='/** @class */',comment=text.indexOf(annotation);let begin=-1,end=-1;
    if(comment>=0){assert(nn.nodes['class factory call'],'Native class annotation belongs to this exact factory');begin=x1+comment;end=begin+annotation.length;assert.equal(text.lastIndexOf(annotation),comment);const owner=n.owner(nn.nodes['factory function'].getStart(n.source));for(let x=begin;x<end;x++)assert.deepEqual(n.owner(x),owner,'Native class annotation retains its exact factory role');}
    assert(/^\s*$/.test(comment<0?text:text.slice(0,comment)+text.slice(comment+annotation.length))&&/^\s*$/.test(a.code.slice(y1,y2)));assert(x1<x2&&y1<y2);const wanted=n.owner(x1);
    for(let x=x1;x<x2;x++)if(x<begin||x>=end)assert.deepEqual(n.owner(x),wanted,label+' exact Native generated gap phase');
    for(let y=y1;y<y2;y++){assert.deepEqual(a.owner(y),wanted,label);units.push({label,actual:y});}
  }
  gap('complete System binding name-to-assignment gap',name.end,nb.operatorToken.getStart(n.source),actualName.end,ab.operatorToken.getStart(a.source));
  gap('complete System binding assignment-to-value gap',nb.operatorToken.end,nn.nodes.initializer.getStart(n.source),ab.operatorToken.end,an.nodes.initializer.getStart(a.source));
  if(nn.nodes['class factory call']) {
    const nc=nn.nodes['class factory call'],ac=an.nodes['class factory call'];assert.equal(nc.arguments.length,0);assert.equal(ac.arguments.length,0);
    const first=nn.nodes['factory body'].end,actualFirst=an.nodes['factory body'].end,last=nn.nodes['outer statement'].end,actualLast=an.nodes['outer statement'].end,wanted=n.owner(nc.end);
    unit('complete generated factory closing token',first-1,actualFirst-1);
    assert(/^[();\s]+$/.test(n.code.slice(first,last))&&/^[();\s]+$/.test(a.code.slice(actualFirst,actualLast)),'only transparent grouping/invocation and statement punctuation remains');
    for(let x=first;x<last;x++)assert.deepEqual(n.owner(x),wanted,'complete Native factory invocation UTF-16 phase');
    for(let y=actualFirst;y<actualLast;y++){assert.deepEqual(a.owner(y),wanted,'complete factory invocation UTF-16 phase');units.push({label:'complete generated factory invocation UTF-16',actual:y});}
  }
  return {pairs,units};
}
let controls=0,codeControls=0,points=0,boundaryUnits=0,upstreamEofIssues=0;const verified=[];
for(const row of rows) {
  const output=ts.transpileModule(row.source,{fileName:'main.ts',compilerOptions:{target:ts.ScriptTarget[row.target],module:ts.ModuleKind.System,experimentalDecorators:true,emitDecoratorMetadata:row.metadata,useDefineForClassFields:row.define,sourceMap:true,inlineSources:true}}),n=nativeView(output.outputText,output.sourceMapText,row.source),a=stream(ts,row.code,row.raw);upstreamEofIssues+=n.decoded.coordinateIssues.length;
  compare(nativeView(output.outputText,output.sourceMapText,row.source),stream(ts,row.code,row.raw));
  const proof=compare(n,a),pairs=proof.pairs;points+=pairs.length;boundaryUnits+=proof.units.length;const seen=new Set();
  for(const pair of [...pairs.map(pair=>({label:pair.label,offset:pair.actual.end})),...proof.units.map(unit=>({label:unit.label,offset:unit.actual}))]) {
    const offset=pair.offset;if(seen.has(offset))continue;seen.add(offset);
    for(const owner of[null,[0,0,1]])if(JSON.stringify(a.owner(offset))!==JSON.stringify(owner)) {
      const changed=mutate(ts,a,offset,owner);assert.throws(()=>compare(nativeView(output.outputText,output.sourceMapText,row.source),changed),undefined,'effective exact '+pair.label);controls++;
    }
  }
  assert.equal(execute(row.code),execute(output.outputText));
  const body=pairs.find(pair=>pair.label==='method body 0').actual,returned=body.statements.find(ts.isReturnStatement);assert(ts.isIdentifier(returned.expression)&&returned.expression.text==='value');
  const changedCode=row.code.slice(0,returned.expression.getStart(a.source))+'41   '+row.code.slice(returned.expression.end),changed=stream(ts,changedCode,row.raw);assert.equal(changed.source.parseDiagnostics.length,0,'effective callable control is valid JavaScript');
  compare(nativeView(output.outputText,output.sourceMapText,row.source),changed);
  assert.throws(()=>assert.equal(execute(changed.code),execute(output.outputText)),undefined,'effective original callable return control');codeControls++;
  verified.push({decorators:row.decorators,constructor:row.constructor,target:row.target,metadata:row.metadata,define:row.define,astral:row.astral,sourceSha256:sha(row.source),nativeJavaScriptSha256:sha(output.outputText),nativeMapSha256:sha(output.sourceMapText),actualJavaScriptSha256:sha(row.code),actualMapSha256:sha(row.raw),pairs:pairs.map(pair=>({label:pair.label,nativeKind:ts.SyntaxKind[pair.native.kind],actualKind:ts.SyntaxKind[pair.actual.kind],nativeEnd:pair.native.end,actualEnd:pair.actual.end,owner:n.owner(pair.native.end)}))});
}
const report={configurations:rows.length,completeChainEndpoints:points,boundaryUnits,controls,codeControls,executions:rows.length*2,unchangedFreshChecks:rows.length,recordedUnmodifiedNativeEofIssues:upstreamEofIssues,passed:true,verified};
if(process.env.BLUEICE_GE5_SYSTEM_PHASE_REPORT)fs.writeFileSync(process.env.BLUEICE_GE5_SYSTEM_PHASE_REPORT,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({...report,verified:verified.length}));
