// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independently read a static-state class value and the factory preserving each evaluation.
const assert=require('node:assert/strict'),classes=require('./ge5_class_roles.cjs');
function unwrap(ts,node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function commas(ts,node){node=unwrap(ts,node);return ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.CommaToken?[...commas(ts,node.left),...commas(ts,node.right)]:[node];}
function read(ts,view,returned,original,blue,options) {
  assert(ts.isReturnStatement(returned)&&returned.expression);assert(ts.isClassExpression(original)&&!ts.getDecorators(original)?.length);
  const fields=original.members.filter(ts.isPropertyDeclaration);assert.equal(fields.length,original.members.length,'closed class-value static field inventory');assert(fields.every(field=>field.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.StaticKeyword)));
  const sourceNames=new Set();function names(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,names);}names(original.getSourceFile());
  let value=unwrap(ts,returned.expression),classValue=value,temporary=null,nameSetter=null,stores=[],factory=null,captured=null,classSyntax=value,transportDeclaration=null,returnedBinding=null,captureStatement=null;
  if(ts.isClassExpression(value)) {
    const blocks=value.members.filter(ts.isClassStaticBlockDeclaration);
    if(blocks.length){assert.equal(blocks.length,value.members.length);stores=blocks.map(block=>{assert.equal(block.body.statements.length,1);const statement=block.body.statements[0];assert(ts.isExpressionStatement(statement));return statement.expression;});}
    else {assert.equal(value.members.length,fields.length);stores=value.members.map(field=>{assert(ts.isPropertyDeclaration(field)&&field.initializer);return field;});}
  } else if(blue) {
    assert(ts.isCallExpression(value)&&value.arguments.length===0);factory=unwrap(ts,value.expression);assert((ts.isArrowFunction(factory)||ts.isFunctionExpression(factory))&&factory.parameters.length===0&&ts.isBlock(factory.body));
    const statements=[...factory.body.statements],declaration=statements.shift(),last=statements.pop();assert(ts.isVariableStatement(declaration)&&declaration.declarationList.declarations.length===1);captureStatement=declaration;captured=declaration.declarationList.declarations[0];assert(ts.isIdentifier(captured.name)&&captured.initializer&&!sourceNames.has(captured.name.text));temporary=captured.name.text;classSyntax=captured.initializer;classValue=unwrap(ts,classSyntax);
    assert(ts.isReturnStatement(last)&&ts.isIdentifier(last.expression)&&last.expression.text===temporary,'complete per-evaluation class return');returnedBinding=last;
    if(!original.name) {
      const statement=statements.shift();assert(ts.isExpressionStatement(statement)&&ts.isCallExpression(statement.expression));nameSetter=statement.expression;assert.equal(nameSetter.expression.getText(view.source),'Object.defineProperty');assert.equal(nameSetter.arguments.length,3);const [target,key,descriptor]=nameSetter.arguments;assert(ts.isIdentifier(target)&&target.text===temporary&&ts.isStringLiteral(key)&&key.text==='name'&&ts.isObjectLiteralExpression(descriptor));
      assert.deepEqual(descriptor.properties.map(property=>property.name.getText(view.source)),['value','configurable']);assert(ts.isStringLiteral(descriptor.properties[0].initializer)&&descriptor.properties[1].initializer.kind===ts.SyntaxKind.TrueKeyword);
    }
    assert.equal(statements.length,fields.length);stores=statements.map(statement=>{assert(ts.isExpressionStatement(statement));return statement.expression;});
  } else {
    const parts=commas(ts,value);assert.equal(parts.length,fields.length+2);const assignment=parts.shift(),last=parts.pop();assert(ts.isBinaryExpression(assignment)&&assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(assignment.left));temporary=assignment.left.text;assert(!sourceNames.has(temporary)&&ts.isIdentifier(last)&&last.text===temporary);classSyntax=assignment.right;classValue=unwrap(ts,classSyntax);stores=parts;returnedBinding=last;
    const parent=returned.parent;assert(ts.isBlock(parent));const declarations=parent.statements.filter(statement=>ts.isVariableStatement(statement)&&statement.declarationList.declarations.some(declaration=>ts.isIdentifier(declaration.name)&&declaration.name.text===temporary));assert.equal(declarations.length,1);const declaration=declarations[0];transportDeclaration=declaration;assert.equal(declaration.declarationList.declarations.length,1);assert(!declaration.declarationList.declarations[0].initializer);const entry=view.tokens(parent)[0],owner=view.owner(entry.start);for(let offset=declaration.getStart(view.source);offset<declaration.end;offset++)assert.deepEqual(view.owner(offset),owner,'Native class-value temporary keeps its original callable entry role');view.cover(declaration.getStart(view.source),declaration.end);(view.roleStarts??=new Map()).set(declaration.getStart(view.source),owner);(view.roleEnds??=new Map()).set(declaration.end,view.owner(declaration.end));
  }
  const records=stores.map((store,index)=>{
    const field=fields[index];assert(ts.isIdentifier(field.name)||ts.isStringLiteral(field.name),'closed original static field key');const expected=field.name.text;
    if(ts.isPropertyDeclaration(store)){assert.equal(store.name.text,expected);return {node:store,key:store.name,value:store.initializer,receiver:null,descriptor:null};}
    if(ts.isBinaryExpression(store)) {assert.equal(store.operatorToken.kind,ts.SyntaxKind.EqualsToken);const target=store.left;assert(ts.isPropertyAccessExpression(target)||ts.isElementAccessExpression(target));assert.equal(ts.isPropertyAccessExpression(target)?target.name.text:target.argumentExpression.text,expected);const receiver=target.expression;assert(temporary?ts.isIdentifier(receiver)&&receiver.text===temporary:receiver.kind===ts.SyntaxKind.ThisKeyword);return {node:store,key:ts.isPropertyAccessExpression(target)?target.name:target.argumentExpression,value:store.right,receiver,operation:store.operatorToken,descriptor:null};}
    assert(ts.isCallExpression(store)&&store.expression.getText(view.source)==='Object.defineProperty'&&store.arguments.length===3);const [receiver,key,descriptor]=store.arguments;assert(temporary?ts.isIdentifier(receiver)&&receiver.text===temporary:receiver.kind===ts.SyntaxKind.ThisKeyword);assert(ts.isStringLiteral(key)&&key.text===expected&&ts.isObjectLiteralExpression(descriptor));const properties=new Map(descriptor.properties.map(property=>{assert(ts.isPropertyAssignment(property));return [property.name.getText(view.source),property.initializer];}));assert.equal(properties.size,4);for(const flag of['enumerable','configurable','writable'])assert.equal(properties.get(flag)?.kind,ts.SyntaxKind.TrueKeyword);assert(properties.has('value'));return {node:store,key,value:properties.get('value'),receiver,descriptor};
  });
  let constructor=null;
  if(options.target==='ES5'){assert(ts.isCallExpression(classValue));const inner=unwrap(ts,classValue.expression);assert(ts.isFunctionExpression(inner));const constructors=inner.body.statements.filter(ts.isFunctionDeclaration);assert.equal(constructors.length,1);constructor=constructors[0];}
  else assert(ts.isClassExpression(classValue));
  return {returned,node:value,classValue,classSyntax,temporary,factory,captured,nameSetter,fields,stores:records,constructor,transportDeclaration,returnedBinding,captureStatement};
}
function compare(ts,native,nReturn,actual,aReturn,original,options,contract) {
  const n=read(ts,native,nReturn,original,false,options),a=read(ts,actual,aReturn,original,true,options);let classProof=null;const phases=[];
  function uniform(view,start,end,owner,label) {
    for(let offset=start;offset<end;offset++)assert.deepEqual(view.owner(offset),owner,label+' every UTF-16 unit');
    view.cover(start,end);(view.roleStarts??=new Map()).set(start,owner);(view.roleEnds??=new Map()).set(end,view.owner(end));
  }
  function endpoint(nn,an,label){assert.deepEqual(actual.owner(an.end),native.owner(nn.end),label+' logical endpoint');}
  if(!a.factory&&!n.factory&&!n.temporary) {
    contract.pair(native.tokens(n.node),actual.tokens(a.node),'complete retained source class expression');endpoint(n.node,a.node,'retained class expression');
  }else {
    assert(n.temporary&&a.factory&&a.temporary,'paired Native static-state capture and complete evaluation factory');
    assert(a.nameSetter,'anonymous evaluation factory has exactly its contextual-name setter');
    const inferred=options.target==='ES5'?n.constructor.name.text:n.temporary;
    assert.equal(a.nameSetter.arguments[2].properties[0].initializer.text,inferred,'Native inferred anonymous function name');
    if(options.target==='ES5') {
      classProof=classes.compareClass(ts,native,n.classValue,actual,a.classValue,original,{...options,classRoles:{generatedBindings:{native:n.constructor,actual:a.constructor}}});
      for(const [view,item]of [[native,n],[actual,a]])for(const token of view.tokens(item.classSyntax).filter(token=>!view.covered(token.start))) {
        assert(token.text==='('||token.text===')','only enclosing class-value grouping remains after the complete constructor proof');
        const owner=token.text==='('?native.owner(classProof.native.factory.getStart(native.source)):native.owner(classProof.native.call.arguments.at(-1).end);
        uniform(view,token.start,token.end,owner,'complete class-value outer grouping');
      }
    }else {
      const strip=(view,item)=>view.tokens(item.classValue).filter(token=>!item.classValue.name||token.start!==item.classValue.name.getStart(view.source));
      if(a.classValue.name) {
        assert(a.classValue.name.text===a.temporary&&!original.name,'fresh anonymous class-expression self binding');
        const keyword=actual.tokens(a.classValue).find(token=>token.text==='class'),heritage=actual.tokens(a.classValue).find(token=>token.text==='extends');assert(keyword&&heritage);
        actual.noSource(keyword.end,heritage.start,'complete generated anonymous self-binding and its formatting');
      }
      contract.pair(strip(native,n),strip(actual,a),'complete moved class-value header/heritage/body');
    }
    const prefixOwner=native.owner(n.node.getStart(native.source));
    uniform(native,n.node.getStart(native.source),n.classSyntax.getStart(native.source),prefixOwner,'Native per-evaluation class capture prefix');
    uniform(actual,a.node.getStart(actual.source),a.classSyntax.getStart(actual.source),prefixOwner,'per-evaluation class factory capture prefix');
    actual.noSource(a.nameSetter.parent.getStart(actual.source),a.nameSetter.parent.end,'complete generated contextual-name descriptor ABI');
    assert.equal(a.stores.length,n.stores.length);const aliases=new Map([[a.temporary,n.temporary]]);
    for(let index=0;index<n.stores.length;index++) {
      contract.pair(native.tokens(n.stores[index].node),actual.tokens(a.stores[index].node),'complete moved static initialization '+index,aliases);
      endpoint(n.stores[index].node,a.stores[index].node,'moved static initialization '+index);
    }
    native.noSource(n.returnedBinding.getStart(native.source),n.node.end,'Native final captured class value');
    actual.noSource(a.returnedBinding.getStart(actual.source),a.node.end,'complete generated class-value return and invocation');
    const nt=native.tokens(n.node).filter(token=>!native.covered(token.start)),at=actual.tokens(a.node).filter(token=>!actual.covered(token.start));
    assert.equal(nt.length,n.fields.length+1,'complete Native class-expression comma separators');assert.equal(at.length,nt.length,'complete factory class/static statement separators');
    for(let index=0;index<nt.length;index++) {
      assert.equal(nt[index].text,',');assert.equal(at[index].text,';');const owner=nt[index].owners[0];assert(nt[index].owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)));
      uniform(native,nt[index].start,nt[index].end,owner,'Native class-value phase separator');uniform(actual,at[index].start,at[index].end,owner,'class-value phase statement separator');assert.deepEqual(actual.owner(at[index].end),native.owner(nt[index].end),'complete class-value phase separator logical endpoint');phases.push({native:nt[index],actual:at[index]});
      native.roleEnds.set(nt[index].end,native.owner(nt[index].end));actual.roleEnds.set(at[index].end,native.owner(nt[index].end));(actual.nativeEnds??=new Map()).set(at[index].end,nt[index].end);
    }
  }
  const complete=contract.region(n.node,a.node);
  return {native:n,actual:a,original,classProof,complete,phases,extraNativeNodes:n.transportDeclaration?[n.transportDeclaration]:[],pairedExpression:{source:original,native:n.node,actual:a.node},scope:'complete original class-value expression including generated evaluation/name/static/constructor phases'};
}
module.exports={read,compare,unwrap};
