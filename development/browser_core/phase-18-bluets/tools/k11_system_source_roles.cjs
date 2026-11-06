// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original K.11 System source inventory composes with the exact G5 envelope.
const assert=require('node:assert/strict'),fs=require('node:fs'),crypto=require('node:crypto');
const path=require('node:path');
const envelope=require('./ge5_module_envelope.cjs'),accounting=require('./k11_system_comment_accounting.cjs');
const sourceHash='f5be563ed189d1c9ba0f5e231f097af4660533868a7bcd7f1030a7fd68d42b3f';
const sha=text=>crypto.createHash('sha256').update(text).digest('hex');
function comments(ts,code){const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,code),out=[];for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if(kind===ts.SyntaxKind.SingleLineCommentTrivia||kind===ts.SyntaxKind.MultiLineCommentTrivia)out.push({start:scanner.getTokenPos(),end:scanner.getTextPos(),text:scanner.getTokenText()});return out;}
function compare(ts,native,actual,options,originalText){
  assert.equal(sha(originalText),sourceHash,'exact unchanged original K11 System source');
  assert.equal(options.module,'System');assert.equal(options.target,'ES2022');assert.equal(options.experimentalDecorators,false);assert.equal(options.emitDecoratorMetadata,false);
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata,'complete observed System map metadata');
  const original=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true),statements=[...original.statements];
  assert.equal(statements.length,4);const[sourceVariable,sourceFunction,sourceInvocation,sourceConsole]=statements;
  assert(ts.isVariableStatement(sourceVariable)&&sourceVariable.modifiers?.some(m=>m.kind===ts.SyntaxKind.ExportKeyword));
  assert.equal(sourceVariable.declarationList.declarations.length,1);const binding=sourceVariable.declarationList.declarations[0];assert(ts.isIdentifier(binding.name)&&ts.isNumericLiteral(binding.initializer));
  assert(ts.isFunctionDeclaration(sourceFunction)&&sourceFunction.name&&sourceFunction.body&&sourceFunction.modifiers?.some(m=>m.kind===ts.SyntaxKind.ExportKeyword));
  assert.equal(sourceFunction.parameters.length,0);assert.equal(sourceFunction.body.statements.length,1);
  const sourceUpdate=sourceFunction.body.statements[0];assert(ts.isExpressionStatement(sourceUpdate)&&ts.isBinaryExpression(sourceUpdate.expression));
  assert.equal(sourceUpdate.expression.operatorToken.kind,ts.SyntaxKind.PlusEqualsToken);assert(ts.isIdentifier(sourceUpdate.expression.left)&&sourceUpdate.expression.left.text===binding.name.text&&ts.isNumericLiteral(sourceUpdate.expression.right));
  assert(ts.isExpressionStatement(sourceInvocation)&&ts.isCallExpression(sourceInvocation.expression)&&sourceInvocation.expression.expression.getText(original)===sourceFunction.name.text&&sourceInvocation.expression.arguments.length===0);
  assert(ts.isExpressionStatement(sourceConsole)&&ts.isCallExpression(sourceConsole.expression)&&sourceConsole.expression.expression.getText(original)==='console.log'&&sourceConsole.expression.arguments.length===1&&sourceConsole.expression.arguments[0].getText(original)===binding.name.text);
  const n=envelope.read(ts,native,options,{originals:[original],dependencies:[]}),a=envelope.read(ts,actual,options,{blue:true,originals:[original],dependencies:[]});
  assert.equal(n.format,'system');assert.equal(a.format,'system');assert.equal(a.generated.length>=4,true);
  const asset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/system_helpers.v1.js'),'utf8');
  const start=a.generated[2].end,end=a.generated.at(-1).end,generatedComments=comments(ts,actual.code).filter(c=>start<=c.start&&c.end<=end);
  assert.deepEqual(generatedComments.map(c=>c.text),comments(ts,asset).map(c=>c.text),'complete canonical System asset comment inventory');
  const sourceComments=comments(ts,originalText).map(c=>c.text),publicationComment='//# sourceMappingURL='+native.decoded.metadata.file+'.map';
  assert.deepEqual(comments(ts,native.code).map(c=>c.text),[...sourceComments,publicationComment],'complete original and publication Native comment inventory');
  assert.deepEqual(comments(ts,actual.code).map(c=>c.text),[...generatedComments.map(c=>c.text),...sourceComments,publicationComment],'complete canonical/source/publication actual comment inventory');
  const contract=accounting.create(ts,native,actual,[originalText],{generatedComments});n.classifyTransport();a.classifyTransport();
  const pairs=[],uniforms=[],ends=[];
  function one(values,label){assert.equal(values.length,1,label);return values[0];}
  function text(view,node){return node.getText(view.source);}
  function uniform(view,first,last,owner,label){for(let offset=first;offset<last;offset++)assert.deepEqual(view.owner(offset),owner,label+' UTF16 '+offset);view.cover(first,last);view.roleStarts.set(first,owner);view.roleEnds.set(last,owner);uniforms.push({side:view===native?'native':'actual',start:first,end:last,label,owner});}
  function endpoint(nn,an,label){assert.deepEqual(actual.owner(an.end),native.owner(nn.end),label+' exact logical endpoint');native.roleStarts.set(nn.getStart(native.source),native.owner(nn.getStart(native.source)));actual.roleStarts.set(an.getStart(actual.source),native.owner(nn.getStart(native.source)));native.roleEnds.set(nn.end,native.owner(nn.end));actual.roleEnds.set(an.end,native.owner(nn.end));actual.nativeStarts.set(an.getStart(actual.source),nn.getStart(native.source));actual.nativeEnds.set(an.end,nn.end);ends.push({label,native:nn.end,actual:an.end,owner:native.owner(nn.end)});}
  function pair(nn,an,label,aliases){contract.pair(native.tokens(nn),actual.tokens(an),label,aliases);pairs.push({label,native:nn,actual:an});endpoint(nn,an,label);}
  const nf=one(n.sourceNodes.filter(node=>ts.isFunctionDeclaration(node)&&node.name?.text===sourceFunction.name.text),'one original lifted Native function');
  const af=one(a.sourceNodes.filter(node=>ts.isFunctionDeclaration(node)&&node.name?.text===sourceFunction.name.text),'one original lifted actual function');
  assert.equal(nf.parameters.length,0);assert.equal(af.parameters.length,0);assert.equal(nf.body.statements.length,1);assert.equal(af.body.statements.length,1);
  function nativePublication(name){return one(n.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&text(native,node.expression.expression)===n.system.publisher&&node.expression.arguments.length===2&&ts.isStringLiteral(node.expression.arguments[0])&&node.expression.arguments[0].text===name),'one source export publication '+name);}
  const functionPublication=nativePublication(sourceFunction.name.text),fp=functionPublication.expression;assert(ts.isIdentifier(fp.arguments[1])&&fp.arguments[1].text===sourceFunction.name.text);
  const actualFunctionPublication=one(a.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isElementAccessExpression(node.expression.left)&&text(actual,node.expression.left.expression)==='exports'&&ts.isStringLiteral(node.expression.left.argumentExpression)&&node.expression.left.argumentExpression.text===sourceFunction.name.text),'one actual function publication');
  assert.equal(actualFunctionPublication.expression.operatorToken.kind,ts.SyntaxKind.EqualsToken);assert(ts.isIdentifier(actualFunctionPublication.expression.right)&&actualFunctionPublication.expression.right.text===sourceFunction.name.text);
  native.noSource(functionPublication.getStart(native.source),functionPublication.end,'generated original function publication');actual.noSource(actualFunctionPublication.getStart(actual.source),actualFunctionPublication.end,'generated original function publication');endpoint(functionPublication,actualFunctionPublication,'generated function publication');
  const expectedExports=new Set([binding.name.text,sourceFunction.name.text]),slots=a.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&text(actual,node.expression.expression)===a.system.stem+'_slot');
  assert.equal(slots.length,expectedExports.size);const slotNames=new Set();for(const slot of slots){const call=slot.expression;assert.equal(call.arguments.length,1);assert(ts.isStringLiteral(call.arguments[0])&&expectedExports.has(call.arguments[0].text)&&!slotNames.has(call.arguments[0].text));slotNames.add(call.arguments[0].text);a.exact(slot,`${a.system.stem}_slot(${JSON.stringify(call.arguments[0].text)});`);actual.noSource(slot.getStart(actual.source),slot.end,'exact complete original export slot');}
  const refresh=one(a.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&text(actual,node.expression.expression)===a.system.stem+'_refresh'),'one final generated refresh');a.exact(refresh,`${a.system.stem}_refresh();`);actual.noSource(refresh.getStart(actual.source),refresh.end,'exact final generated refresh');
  const strict=one(n.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isStringLiteral(node.expression)),'one Native strict directive');n.exact(strict,"'use strict';");native.noSource(strict.getStart(native.source),strict.end,'Native strict directive');
  const hoist=one(n.sourceNodes.filter(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&node.declarationList.declarations[0].name.getText(native.source)===binding.name.text),'one Native source binding hoist');n.exact(hoist,`var ${binding.name.text};`);native.noSource(hoist.getStart(native.source),hoist.end,'exact source binding hoist');
  const moduleName=one(n.sourceNodes.filter(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&node.declarationList.declarations[0].name.getText(native.source)==='__moduleName'),'one Native module-name transport');n.exact(moduleName,`var __moduleName=${n.system.context}&&${n.system.context}.id;`);native.noSource(moduleName.getStart(native.source),moduleName.end,'exact generated module name');
  function actualVariable(name){return one(a.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isPropertyAccessExpression(node.expression.left)&&text(actual,node.expression.left.expression)==='exports'&&node.expression.left.name.text===name),'one actual variable initialization');}
  const initial=nativePublication(binding.name.text),actualInitial=actualVariable(binding.name.text);
  function publication(ns,as,sourceExpression,label){
    assert(ts.isExpressionStatement(ns)&&ts.isCallExpression(ns.expression)&&text(native,ns.expression.expression)===n.system.publisher&&ns.expression.arguments.length===2);const call=ns.expression;assert(ts.isStringLiteral(call.arguments[0])&&call.arguments[0].text===binding.name.text);
    const na=call.arguments[1],aa=as.expression;assert(ts.isBinaryExpression(na)&&ts.isBinaryExpression(aa));assert(ts.isIdentifier(na.left)&&na.left.text===binding.name.text);assert(ts.isPropertyAccessExpression(aa.left)&&text(actual,aa.left.expression)==='exports'&&aa.left.name.text===binding.name.text);assert.equal(na.operatorToken.kind,sourceExpression.operatorToken.kind);assert.equal(aa.operatorToken.kind,sourceExpression.operatorToken.kind);assert(ts.isNumericLiteral(na.right)&&ts.isNumericLiteral(aa.right));assert.equal(na.right.text,sourceExpression.right.text);assert.equal(aa.right.text,sourceExpression.right.text);
    const owner=native.owner(call.getStart(native.source));uniform(native,call.getStart(native.source),na.left.getStart(native.source),owner,label+' Native publication prefix');uniform(actual,aa.getStart(actual.source),aa.left.name.getStart(actual.source),owner,label+' expanded publication prefix');
    for(const[x,y,role]of[[na.left,aa.left.name,'source binding'],[na.operatorToken,aa.operatorToken,'operator'],[na.right,aa.right,'value']])pair(x,y,label+' '+role);
    const tail=native.owner(na.right.end);uniform(native,na.right.end,ns.end,tail,label+' Native completion transport');uniform(actual,aa.right.end,as.end,tail,label+' actual completion transport');endpoint(ns,as,label+' complete statement');
    endpoint(na.right,aa.right,label+' value endpoint');
  }
  const syntheticInitial={operatorToken:{kind:ts.SyntaxKind.EqualsToken},right:binding.initializer};publication(initial,actualInitial,syntheticInitial,'initial variable publication');
  publication(nf.body.statements[0],af.body.statements[0],sourceUpdate.expression,'original live-binding update');
  const nFunctionTokens=native.tokens(nf).filter(t=>!(nf.body.statements[0].getStart(native.source)<=t.start&&t.end<=nf.body.statements[0].end)),aFunctionTokens=actual.tokens(af).filter(t=>!(af.body.statements[0].getStart(actual.source)<=t.start&&t.end<=af.body.statements[0].end));contract.pair(nFunctionTokens,aFunctionTokens,'complete source function signature/body delimiters');endpoint(nf,af,'complete original lifted function');endpoint(nf.body,af.body,'original function body');
  const invocation=item=>one(item.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText(item.view.source)===sourceFunction.name.text),'one source function invocation');
  const ni=invocation(n),ai=invocation(a);pair(ni,ai,'complete retained original invocation');
  const consoleNode=item=>one(item.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText(item.view.source)==='console.log'),'one original console statement');const nc=consoleNode(n),ac=consoleNode(a);assert.equal(nc.expression.arguments.length,1);assert.equal(ac.expression.arguments.length,1);const nr=nc.expression.arguments[0],ar=ac.expression.arguments[0];assert(ts.isIdentifier(nr)&&nr.text===binding.name.text);assert(ts.isPropertyAccessExpression(ar)&&text(actual,ar.expression)==='exports'&&ar.name.text===binding.name.text);
  contract.pair([native.token(nr)],[actual.token(ar)],'complete source binding read through System transport',new Map([[text(actual,ar),nr.text]]));endpoint(nr,ar,'exact original variable-read endpoint');
  const nt=native.tokens(nc).filter(t=>!(nr.getStart(native.source)<=t.start&&t.end<=nr.end)),at=actual.tokens(ac).filter(t=>!(ar.getStart(actual.source)<=t.start&&t.end<=ar.end));contract.pair(nt,at,'complete original console syntax');endpoint(nc,ac,'complete original console statement');
  const nInventory=[strict,hoist,moduleName,nf,functionPublication,initial,ni,nc],aInventory=[af,...slots,actualFunctionPublication,actualInitial,ai,ac,refresh];assert.deepEqual(new Set(nInventory),new Set(n.sourceNodes),'complete source-derived Native statement inventory');assert.deepEqual(new Set(aInventory),new Set(a.sourceNodes),'complete source-derived actual statement inventory');
  contract.finish([]);
  return {original,native:n,actual:a,pairs,uniforms,ends,generatedComments,completeModuleRoles:true,completeUTF16Characters:{native:native.code.length,actual:actual.code.length}};
}
module.exports={compare};
