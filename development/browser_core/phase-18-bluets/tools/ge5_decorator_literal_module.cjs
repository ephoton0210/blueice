// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),helpers=require('./ge5_decorator_helpers.cjs'),fields=require('./ge5_decorator_fields.cjs');
function readComponents(ts,native,actual,options,original){
  const sourceNames=new Set();function sourceIdentifiers(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,sourceIdentifiers);}sourceIdentifiers(original);
  const originalText=original.text,classes=original.statements.filter(ts.isClassDeclaration);assert.equal(classes.length,1);const source=classes[0],sourceFields=source.members.filter(ts.isPropertyDeclaration);assert.equal(sourceFields.length,source.members.length);
  const proof=helpers.compare(ts,native,actual,[originalText]);
  function read(view,state){
    const aliases=new Map([...state.bindings].map(([name,item])=>[name,'$helper.'+item.kind])),slots=new Map(),slotNames=new Set(),stores=[];
    function visit(node){
      if(state.definitions.includes(node))return;
      if(ts.isCallExpression(node)&&ts.isIdentifier(node.expression)&&helpers.helpers.get(node.expression.text)==='esDecorate'){
        assert.equal(node.arguments.length,6);const context=node.arguments[3];assert(ts.isObjectLiteralExpression(context));const properties=new Map(context.properties.map(p=>{assert(ts.isPropertyAssignment(p));return[p.name.getText(view.source),p.initializer]}));assert.equal(properties.get('kind').text,'field');assert.equal(properties.get('private').kind,ts.SyntaxKind.FalseKeyword);const key=properties.get('name').text,isStatic=properties.get('static').kind===ts.SyntaxKind.TrueKeyword;const field=sourceFields.find(f=>fields.literal(ts,f.name)===key&&!!f.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword)===isStatic);assert(field,'decoration context belongs to an exact original field');assert(!slots.has(field),'one decoration event for each source field');for(const[index,role]of[[2,'decorators'],[4,'initializers'],[5,'extra']]){assert(ts.isIdentifier(node.arguments[index]));const name=node.arguments[index].text;assert(!sourceNames.has(name)&&!slotNames.has(name),'each decorator/initializer/extra slot is independently fresh from source and other roles');slotNames.add(name);aliases.set(name,'$field.'+key+'.'+role);}slots.set(field,node);
      }
      ts.forEachChild(node,visit);
    }
    visit(view.source);
    if(!options.experimentalDecorators)assert.equal(slots.size,sourceFields.length);
    // Receiver identities below are source-bound class names or compiler values
    // assigned the unique emitted named class expression, rather than arbitrary bindings.
    const receivers=new Set([source.name.text]);
    function classValues(node){
      if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(node.left)){
        const value=fields.unwrap(ts,node.right);let named=ts.isClassExpression(value)&&value.name?.text===source.name.text;
        if(ts.isCallExpression(value)){const fn=fields.unwrap(ts,value.expression);if(ts.isFunctionExpression(fn)&&ts.isBlock(fn.body))named=fn.body.statements.some(statement=>ts.isFunctionDeclaration(statement)&&statement.name?.text===source.name.text)&&fn.body.statements.some(statement=>ts.isReturnStatement(statement)&&ts.isIdentifier(statement.expression)&&statement.expression.text===source.name.text);}
        if(named)receivers.add(node.left.text);
      }
      ts.forEachChild(node,classValues);
    }
    classValues(view.source);
    for(const name of receivers)aliases.set(name,'$field.receiver');aliases.set('this','$field.receiver');
    function candidates(node){
      if(state.definitions.includes(node))return;
      if(ts.isCallExpression(node)||ts.isExpressionStatement(node)||ts.isPropertyDeclaration(node)||ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.EqualsToken){
          const isDescriptor=expression=>ts.isCallExpression(expression)&&expression.expression.getText(view.source)==='Object.defineProperty'&&expression.arguments.length===3&&(ts.isStringLiteral(expression.arguments[1])||ts.isNumericLiteral(expression.arguments[1]))&&ts.isObjectLiteralExpression(expression.arguments[2])&&expression.arguments[2].properties.length===4;
          const canRead=ts.isPropertyDeclaration(node)||ts.isBinaryExpression(node)||isDescriptor(node)||ts.isExpressionStatement(node)&&(ts.isBinaryExpression(node.expression)||ts.isCallExpression(node.expression)&&node.expression.expression.getText(view.source)==='Object.defineProperty'&&node.expression.arguments.length===3&&(ts.isStringLiteral(node.expression.arguments[1])||ts.isNumericLiteral(node.expression.arguments[1]))&&ts.isObjectLiteralExpression(node.expression.arguments[2])&&node.expression.arguments[2].properties.length===4);
          if(canRead){const store=fields.readStore(ts,view,node);if(store&&sourceFields.some(field=>fields.literal(ts,field.name)===store.key)&&(!store.receiver||store.receiver.kind===ts.SyntaxKind.ThisKeyword||ts.isIdentifier(store.receiver)&&receivers.has(store.receiver.text))){if(!stores.some(s=>s.expression===store.expression))stores.push(store);}}
      }
      ts.forEachChild(node,candidates);
    }
    candidates(view.source);assert.equal(stores.length,sourceFields.length,'exact complete source field store inventory');return {aliases,slots,stores};
  }
  const n=read(native,proof.native),a=read(actual,proof.actual),fieldProofs=[];
  for(const sourceField of sourceFields){const key=fields.literal(ts,sourceField.name),ns=n.stores.filter(s=>s.key===key),as=a.stores.filter(s=>s.key===key);assert.equal(ns.length,1);assert.equal(as.length,1);fieldProofs.push(fields.compareStore(ts,native,ns[0].node,actual,as[0].node,sourceField,{nativeAliases:n.aliases,actualAliases:a.aliases}));}
  const metadata=!options.experimentalDecorators?require('./ge5_decorator_field_metadata.cjs').compare(ts,native,actual,proof.native,proof.actual,original,{native:n,actual:a}):null;
  return {source,sourceFields,native:n,actual:a,fields:fieldProofs,metadata,helpers:proof};
}
function compare(ts,native,actual,options,originalText){
  let previous;for(const point of actual.decoded.segments){assert(!previous||point[0]>previous[0]||point[0]===previous[0]&&point[1]>previous[1],'ordered unique decoded segments cannot hide a zero-width source grant');previous=point;}
  const comments=require('./ge5_decorator_preserved.cjs').comments,originalComments=comments(ts,{code:originalText}).map(comment=>comment.text),ast=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true);let classCount=0;function classes(node){if(ts.isClassDeclaration(node)||ts.isClassExpression(node))classCount++;ts.forEachChild(node,classes);}classes(ast);
  const asset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/class_helpers.v1.js'),'utf8'),assetComments=comments(ts,{code:asset}).map(comment=>comment.text);
  for(const [view,blue]of [[native,false],[actual,true]]){const all=comments(ts,view),publications=all.filter(comment=>comment.text.startsWith('//# sourceMappingURL='));assert.equal(publications.length,1,'one complete map publication');const payload=publications[0].text.slice('//# sourceMappingURL='.length);if(blue&&payload.startsWith('data:application/json;base64,'))assert.equal(Buffer.from(payload.split('base64,')[1],'base64').toString('utf8'),actual.raw,'inline publication carries this complete observed map');const generated=all.filter(comment=>comment!==publications[0]&&!originalComments.includes(comment.text)).map(comment=>comment.text),expected=options.target!=='ES5'?[]:blue?options.importHelpers||options.noEmitHelpers?[]:Array.from({length:classCount},()=>assetComments).flat():Array(classCount).fill('/** @class */');assert.deepEqual(generated,expected,'complete original-class-derived helper/comment inventory');}
  const footer=view=>comments(ts,view).find(comment=>comment.text.startsWith('//# sourceMappingURL='));if(!footer(native).text.includes('base64,'))assert.equal(footer(actual).text,footer(native).text,'exact external map publication identity');
  if(!options.experimentalDecorators&&options.target==='ESNext'&&options.useDefineForClassFields)return {preserved:true,...require('./ge5_decorator_preserved.cjs').compare(ts,native,actual)};
  let components,prelude,legacy;
  return require('./ge5_class_module.cjs').compare(ts,native,actual,options,originalText,{
    prepare({n,a,contract,original}){
      components=readComponents(ts,native,actual,options,original);
      if(options.experimentalDecorators){
        legacy=require('./ge5_decorator_roles.cjs').compare(ts,native,actual,[originalText]);
        components.legacyPairs=legacy.statementPairs;
        for(const pair of legacy.statementPairs){actual.nativeStarts.set(pair.actual.getStart(actual.source),pair.native.getStart(native.source));actual.nativeEnds.set(pair.actual.end,pair.native.end);}
        for(const pair of legacy.statementPairs)for(const [view,node]of [[native,pair.native],[actual,pair.actual]]){view.roleStarts.set(node.getStart(view.source),view.owner(node.getStart(view.source)));view.roleEnds.set(node.end,view.owner(node.end));}
      }
      prelude=require('./ge5_class_expression_prelude.cjs').compare(ts,native,actual,original,options,contract);assert.equal(prelude.deferred.length,0,'complete source callable prelude inventory');
      const names=new Set();function sourceNames(node){if(ts.isIdentifier(node))names.add(node.text);ts.forEachChild(node,sourceNames);}sourceNames(original);
      function unusedThis(item,blue){const seen=[];for(const node of item.sourceNodes){if(!ts.isVariableStatement(node)||node.declarationList.declarations.length!==1)continue;const declaration=node.declarationList.declarations[0];if(!ts.isIdentifier(declaration.name)||names.has(declaration.name.text)||declaration.initializer?.kind!==ts.SyntaxKind.ThisKeyword)continue;let count=0;function refs(n){if(ts.isIdentifier(n)&&n.text===declaration.name.text)count++;ts.forEachChild(n,refs);}refs(item.view.source);assert.equal(count,1,'compiler-only unused top-level this capture has no semantic consumers');seen.push(node);const position=original.getLineAndCharacterOfPosition(original.statements[0].getStart(original)),owner=[0,position.line,position.character];for(let offset=node.getStart(item.view.source);offset<node.end;offset++)assert.deepEqual(item.view.owner(offset),owner,'unused generated this capture retains the Native source-file entry role');item.view.cover(node.getStart(item.view.source),node.end);item.view.roleStarts.set(node.getStart(item.view.source),owner);item.view.roleEnds.set(node.end,item.view.owner(node.end));}assert.equal(seen.length,!blue&&options.target==='ES5'&&!options.experimentalDecorators?1:0,'exact unused lexical-this capture inventory');return seen;}
      const captures={native:unusedThis(n,false),actual:unusedThis(a,true)};
      const classProofs=prelude.classes.filter(proof=>proof.classProof).map(proof=>({...proof.classProof,native:{...proof.classProof.native,node:proof.native.node}}));
      return {components,prelude,legacy,captures,classProofs};
    },
    compareClass({source,nn,an,contract,n,a}){
      const core=require('./ge5_decorator_field_core.cjs').compare(ts,native,actual,source,options,components,legacy);
      return require('./ge5_decorator_field_frame.cjs').compare(ts,native,nn,actual,an,source,options,components,core,contract,{native:n,actual:a});
    }
  });
}
module.exports={compare,readComponents};
