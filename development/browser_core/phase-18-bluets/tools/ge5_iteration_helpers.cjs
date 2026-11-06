// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete helper groups, module headers and hoisted scopes around iterable code.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
function read(ts,view,item,options,original) {
  const originalSource=ts.createSourceFile('main.ts',original,ts.ScriptTarget.Latest,true),identifiers=new Set(),bindings=new Set();
  function names(name){if(ts.isIdentifier(name))bindings.add(name.text);else for(const element of name.elements)if(ts.isBindingElement(element))names(element.name);}
  function visit(node){if(ts.isIdentifier(node))identifiers.add(node.text);if(ts.isVariableDeclaration(node))names(node.name);ts.forEachChild(node,visit);}visit(originalSource);
  const generated=[],user=[],helpers=new Map(),hoisted=new Set(),helperFunctions=[];
  const es5=options.target==='ES5',privateHelpers=es5&&!options.importHelpers&&!options.noEmitHelpers;
  function noSource(node,label){view.noSource(node.getStart(view.source),node.end,label);generated.push(node);}
  for(const node of item.sourceNodes) {
    if(node===item.fn){user.push({kind:'generator',node});continue;}
    if(ts.isFunctionDeclaration(node)) {assert(item.blue&&privateHelpers);const match=/^__blueice_target_(generator|delegate_iterator|delegate_step|binding_read|binding_iterator|binding_step|iterator|iterator_result)_\d+$/.exec(node.name?.text);assert(match&&!identifiers.has(node.name.text));assert(!helpers.has(node.name.text));helpers.set(node.name.text,match[1]==='binding_read'?'read':match[1]==='iterator'?'values':match[1]);helperFunctions.push(node);noSource(node,'complete private iterator helper');continue;}
    if(ts.isImportDeclaration(node)){assert(item.reference&&options.importHelpers);noSource(node,'complete iterator helper import');continue;}
    if(ts.isVariableStatement(node)) {
      const declarations=node.declarationList.declarations;
      if(declarations.length===1){const declaration=declarations[0],name=declaration.name.getText(),init=declaration.initializer;
        if(['__generator','__read','__values'].includes(name)){assert(!item.blue&&privateHelpers&&!helpers.has(name));helpers.set(name,name==='__generator'?'generator':name==='__read'?'read':'values');noSource(node,'complete Native iterator helper');continue;}
        if(init&&ts.isCallExpression(init)&&init.expression.getText()==='require'){assert(item.reference&&options.importHelpers);noSource(node,'iterator helper namespace import');continue;}
        if(name==='__moduleName'&&item.format==='system'&&!item.blue){item.exact(node,`var __moduleName=${item.system.context}&&${item.system.context}.id;`);noSource(node,'generated module name');continue;}
      }
      if(declarations.every(declaration=>ts.isIdentifier(declaration.name)&&!declaration.initializer)) {
        for(const declaration of declarations){const name=declaration.name.text;assert(bindings.has(name)||!identifiers.has(name),'original hoisted variable or fresh iterator slot');assert(!hoisted.has(name));hoisted.add(name);}noSource(node,'complete hoisted iterator scope');continue;
      }
    }
    if(ts.isExpressionStatement(node)) {
      const expression=node.expression;
      if(ts.isStringLiteral(expression)){item.exact(node,`'use strict';`);noSource(node,'generated module strict directive');continue;}
      if(ts.isCallExpression(expression)&&expression.expression.getText()==='Object.defineProperty'){item.exact(node,'Object.defineProperty(exports,"__esModule",{value:true});');noSource(node,'generated CommonJS marker');continue;}
      if(ts.isBinaryExpression(expression)&&expression.left.getText()==='exports.result'&&ts.isVoidExpression(expression.right)){item.exact(node,'exports.result=void 0;');noSource(node,'generated export placeholder');continue;}
      if(item.blue&&item.format==='system'&&ts.isCallExpression(expression)&&expression.expression.getText()===item.system.stem+'_slot'){item.exact(node,`${item.system.stem}_slot('result');`);noSource(node,'generated System slot');continue;}
      if(item.blue&&item.format==='system'&&ts.isCallExpression(expression)&&expression.expression.getText()===item.system.stem+'_refresh'){item.exact(node,`${item.system.stem}_refresh();`);noSource(node,'generated System refresh');continue;}
    }
    user.push({kind:'statement',node});
  }
  if(item.blue&&privateHelpers) {
    assert.equal(helperFunctions.length,8,'whole generator/binding/iteration dependency groups');
    require('./ge5_binding_helpers.cjs').validate(ts,view,helperFunctions.filter(node=>!/^__blueice_target_iterator(?:_result)?_\d+$/.test(node.name.text)),identifiers);
    const iterations=helperFunctions.filter(node=>/^__blueice_target_iterator(?:_result)?_\d+$/.test(node.name.text));assert.equal(iterations.length,2);
    const iterator=iterations.find(node=>helpers.get(node.name.text)==='values'),step=iterations.find(node=>helpers.get(node.name.text)==='iterator_result');assert(iterator&&step);
    const asset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/iteration_helpers.v1.js'),'utf8').replaceAll('__blueice_target_iterator',iterator.name.text).replaceAll('__blueice_target_step_result',step.name.text),source=ts.createSourceFile('iterator-helper-abi.js',asset,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
    for(let index=0;index<2;index++)item.exact(iterations[index],source.statements[index].getText(source));
  } else assert.equal(helpers.size,privateHelpers?3:0,'complete inline helper inventory');
  assert.equal(user.length,6,'complete iterable module statement inventory');
  return {...item,user,helpers,generated,hoisted,identifiers,bindings};
}
function helper(ts,item,node,role) {
  const text=node.getText(item.view.source);if(item.helpers.get(text)===role)return true;
  const name=role==='read'?'__read':role==='values'?'__values':role==='generator'?'__generator':null;if(!name)return false;
  if(item.reference?.namespace)return ts.isPropertyAccessExpression(node)&&ts.isIdentifier(node.expression)&&node.expression.text===item.reference.namespace&&node.name.text===name;
  return ts.isIdentifier(node)&&node.text===name;
}
module.exports={read,helper};
