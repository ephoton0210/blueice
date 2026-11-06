// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Whole replacement classes compose their independently identified construction,
// source initializer, metadata, queue, publication and enclosing callable phases.
const assert=require('node:assert/strict'),frames=require('./ge5_decorator_replacement_frame.cjs'),helpers=require('./ge5_decorator_helpers.cjs'),fields=require('./ge5_decorator_fields.cjs');
function compare(ts,native,actual,options,originalText){
 require('./ge5_decorator_replacement_envelope.cjs').verify(ts,native,actual,options,originalText);
 if(options.experimentalDecorators)return require('./ge5_decorator_replacement_legacy.cjs').compare(ts,native,actual,options,originalText);
 if(options.target==='ESNext'&&options.useDefineForClassFields)return{preserved:true,...require('./ge5_decorator_preserved.cjs').compare(ts,native,actual)};
 let previous;for(const point of actual.decoded.segments){assert(!previous||point[0]>previous[0]||point[0]===previous[0]&&point[1]>previous[1],'ordered unique source-map entries');previous=point;}
 let components;
 const result=require('./ge5_class_module.cjs').compare(ts,native,actual,options,originalText,{
  prepare({n,a,contract,original}){
   const sources=original.statements.filter(ts.isClassDeclaration);assert.equal(sources.length,1);const source=sources[0],helperProof=helpers.compare(ts,native,actual,[originalText]),nf=frames.readStandard(ts,native,source,options),af=frames.readStandard(ts,actual,source,options,true);
   if(!['ES2022','ESNext'].includes(options.target)){
    const canonical=ts.createSourceFile('name-helper.js',ts.transpileModule('declare function d(...args:any[]):any;@d class Example{}',{compilerOptions:{target:ts.ScriptTarget[options.target],module:ts.ModuleKind.ESNext}}).outputText,99,true),expected=canonical.statements.find(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.some(d=>d.name.text==='__setFunctionName'));
    const definitions=native.source.statements.filter(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.some(d=>d.name.text==='__setFunctionName'));assert.equal(definitions.length,1);assert(expected);assert.deepEqual(frames.tokens(ts,definitions[0].getText(native.source)),frames.tokens(ts,expected.getText(canonical)),'complete pinned class-name helper algorithm');native.noSource(definitions[0].getStart(native.source),definitions[0].end,'complete generated class-name algorithm');
   }
   const prelude=require('./ge5_class_expression_prelude.cjs').compare(ts,native,actual,original,options,contract);assert.equal(prelude.deferred.length,0,'complete source callable inventory');
   const classProofs=prelude.classes.filter(p=>p.classProof).map(p=>({...p.classProof,native:{...p.classProof.native,node:p.native.node}}));
   components={source,helpers:helperProof,native:nf,actual:af,fields:[],endpoints:[]};return{components,prelude,classProofs};
  },
  compareClass({source,nn,an,contract}){
   const n=components.native,a=components.actual;assert.equal(n.root,nn);assert.equal(a.root,an);
   const modern=['ES2022','ESNext'].includes(options.target);
   for(const item of[n,a]){
    const aliases=item.aliases;aliases.set(item.binding.metadata.name.text,'$class.metadata');
    for(const[name,binding]of item.binding.helperState.bindings)aliases.set(name,'$helper.'+binding.kind);
    if(item.captured)aliases.set(item.captured.name.text,'$class.initial');
    if(item.constructor.name)aliases.set(item.constructor.name.text,'$class.constructor');
   }
   function units(view,node,item){const literal=new Map();function visit(node){if(ts.isStringLiteral(node)||ts.isNumericLiteral(node))literal.set(node.getStart(view.source),node);ts.forEachChild(node,visit);}visit(node);return view.tokens(node).filter(t=>!view.covered(t.start)).map(token=>{const value=literal.get(token.start),text=item.aliases.get(token.text)||(value?'$literal.'+value.kind+'.'+value.text:null);if(!text)return token;const owner=token.owners[0];assert(token.owners.every(p=>JSON.stringify(p)===JSON.stringify(owner)),'complete alias/literal UTF-16 region');return{...token,text,owners:[owner]};});}
   function pairNodes(nnode,anode,label){const nt=units(native,nnode,n),at=units(actual,anode,a);contract.pair(nt,at,label);for(let i=0;i<nt.length;i++){assert.deepEqual(actual.owner(at[i].end),native.owner(nt[i].end),label+' exact logical token/subtree end '+nt[i].text);components.endpoints.push({kind:label,actual:at[i].end,native:nt[i].end,owner:native.owner(nt[i].end)});}assert.deepEqual(actual.owner(anode.end),native.owner(nnode.end),label+' complete AST endpoint');}
   function generated(view,node,label){view.noSource(node.getStart(view.source),node.end,label);}
   function transport(view,node,label){for(const token of view.tokens(node).filter(t=>!view.covered(t.start)))view.noSource(token.start,token.end,label);}
   function grouping(view,item){function visit(node){if(ts.isParenthesizedExpression(node)){const expected=frames.tokens(ts,node.expression.getText(view.source));assert(expected.length);for(const token of node.getChildren(view.source).filter(ts.isToken)){let call=node;while(call&&!item.memberBindings.entries.size)call=call.parent;const entry=[...item.memberBindings.entries].find(([,entry])=>entry.call.pos<=node.pos&&node.end<=entry.call.end);if(entry){const owner=view.owner(entry[1].call.getStart(view.source));for(let at=token.getStart(view.source);at<token.end;at++)assert.deepEqual(view.owner(at),owner,'member-context grouping owns the exact original member phase');view.cover(token.getStart(view.source),token.end);}}}ts.forEachChild(node,visit);}visit(item.metadataBlock);}
   grouping(native,n);grouping(actual,a);pairNodes(n.metadataBlock,a.metadataBlock,'complete ordered member/class metadata body');
   for(let i=0;i<n.plans.length;i++){
    const np=n.plans[i],ap=a.plans[i];assert.equal(ap.node,np.node);const ns=n.stores.get(np),as=a.stores.get(ap);pairNodes(ns.userInitializer,as.userInitializer,'complete original field initializer subtree '+np.key);const proof=fields.compareStore(ts,native,ns.node,actual,as.node,np.node,{nativeAliases:n.aliases,actualAliases:a.aliases});components.fields.push(proof);
   }
   pairNodes(n.constructor,a.constructor,'complete implicit constructor and instance extras');
   for(let i=0;i<n.extraCalls.length;i++)pairNodes(n.extraCalls[i],a.extraCalls[i],'complete initializer queue call '+i);
   if(modern){
    generated(actual,a.namePhase.body.statements[0],'complete generated original-class name setter');pairNodes(n.namePhase.node,a.namePhase.node,'complete initial class receiver capture');
    pairNodes(n.extraPhase.node,a.extraPhase.node,'complete static and class extra phase');
    pairNodes(n.classValue,a.classValue,'complete retained class construction shell');
   }else{
    generated(native,n.nameSetter.parent,'complete Native class name setter');generated(actual,a.nameSetter.parent,'complete generated class name setter');generated(actual,a.namePhase.node.parent,'complete generated initial class receiver capture');
    transport(native,n.metadataPhase.node.parent,'Native metadata invocation transport');transport(actual,a.metadataPhase.node.parent,'metadata invocation transport');
    pairNodes(n.extraPhase.body,a.extraPhase.body,'complete static and class extra body');transport(native,n.extraPhase.node.parent,'Native class-extra invocation transport');transport(actual,a.extraPhase.node.parent,'class-extra invocation transport');
    if(options.target==='ES5')pairNodes(n.factory.fn,a.factory.fn,'complete base class factory');
    else{
     if(a.classValue.name)generated(actual,a.classValue.name,'generated original class value binding');pairNodes(n.classValue,a.classValue,'complete base class value');
    }
    if(options.target==='ES5'){
     const point=source.getSourceFile().getLineAndCharacterOfPosition(source.end),completion=[0,point.line,point.character];
     for(const[view,item,syntax]of[[native,n,n.classBinding.initializer],[actual,a,a.captured.initializer]])for(const token of view.tokens(syntax).filter(t=>!view.covered(t.start))){const suffix=token.start>=item.factory.fn.end;if(suffix)assert(token.text==='('||token.text===')','complete original base-factory invocation delimiters');const owner=suffix?completion:null;for(let at=token.start;at<token.end;at++)assert.deepEqual(view.owner(at),owner,'complete source class factory grouping/invocation UTF-16 role');view.cover(token.start,token.end);view.roleStarts.set(token.start,owner);view.roleEnds.set(token.end,view.owner(token.end));}
     assert.deepEqual(actual.owner(a.classValue.end),native.owner(n.classValue.end),'complete initial class factory invocation endpoint');
    }
    if(options.target==='ES5'){
     const actualStatement=a.captured.parent.parent;assert(ts.isVariableStatement(actualStatement));const nt=native.tokens(n.bindingStatement).at(-1),at=actual.tokens(actualStatement).at(-1);assert.equal(nt.text,';');assert.equal(at.text,';');contract.pair([nt],[at],'complete initial class construction statement terminator');assert.deepEqual(actual.owner(actualStatement.end),native.owner(n.bindingStatement.end),'complete initial class construction statement endpoint');
    }
    transport(native,n.classBinding.initializer,'Native initial class receiver assignment/grouping');transport(actual,a.construction.node,'complete captured class construction transport');
   }
   for(let i=0;i<n.prefix.length;i++)pairNodes(n.prefix[i],a.prefix[i],'complete original decorator and queue slot '+i);
   pairNodes(n.returned,a.returned,'complete final replaced class binding result');
   if(modern)pairNodes(nn,an,'complete replacement class binding');
   else{
    // Both independently validated wrappers differ only in the additional Blue
    // per-evaluation capture; the original outer header and completion are paired.
    for(const[view,item]of[[native,n],[actual,a]]){let expression=item.outer.node.expression;while(ts.isParenthesizedExpression(expression)){for(const token of expression.getChildren(view.source).filter(ts.isToken)){const start=token.getStart(view.source),owner=token.kind===ts.SyntaxKind.OpenParenToken?native.owner(n.outer.fn.getStart(native.source)):native.owner(n.outer.fn.end);for(let offset=start;offset<token.end;offset++)assert.deepEqual(view.owner(offset),owner,'complete outer ordinary class-factory grouping role');view.cover(start,token.end);view.roleStarts.set(start,owner);view.roleEnds.set(token.end,view.owner(token.end));}expression=expression.expression;}}
    const head=(view,item)=>view.tokens(item.root).filter(t=>t.end<=item.outer.body.statements[0].getStart(view.source)&&!view.covered(t.start)),tail=(view,item)=>view.tokens(item.root).filter(t=>t.start>=item.returned.end&&!view.covered(t.start));contract.pair(head(native,n),head(actual,a),'complete original class binding/factory header');contract.pair(tail(native,n),tail(actual,a),'complete original class binding/factory completion');
    transport(native,n.bindingStatement,'complete Native initial class binding');transport(actual,a.bindingStatement,'complete captured initial class binding');
   }
   if(options.target==='ES5')for(const comment of native.comments().filter(c=>n.bindingStatement.pos<=c.start&&c.end<=n.factory.fn.getStart(native.source))){assert.equal(comment.text,'/** @class */');const owner=native.owner(n.factory.fn.getStart(native.source));for(let at=comment.start;at<comment.end;at++)assert.deepEqual(native.owner(at),owner,'complete original base-class factory marker');native.cover(comment.start,comment.end);native.roleStarts.set(comment.start,owner);native.roleEnds.set(comment.end,native.owner(comment.end));}
   contract.region(n.binding.declaration.initializer,a.binding.declaration.initializer);
   const nd=n.binding.declaration,ad=a.binding.declaration,prefix=(view,node,d)=>view.tokens(node).filter(t=>t.end<=d.initializer.getStart(view.source)).at(-1),suffix=(view,node,d)=>view.tokens(node).find(t=>t.start>=d.initializer.end);require('./ge5_role_stream.cjs').compareGap(native,prefix(native,nn,nd).end,nd.initializer.getStart(native.source),actual,prefix(actual,an,ad).end,ad.initializer.getStart(actual.source),'complete replacement binding to initialization gap');require('./ge5_role_stream.cjs').compareGap(native,nd.initializer.end,suffix(native,nn,nd).start,actual,ad.initializer.end,suffix(actual,an,ad).start,'complete replacement initialization to binding terminator gap');

   for(const [view,node]of[[native,nn],[actual,an]])for(let at=node.getStart(view.source);at<node.end;at++)assert(view.covered(at),'complete replacement class UTF-16 coverage');
   assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete replacement class binding endpoint');return{source,native:{...n,node:nn,native:options.target!=='ES5',factory:n.factory?.fn},actual:{...a,node:an,native:options.target!=='ES5',factory:a.factory?.fn},complete:true};
  }
 });
 require('./ge5_decorator_replacement_envelope.cjs').ends(ts,native,actual);return result;
}
module.exports={compare};
