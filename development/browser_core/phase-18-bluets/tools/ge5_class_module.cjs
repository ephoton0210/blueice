// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const envelope=require('./ge5_module_envelope.cjs'),classes=require('./ge5_class_roles.cjs'),accounting=require('./ge5_class_accounting.cjs');
function compare(ts,native,actual,options,originalText,delegates={}) {
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
  if(native.code===actual.code)assert.deepEqual(actual.decoded,native.decoded,'identical JavaScript complete decoded segments');
  const original=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true),originalNames=new Set();
  function names(node){if(ts.isIdentifier(node))originalNames.add(node.text);ts.forEachChild(node,names);}names(original);
  const dependencies=delegates.dependencies??(options.importHelpers&&options.target==='ES5'?['tslib']:[]);
  const n=envelope.read(ts,native,options,{originals:[original],dependencies}),a=envelope.read(ts,actual,options,{blue:true,originals:[original],dependencies});
  n.classifyTransport();a.classifyTransport();const contract=accounting.create(ts,native,actual,[originalText],{erasedImportComments:delegates.erasedImportComments??[]}),aliases=new Map(),proofs=[];
  const sources=original.statements.filter(ts.isClassDeclaration);
  const runtimeClasses=[];
  function runtimeClassInventory(node) {
    if(ts.isInterfaceDeclaration(node)||ts.isTypeAliasDeclaration(node)
      ||ts.canHaveModifiers(node)&&ts.getModifiers(node)?.some(modifier=>modifier.kind===ts.SyntaxKind.DeclareKeyword))return;
    if(ts.isClassDeclaration(node)||ts.isClassExpression(node))runtimeClasses.push(node);
    ts.forEachChild(node,runtimeClassInventory);
  }
  if(!original.isDeclarationFile)runtimeClassInventory(original);
  function nodeFor(item,name) {
    const delegated=delegates.classNode?.(item,name);if(delegated)return delegated;
    const nodes=item.sourceNodes.filter(node=>!coveredNode(item,node)).filter(node=>ts.isClassDeclaration(node)&&node.name?.text===name||ts.isVariableStatement(node)&&node.declarationList.declarations.some(d=>ts.isIdentifier(d.name)&&d.name.text===name&&d.initializer)||ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&node.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(node.expression.left)&&node.expression.left.text===name);
    assert.equal(nodes.length,1,'complete class binding '+name);return nodes[0];
  }
  const nativeHelperSource=ts.transpileModule('class A{};class B extends A{};let values=[...input];',{compilerOptions:{target:ts.ScriptTarget.ES5,module:ts.ModuleKind.CommonJS,downlevelIteration:true}}).outputText;
  const nativeHelpers=ts.createSourceFile('native-helpers.js',nativeHelperSource,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  const helperSpecs=new Map(nativeHelpers.statements.filter(ts.isVariableStatement).flatMap(node=>node.declarationList.declarations.filter(d=>ts.isIdentifier(d.name)&&d.name.text.startsWith('__')).map(d=>[d.name.text,node.getText(nativeHelpers)])));
  const assets=['class_helpers.v1.js','spread_helpers.v1.js','iteration_helpers.v1.js'].flatMap(file=>{
    const source=ts.createSourceFile(file,fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets',file),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
    return source.statements.filter(ts.isFunctionDeclaration).map(node=>[node.name.text,node.getText(source)]);
  });
  const assetMap=new Map(assets),canonical=name=>name.replace(/^(__blueice_target_(?:inherit_class|call_base|spread_array|spread_call|spread_construct))_\d+$/,'$1').replace(/^__blueice_target_spread_iterator_\d+$/,'__blueice_target_iterator').replace(/^__blueice_target_spread_step_\d+$/,'__blueice_target_step_result');
  const helpers=new Map();
  function generated(item,blue) {
    const libraryNamespaces=new Map(),sourceImports=new Map(),slots=new Set(),seenHelpers=new Set();
    if(item.format==='system')for(const [index,setter]of item.setterNodes.entries()) {
      if(coveredNode(item,setter))continue;
      const dependency=item.dependencies[index],parameter=setter.parameters[0].name.text,assignment=setter.body.statements[blue?1:0];
      assert(ts.isExpressionStatement(assignment)&&ts.isBinaryExpression(assignment.expression)&&ts.isIdentifier(assignment.expression.left));
      const binding=assignment.expression.left.text;assert(!originalNames.has(binding),'fresh System dependency namespace');libraryNamespaces.set(dependency,binding);
      item.exact(setter,blue?`function(${parameter}){${item.system.stem}_imports[${JSON.stringify(dependency)}]=${parameter};${binding}=${parameter};${item.system.stem}_refresh();}`:`function(${parameter}){${binding}=${parameter};}`);
      item.view.noSource(setter.getStart(item.view.source),setter.end,'complete generated System dependency setter');
    }
    const helperSlots=new Set();
    if(blue&&item.format==='system')for(const node of item.sourceNodes) {
      if(!ts.isExpressionStatement(node)||!ts.isBinaryExpression(node.expression))continue;
      const expression=node.expression;
      if(!ts.isIdentifier(expression.left)||!/^__blueice_target_inherit_class_\d+$/.test(expression.left.text))continue;
      const name=expression.left.text;
      assert(options.importHelpers&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isPropertyAccessExpression(expression.right)&&expression.right.name.text==='__extends');
      assert([...libraryNamespaces.values()].includes(expression.right.expression.getText(item.view.source)));assert(!originalNames.has(name)&&!helpers.has(name));helperSlots.add(name);helpers.set(name,'__blueice_target_inherit_class');
      item.exact(node,`${name}=${expression.right.expression.getText(item.view.source)}.__extends;`);item.view.noSource(node.getStart(item.view.source),node.end,'complete System imported inheritance binding');
    }
    const rootBindings=new Set(original.statements.flatMap(node=>ts.isClassDeclaration(node)||ts.isFunctionDeclaration(node)?[node.name.text]:ts.isVariableStatement(node)?node.declarationList.declarations.map(d=>d.name.getText(original)):[]));
    const exported=new Set(original.statements.flatMap(node=>node.modifiers?.some(m=>m.kind===ts.SyntaxKind.ExportKeyword)?ts.isClassDeclaration(node)||ts.isFunctionDeclaration(node)?[node.name.text]:ts.isVariableStatement(node)?node.declarationList.declarations.map(d=>d.name.getText(original)):[]:[]));
    for(const node of item.sourceNodes) {
      if(coveredNode(item,node))continue;
      if(item.format==='system'&&ts.isVariableStatement(node)&&node.declarationList.declarations.every(d=>ts.isIdentifier(d.name)&&!d.initializer&&(rootBindings.has(d.name.text)||helperSlots.has(d.name.text)||[...libraryNamespaces.values()].includes(d.name.text)))) {
        item.view.noSource(node.getStart(item.view.source),node.end,'complete original/transport hoisted binding inventory');continue;
      }
      if(item.format==='system'&&!blue&&ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&node.declarationList.declarations[0].name.getText(item.view.source)==='__moduleName') {
        item.exact(node,`var __moduleName=${item.system.context}&&${item.system.context}.id;`);item.view.noSource(node.getStart(item.view.source),node.end,'generated System module name');continue;
      }
      if(item.format==='system'&&blue&&ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)) {
        const expression=node.expression;
        if(expression.expression.getText(item.view.source)===item.system.stem+'_slot') {
          assert.equal(expression.arguments.length,1);assert(ts.isStringLiteral(expression.arguments[0])&&exported.has(expression.arguments[0].text));assert(!slots.has(expression.arguments[0].text),'one slot for each original export');slots.add(expression.arguments[0].text);item.exact(node,`${item.system.stem}_slot(${JSON.stringify(expression.arguments[0].text)});`);item.view.noSource(node.getStart(item.view.source),node.end,'complete generated System publication slot');continue;
        }
        if(expression.expression.getText(item.view.source)===item.system.stem+'_refresh') {item.exact(node,`${item.system.stem}_refresh();`);item.view.noSource(node.getStart(item.view.source),node.end,'generated System publication refresh');continue;}
      }
      if(item.format==='system'&&ts.isExpressionStatement(node)) {
        if(!blue&&ts.isCallExpression(node.expression)&&node.expression.expression.getText(item.view.source)===item.system.publisher&&node.expression.arguments.length===2) {
          const [name,value]=node.expression.arguments;if(ts.isStringLiteral(name)&&ts.isIdentifier(value)&&sources.some(source=>source.name.text===name.text)) {
            assert(exported.has(name.text)&&ts.isIdentifier(value)&&value.text===name.text);item.exact(node,`${item.system.publisher}(${JSON.stringify(name.text)},${name.text});`);item.view.noSource(node.getStart(item.view.source),node.end,'generated System class publication');continue;
          }
        }
        if(blue&&ts.isBinaryExpression(node.expression)&&ts.isPropertyAccessExpression(node.expression.left)&&node.expression.left.expression.getText(item.view.source)==='exports'&&sources.some(source=>source.name.text===node.expression.left.name.text)) {
          const name=node.expression.left.name.text;assert(exported.has(name)&&ts.isIdentifier(node.expression.right)&&node.expression.right.text===name);item.exact(node,`exports.${name}=${name};`);item.view.noSource(node.getStart(item.view.source),node.end,'generated System class publication');continue;
        }
      }
      if(ts.isFunctionDeclaration(node)&&!originalNames.has(node.name?.text)) {
        assert(blue,'Native helpers use reviewed variable declarations');const key=canonical(node.name.text),specification=assetMap.get(key);assert(specification,'reviewed class/spread helper algorithm');
        assert(!helpers.has(node.name.text),'unique generated helper binding');const bindings=new Map(item.view.tokens(node).filter(t=>t.text.startsWith('__blueice_target_')).map(t=>[t.text,canonical(t.text)]));item.exact(node,specification,bindings);helpers.set(node.name.text,key);item.view.noSource(node.getStart(item.view.source),node.end,'complete generated class/spread helper');continue;
      }
      if(ts.isImportDeclaration(node)&&ts.isStringLiteral(node.moduleSpecifier)&&node.moduleSpecifier.text==='tslib') {
        assert(options.importHelpers&&options.target==='ES5');
        if(blue) {assert(ts.isNamespaceImport(node.importClause.namedBindings));const binding=node.importClause.namedBindings.name.text;assert(!originalNames.has(binding));item.exact(node,`import * as ${binding} from 'tslib';`);libraryNamespaces.set('tslib',binding);}
        else item.exact(node,`import {__extends} from 'tslib';`);
        item.view.noSource(node.getStart(item.view.source),node.end,'generated helper import');continue;
      }
      if(ts.isVariableStatement(node)&&node.declarationList.declarations.length===1) {
        const declaration=node.declarationList.declarations[0];if(!ts.isIdentifier(declaration.name))continue;
        const name=declaration.name.text,initializer=declaration.initializer;
        if(!blue&&helperSpecs.has(name)) {assert(!seenHelpers.has(name),'unique complete Native helper');seenHelpers.add(name);item.exact(node,helperSpecs.get(name));item.view.noSource(node.getStart(item.view.source),node.end,'complete Native class/spread helper');continue;}
        if(initializer&&ts.isCallExpression(initializer)&&initializer.expression.getText(item.view.source)==='require'&&initializer.arguments.length===1&&ts.isStringLiteral(initializer.arguments[0])) {
          const dependency=initializer.arguments[0].text;assert(dependency==='tslib'||original.statements.some(source=>ts.isImportDeclaration(source)&&source.moduleSpecifier.text===dependency));assert(!originalNames.has(name),'fresh imported namespace');
          item.exact(node,`${options.target==='ES5'?'var':'const'} ${name}=require(${JSON.stringify(dependency)});`);libraryNamespaces.set(dependency,name);if(dependency==='tslib')item.view.noSource(node.getStart(item.view.source),node.end,'generated helper namespace import');else sourceImports.set(dependency,node);continue;
        }
        if(blue&&/^__blueice_target_inherit_class_\d+$/.test(name)) {
          assert(options.importHelpers&&initializer&&ts.isPropertyAccessExpression(initializer)&&initializer.name.text==='__extends');assert([...libraryNamespaces.values(),...item.parameterBindings.map(p=>p.binding)].includes(initializer.expression.getText(item.view.source)));item.exact(node,`var ${name}=${initializer.expression.getText(item.view.source)}.__extends;`);helpers.set(name,'__blueice_target_inherit_class');item.view.noSource(node.getStart(item.view.source),node.end,'imported inheritance helper binding');continue;
        }
      }
      if(ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)) {
        let expression=node.expression;const names=[];
        while(ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isPropertyAccessExpression(expression.left)&&expression.left.expression.getText(item.view.source)==='exports') {
          names.push(expression.left.name.text);expression=expression.right;
        }
        if(ts.isVoidExpression(expression)&&ts.isNumericLiteral(expression.expression)&&expression.expression.text==='0') {
          assert.equal(new Set(names).size,names.length,'unique CommonJS predeclared exports');assert.deepEqual(names.slice().sort(),[...exported].sort(),'complete CommonJS predeclared export inventory');item.view.noSource(node.getStart(item.view.source),node.end,'generated CommonJS export predeclarations');continue;
        }
      }
      if(ts.isExpressionStatement(node)&&ts.isStringLiteral(node.expression)) {item.exact(node,`'use strict';`);item.view.noSource(node.getStart(item.view.source),node.end,'module strict directive');continue;}
      if(ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText(item.view.source)==='Object.defineProperty'&&ts.isIdentifier(node.expression.arguments[0])&&node.expression.arguments[0].text==='exports') {item.exact(node,'Object.defineProperty(exports,"__esModule",{value:true});');item.view.noSource(node.getStart(item.view.source),node.end,'CommonJS module marker');continue;}
    }
    if(blue&&item.format==='system')assert.deepEqual([...slots].sort(),[...exported].sort(),'complete System publication slot inventory');
    return {namespaces:libraryNamespaces,sourceImports};
  }
  function coveredNode(item,node){for(let offset=node.getStart(item.view.source);offset<node.end;offset++)if(!item.view.covered(offset))return false;return true;}
  const delegated=delegates.prepare?.({native,actual,n,a,contract,original});
  const ni=generated(n,false),ai=generated(a,true);
  const constructions=require('./ge5_class_spread.cjs').compare(ts,native,actual,n,a,original,options,helpers,contract);
  const firstUser=item=>item.sourceNodes.find(node=>!item.view.covered(node.getStart(item.view.source)));
  const nf=firstUser(n),af=firstUser(a);assert(nf&&af,'complete original module entry');
  const generatedRanges=[];
  for(let offset=0;offset<af.getStart(actual.source);) {
    if(!actual.covered(offset)){offset++;continue;}const start=offset;while(offset<af.getStart(actual.source)&&actual.covered(offset))offset++;generatedRanges.push([start,offset]);
  }
  require('./ge5_role_stream.cjs').compareGap(native,0,nf.getStart(native.source),actual,0,af.getStart(actual.source),'complete Native-derived module entry gaps',generatedRanges);
  for(const [dependency,node]of ai.sourceImports){assert(ni.sourceImports.has(dependency));aliases.set(ai.namespaces.get(dependency),ni.namespaces.get(dependency));contract.pair(native.tokens(ni.sourceImports.get(dependency)),actual.tokens(node),'complete source namespace import',aliases);}
  if(n.format==='system') {
    const exportedVariables=original.statements.filter(node=>ts.isVariableStatement(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)).flatMap(node=>[...node.declarationList.declarations]);
    for(const declaration of exportedVariables) {
      assert(ts.isIdentifier(declaration.name));const name=declaration.name.text;
      const ns=n.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText(native.source)===n.system.publisher&&node.expression.arguments.length===2&&ts.isStringLiteral(node.expression.arguments[0])&&node.expression.arguments[0].text===name);
      const as=a.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isPropertyAccessExpression(node.expression.left)&&node.expression.left.expression.getText(actual.source)==='exports'&&node.expression.left.name.text===name);assert.equal(ns.length,1);assert.equal(as.length,1);
      const nr=ns[0],ar=as[0],assignment=nr.expression.arguments[1];assert(ts.isBinaryExpression(assignment)&&assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(assignment.left)&&assignment.left.text===name);
      const prefixOwner=native.owner(nr.getStart(native.source));
      function uniform(view,start,end,owner,label){for(let offset=start;offset<end;offset++)assert.deepEqual(view.owner(offset),owner,label+' complete transport UTF-16');view.cover(start,end);(view.roleEnds??=new Map()).set(end,owner);}
      uniform(native,nr.getStart(native.source),assignment.left.getStart(native.source),prefixOwner,'Native System variable publication prefix');
      uniform(actual,ar.getStart(actual.source),ar.expression.left.name.getStart(actual.source),prefixOwner,'System variable publication prefix');
      contract.pair([native.token(assignment.left),native.token(assignment.operatorToken)],[actual.token(ar.expression.left.name),actual.token(ar.expression.operatorToken)],'complete System published variable binding');
      const endpoint=native.owner(assignment.right.end);
      uniform(native,assignment.right.end,nr.end,endpoint,'Native variable publication completion');uniform(actual,ar.expression.right.end,ar.end,endpoint,'variable publication completion');
      native.roleEnds.set(nr.end,native.owner(nr.end));actual.roleEnds.set(ar.end,native.owner(nr.end));
    }
  }
  if(options.target==='ES5') {
    const kinds=[...helpers.values()],derived=runtimeClasses.filter(source=>source.heritageClauses?.length).length;
    assert.equal(kinds.filter(kind=>kind==='__blueice_target_inherit_class').length,options.importHelpers?derived:options.noEmitHelpers?0:runtimeClasses.length,'complete inheritance helper group inventory');
    assert.equal(kinds.filter(kind=>kind==='__blueice_target_call_base').length,options.importHelpers||options.noEmitHelpers?0:runtimeClasses.length,'complete constructor helper group inventory');
  }
  for(const source of sources) {
    const nn=nodeFor(n,source.name.text),an=nodeFor(a,source.name.text);let relocated=null;
    const classRoles=delegates.classRoles?.(source,nn,an)??{};
    const delegatedClass=delegates.compareClass?.({source,nn,an,options,classRoles,contract,n,a,helpers});
    if(delegatedClass) {
      assert.equal(delegatedClass.source,source,'delegated class belongs to this exact original AST');
      for(const [view,node]of [[native,nn],[actual,an]]) {
        assert.equal(node.getSourceFile(),view.source,'delegated complete class binding belongs to its exact emitted AST');
        for(let offset=node.getStart(view.source);offset<node.end;offset++)assert(view.covered(offset),'delegated class binding must already prove every UTF-16 character');
      }
      proofs.push(delegatedClass);continue;
    }
    if(source.heritageClauses?.length) {
      const sourceHeritage=source.heritageClauses[0].types[0].expression,emitted=classes.readClass(ts,actual,an,source,{excludedNodes:classRoles.actualExcluded??[]});
      const inherited=emitted.native?emitted.node.heritageClauses[0].types[0].expression:emitted.call.arguments[0];
      if(ts.isIdentifier(inherited)&&!ts.isIdentifier(sourceHeritage)) {
        const candidates=a.sourceNodes.filter(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&ts.isIdentifier(node.declarationList.declarations[0].name)&&node.declarationList.declarations[0].name.text===inherited.text);
        assert.equal(candidates.length,1,'one captured heritage declaration');assert(!originalNames.has(inherited.text),'fresh captured heritage binding');const statement=candidates[0],declaration=statement.declarationList.declarations[0];assert(declaration.initializer);relocated=declaration.initializer;
        actual.noSource(statement.getStart(actual.source),relocated.getStart(actual.source),'generated captured heritage declaration prefix');actual.noSource(relocated.end,statement.end,'generated captured heritage declaration completion');
      }
    }
    proofs.push(classes.compareClass(ts,native,nn,actual,an,source,{...options,classRoles:{...classRoles,helpers,actualHeritageExpression:relocated??classRoles.actualHeritageExpression}}));
  }
  const remaining=view=>view.tokens(view.source).filter(token=>!view.covered(token.start));
  contract.pair(remaining(native),remaining(actual),'complete retained module statements',aliases);contract.finish([...proofs,...(delegated?.classProofs??[])]);
  return {native:n,actual:a,classes:proofs,helpers,delegated,constructions};
}
module.exports={compare};
