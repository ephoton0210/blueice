// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Auto-storage, callable bodies, metadata and module transport must all close together.
const assert=require('node:assert/strict');
function componentsFor(ts,native,actual,options,original){
 if(options.target!=='ESNext'||options.experimentalDecorators||options.useDefineForClassFields)return require('./ge5_decorator_member_module.cjs').readComponents(ts,native,actual,options,original);
 const source=original.statements.find(ts.isClassDeclaration),memberPlan=require('./ge5_decorator_member_plan.cjs'),plans=memberPlan.original(ts,source),roles=require('./ge5_decorator_roles.cjs'),helpers=require('./ge5_decorator_helpers.cjs').compare(ts,native,actual,[original.text]),syntax=require('./ge5_decorator_auto_syntax.cjs').compare(ts,native,actual,source,options);
 function read(view,blue){const state=roles.read(ts,view,[original.text],blue),result=memberPlan.read(ts,view,state,source,plans,{receivers:new Set([source.name.text])}),aliases=syntax[blue?'actual':'native'].aliases;for(const[name,alias]of aliases)if(alias.startsWith('$helper.')){if(result.aliases.has(name))assert.equal(result.aliases.get(name),alias);else result.aliases.set(name,alias);}result.slots=new Map([...result.entries].map(([plan,entry])=>[plan.node,entry.call]));result.aliases.set(source.name.text,'$field.receiver');result.aliases.set('this','$field.receiver');return result;}
 const n=read(native,false),a=read(actual,true),metadata=require('./ge5_decorator_member_metadata.cjs').compare(ts,native,actual,roles.read(ts,native,[original.text],false),roles.read(ts,actual,[original.text],true),original,{native:n,actual:a});return{source,sourceFields:plans.map(plan=>plan.node),plans,native:n,actual:a,fields:[],metadata,helpers,syntax};
}
function compare(ts,native,actual,options,originalText){
 assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);let previous;for(const point of actual.decoded.segments){assert(!previous||point[0]>previous[0]||point[0]===previous[0]&&point[1]>previous[1],'ordered unique decoded auto-accessor segments');previous=point;}
 const original=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true),source=original.statements.find(ts.isClassDeclaration),plans=require('./ge5_decorator_auto_accessors.cjs').original(ts,source),comments=require('./ge5_decorator_preserved.cjs').comments,originalComments=comments(ts,{code:originalText}).map(comment=>comment.text);
 for(const[view,blue]of[[native,false],[actual,true]]){const all=comments(ts,view),publications=all.filter(comment=>comment.text.startsWith('//# sourceMappingURL='));assert.equal(publications.length,1,'one complete auto-accessor map publication');const payload=publications[0].text.slice('//# sourceMappingURL='.length);if(blue&&payload.startsWith('data:application/json;base64,'))assert.equal(Buffer.from(payload.split('base64,')[1],'base64').toString('utf8'),actual.raw,'the inline publication carries this exact complete map');const generated=all.filter(comment=>comment!==publications[0]&&!originalComments.includes(comment.text)).map(comment=>comment.text);assert.deepEqual(generated,blue&&options.target==='ES2015'?['/* bluets-class-helper-v1 */']:[],'complete auto-storage helper/comment inventory');}
 if(options.target==='ESNext'&&!options.experimentalDecorators&&options.useDefineForClassFields){const syntax=require('./ge5_decorator_auto_syntax.cjs').compare(ts,native,actual,source,options),chain=require('./ge5_class_declaration_chain.cjs').compare(ts,native,syntax.native.node,syntax.native,actual,syntax.actual.node,syntax.actual);return{preserved:true,constructionChain:{pairs:chain},syntax,...require('./ge5_decorator_preserved.cjs').compare(ts,native,actual)};}
 let components,prelude,legacy,lowered;
 return require('./ge5_class_module.cjs').compare(ts,native,actual,options,originalText,{
  prepare({contract,original}){
   components=componentsFor(ts,native,actual,options,original);
   if(options.experimentalDecorators){legacy=require('./ge5_decorator_auto_legacy.cjs').compare(ts,native,actual,components.source);components.legacyPairs=legacy.statementPairs;}
   if(options.target==='ES2015'){
    for(const comment of actual.comments().filter(comment=>comment.text==='/* bluets-class-helper-v1 */'))actual.noSource(comment.start,comment.end,'exact generated auto-storage helper version comment');
    lowered=require('./ge5_decorator_auto_lowered.cjs').inspect(ts,native,actual,components.source,options);
    const sourceNames=new Set();function names(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,names);}names(original);
    for(const[state,view]of[[lowered.native,native],[lowered.actual,actual]]){const expected=new Set([...state.entries.values()].map(entry=>entry.storage)),statements=new Set();for(const entry of state.entries.values()){let node=entry.declaration;while(!ts.isVariableStatement(node))node=node.parent;statements.add(node);if(entry.plan.static&&!sourceNames.has(entry.receiver))expected.add(entry.receiver);}assert.equal(statements.size,1,'one exact lexical private-storage declaration group');const statement=[...statements][0];assert(statement.declarationList.declarations.every(declaration=>ts.isIdentifier(declaration.name)&&!declaration.initializer));assert.deepEqual(statement.declarationList.declarations.map(declaration=>declaration.name.text).sort(),[...expected].sort(),'complete derived storage/capture slot inventory');view.noSource(statement.getStart(view.source),statement.end,'complete compiler auto-storage lexical declarations');}
   }
   prelude=require('./ge5_class_expression_prelude.cjs').compare(ts,native,actual,original,options,contract);assert.equal(prelude.deferred.length,0,'complete original auto-accessor source prelude callables');const classProofs=prelude.classes.filter(proof=>proof.classProof).map(proof=>({...proof.classProof,native:{...proof.classProof.native,node:proof.native.node}}));return{components,prelude,legacy,lowered,classProofs};
  },
  compareClass({source,nn,an,contract,n,a}){
   const core=require('./ge5_decorator_auto_core.cjs').compare(ts,native,actual,source,options,components);components.core=core;components.constructionChain={pairs:require('./ge5_class_declaration_chain.cjs').compare(ts,native,nn,core.native,actual,an,core.actual)};
   return require('./ge5_decorator_auto_frame.cjs').compare(ts,native,nn,actual,an,source,options,components,core,contract,{native:n,actual:a});
  }
 });
}
module.exports={compare};
