// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete module roles compose with a separately closed default guard and core.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),{inspect}=require('./ge5_async_arrow_roles.cjs'),envelope=require('./ge5_module_envelope.cjs'),{compareTokens,compareGap}=require('./ge5_role_stream.cjs'),machine=require('./ge5_continuation_machine.cjs'),linear=require('./ge5_linear_roles.cjs'),guard=require('./ge5_async_arrow_guard.cjs'),frames=require('./ge5_async_arrow_frames.cjs');
function identifiers(ts,source){const result=new Set();function visit(node){if(ts.isIdentifier(node))result.add(node.text);ts.forEachChild(node,visit);}visit(source);return result;}
function read(ts,view,options,original,blue){const item=inspect(ts,view,options,original,blue),env=envelope.read(ts,view,options,{blue,originals:[original]}),generated=[],helpers=[];
  for(const node of env.sourceNodes){if(node===item.declaration.parent.parent||node===view.source.statements.at(-1))continue;
    if(ts.isFunctionDeclaration(node)){assert(blue&&!item.retained);helpers.push(node);}
    else if(ts.isVariableStatement(node)){assert(!blue&&!item.retained&&node.declarationList.declarations.length===1);helpers.push(node);}
    else if(ts.isExpressionStatement(node)){const e=node.expression;if(ts.isStringLiteral(e))env.exact(node,'"use strict";');else if(ts.isCallExpression(e))env.exact(node,'Object.defineProperty(exports,"__esModule",{value:true});');else env.exact(node,'exports.result=void 0;');}
    else assert.fail('unreviewed async arrow module statement');view.noSource(node.getStart(view.source),node.end,'complete generated callable module statement');generated.push(node);
  }
  if(blue&&!item.retained){
    assert.equal(item.entryFunction.parameters.length,options.target==='ES5'?item.arrow.parameters.length:0,'complete versioned callable formal inventory');
    for(const parameter of item.factory.parameters)assert(!parameter.initializer&&!parameter.dotDotDotToken&&!parameter.questionToken&&!parameter.type&&!parameter.modifiers?.length,'plain lexical-capture factory formal');
  }
  const expected=options.target==='ES5'?4:item.retained?0:1;assert.equal(helpers.length,expected,'complete callable helper inventory');
  if(blue&&helpers.length){const generators=helpers.filter(node=>/^__blueice_target_(?:generator|delegate_iterator|delegate_step)_\d+$/.test(node.name?.text));if(options.target==='ES5')require('./ge5_binding_helpers.cjs').validate(ts,view,generators,identifiers(ts,item.source));else assert.equal(generators.length,0);
    const async=helpers.filter(node=>!generators.includes(node));assert.equal(async.length,1);item.helper=async[0];const name=async[0].name.text;assert.match(name,options.target==='ES5'?/^__blueice_target_async_driver_\d+$/:/^__blueice_target_async_\d+$/);env.exact(async[0],fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/async_helpers.v1.js'),'utf8').replaceAll('__blueice_target_async',name));assert.equal(item.driver.expression.getText(view.source),name);
  }else if(helpers.length)assert.deepEqual(helpers.map(node=>node.declarationList.declarations[0].name.text).sort(),(options.target==='ES5'?['__awaiter','__generator','__read','__spreadArray']:['__awaiter']).sort());
  return {...item,env,generated,helpers};
}
function compare(ts,native,actual,options,original){const sourceLines=original.split('\n');for(const view of[native,actual]){assert.equal(view.decoded.metadata.sources.length,1,'complete original callable source table');for(const segment of view.decoded.segments)if(segment.length>2)assert(segment[2]===0&&segment[3]<sourceLines.length&&segment[4]<=sourceLines[segment[3]].length,'every callable source coordinate is within original UTF-16 bounds');}if(!options.inlineSourceMap){const url=view=>/(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(view.code)?.[1];assert.equal(url(actual),url(native),'complete external map publication identity');}if(options.inlineSourceMap){for(const view of[native,actual]){const match=/(?:^|\n)\/\/# sourceMappingURL=data:application\/json;base64,([^\r\n]+)\r?\n?$/.exec(view.code);assert(match,'complete inline publication');assert.equal(Buffer.from(match[1],'base64').toString(),view.raw,'inline publication and consumed raw map agree');}}assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);const n=read(ts,native,options,original,false),a=read(ts,actual,options,original,true),proof={native:n,actual:a},start=(v,node)=>node.getStart(v.source);
  const nd=n.declaration.parent.parent,ad=a.declaration.parent.parent;
  compareTokens(native,native.tokens(nd).filter(t=>t.end<=start(native,n.declaration.initializer)),actual,actual.tokens(ad).filter(t=>t.end<=start(actual,a.declaration.initializer)),'complete callable declaration prefix');
  const np=native.tokens(nd).filter(t=>t.end<=start(native,n.declaration.initializer)),ap=actual.tokens(ad).filter(t=>t.end<=start(actual,a.declaration.initializer));
  compareGap(native,np.at(-1).end,start(native,n.declaration.initializer),actual,ap.at(-1).end,start(actual,a.declaration.initializer),'complete callable initializer boundary');
  compareTokens(native,native.tokens(native.source.statements.at(-1)),actual,actual.tokens(actual.source.statements.at(-1)),'complete callable result publication');
  if(n.retained)compareTokens(native,native.tokens(nd),actual,actual.tokens(ad),'complete retained async arrow');
  else {
    if(options.target==='ES5'){const nm=machine.read(ts,native,n.callback,'native',[original],{bodyStatements:n.callback.body.statements.filter(node=>node!==n.defaultGuard)}),am=machine.read(ts,actual,a.callback,'blue',[original]);proof.guard=guard.read(ts,native,n,actual,a,nm,am,a.env.exact);proof.core=linear.compareFunction(ts,native,proof.guard.nativeMachine,actual,proof.guard.actualMachine);}
    frames.complete(ts,native,n,actual,a);
    compareTokens(native,native.tokens(nd).filter(t=>t.start>=n.declaration.initializer.end),actual,actual.tokens(ad).filter(t=>t.start>=a.declaration.initializer.end),'complete lowered callable declaration ending');
  }
  compareGap(native,0,start(native,nd),actual,0,start(actual,ad),'complete callable module prefix');compareGap(native,nd.end,start(native,native.source.statements.at(-1)),actual,ad.end,start(actual,actual.source.statements.at(-1)),'complete callable declaration/publication boundary');compareGap(native,native.source.statements.at(-1).end,native.code.length,actual,actual.source.statements.at(-1).end,actual.code.length,'complete callable trailing gap');
  const originalComments=[],scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,original);for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if([ts.SyntaxKind.SingleLineCommentTrivia,ts.SyntaxKind.MultiLineCommentTrivia].includes(kind))originalComments.push(scanner.getTokenText());
  for(const[view,item]of[[native,n],[actual,a]]){
    assert.deepEqual(view.comments().filter(comment=>originalComments.includes(comment.text)).map(comment=>comment.text),originalComments,'complete original callable comment inventory');
    for(const comment of view.comments())if(!originalComments.includes(comment.text)){
      if(options.target==='ES5'&&start(view,item.callback)<=comment.start&&comment.end<=item.callback.end){for(let offset=comment.start;offset<comment.end;offset++)assert(view.covered(offset),'complete continuation opcode comment role');}
      else view.noSource(comment.start,comment.end,'complete generated callable comment');
    }
    for(let offset=0;offset<view.code.length;offset++)if(!view.covered(offset)){assert(/\s/.test(view.code[offset]),'unclassified callable syntax at '+offset);assert.equal(view.owner(offset),null,'unclassified callable source grant at '+offset);view.cover(offset,offset+1);}view.complete('complete callable physical source roles and generated gaps');
  }
  assert.deepEqual(actual.owner(actual.code.length),native.owner(native.code.length));return proof;
}
module.exports={read,compare};
