// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independently identify module bindings and canonical rest/copy operations.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const envelope=require('./ge5_module_envelope.cjs'),{compareTokens,compareGap}=require('./ge5_role_stream.cjs');
function compare(ts,native,actual,options,original) {
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
  if(actual.code===native.code)assert.deepEqual(actual.decoded,native.decoded);
  const lowered=!['ES2018','ES2022'].includes(options.target),rest=lowered||options.module==='System',assign=options.target==='ES5';
  const members=[...(assign?['__assign']:[]),...(rest?['__rest']:[])],dependencies=options.importHelpers&&lowered?['tslib']:[];
  const originalSource=ts.createSourceFile('original.ts',original,ts.ScriptTarget.Latest,true),identifiers=new Set();
  function collect(node){if(ts.isIdentifier(node))identifiers.add(node.text);ts.forEachChild(node,collect);}collect(originalSource);
  function read(view,blue) {
    const item=envelope.read(ts,view,options,{blue,originals:[original],dependencies}),user=[],helpers=new Map(),generated=[],slots=new Set();
    let namespace=null,imported=false;
    function fresh(name){assert(!identifiers.has(name),'fresh module helper binding');return name;}
    function importedNamespace(name){fresh(name);assert(namespace===null||namespace===name);namespace=name;imported=true;}
    for(const binding of item.parameterBindings)if(binding.binding)importedNamespace(binding.binding);
    if(item.format==='system'&&dependencies.length) {
      const setter=item.setterNodes[0],statements=setter.body.statements,assignment=statements[blue?1:0];assert(ts.isExpressionStatement(assignment)&&ts.isBinaryExpression(assignment.expression)&&ts.isIdentifier(assignment.expression.left));
      importedNamespace(assignment.expression.left.text);const parameter=setter.parameters[0].name.text;
      item.exact(setter,blue?`function(${parameter}){${item.system.stem}_imports['tslib']=${parameter};${namespace}=${parameter};${item.system.stem}_refresh();}`:`function(${parameter}){${namespace}=${parameter};}`);generated.push(setter);
    }
    const helperAsset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/object_spread_helpers.v1.js'),'utf8');
    function classify(node,prelude=false) {
      if(ts.isFunctionDeclaration(node)) {
        assert(blue&&/^__blueice_target_(?:rest_properties|object_properties)_\d+$/.test(node.name?.text),'reviewed object helper binding');fresh(node.name.text);
        const role=node.name.text.includes('rest_properties')?'rest':'assign';assert(![...helpers.values()].includes(role));item.exact(node,helperAsset.replaceAll('__blueice_target_object_properties',node.name.text));helpers.set(node.name.text,role);generated.push(node);return;
      }
      if(ts.isImportDeclaration(node)) {
        assert(dependencies.length&&!imported&&ts.isStringLiteral(node.moduleSpecifier)&&node.moduleSpecifier.text==='tslib');
        if(blue){assert(ts.isNamespaceImport(node.importClause.namedBindings));importedNamespace(node.importClause.namedBindings.name.text);item.exact(node,`import * as ${namespace} from 'tslib';`);}
        else {assert(ts.isNamedImports(node.importClause.namedBindings));assert.deepEqual(node.importClause.namedBindings.elements.map(e=>{assert(!e.propertyName);return e.name.text;}),members);item.exact(node,`import {${members.join(',')}} from 'tslib';`);imported=true;}generated.push(node);return;
      }
      if(ts.isExpressionStatement(node)&&ts.isStringLiteral(node.expression)){item.exact(node,`'use strict';`);generated.push(node);return;}
      if(ts.isVariableStatement(node)) {
        const declarations=node.declarationList.declarations;
        if(declarations.length===1) {
          const d=declarations[0],init=d.initializer,name=d.name.getText();
          if(['__assign','__rest'].includes(name)) {assert(!blue&&!options.importHelpers&&!options.noEmitHelpers&&members.includes(name));assert(!helpers.has(name));helpers.set(name,name==='__rest'?'rest':'assign');generated.push(node);return;}
          if(init&&ts.isCallExpression(init)&&init.expression.getText()==='require') {
            assert(dependencies.length&&!imported&&ts.isIdentifier(d.name));importedNamespace(name);assert.equal(init.arguments.length,1);assert(ts.isStringLiteral(init.arguments[0])&&init.arguments[0].text==='tslib');
            const keyword=blue||options.target==='ES5'?'var':'const';item.exact(node,`${keyword} ${name}=require('tslib');`);generated.push(node);return;
          }
          if(prelude&&name==='__moduleName'){assert(!blue);item.exact(node,`var __moduleName=${item.system.context}&&${item.system.context}.id;`);generated.push(node);return;}
        }
        if(prelude){for(const d of declarations){assert(ts.isIdentifier(d.name)&&!d.initializer);const name=d.name.text;assert(['source','a','remaining','copy',...(!blue?['result']:[]),...(namespace?[namespace]:[])].includes(name),'complete hoisted object binding');assert(!slots.has(name));slots.add(name);}generated.push(node);return;}
      }
      if(ts.isExpressionStatement(node)) {
        const e=node.expression;
        if(ts.isCallExpression(e)&&e.expression.getText()==='Object.defineProperty'){item.exact(node,'Object.defineProperty(exports,"__esModule",{value:true});');generated.push(node);return;}
        if(ts.isBinaryExpression(e)&&ts.isPropertyAccessExpression(e.left)&&e.left.getText()==='exports.result'&&ts.isVoidExpression(e.right)){item.exact(node,'exports.result=void 0;');generated.push(node);return;}
        if(blue&&item.format==='system'&&ts.isCallExpression(e)&&e.expression.getText()===item.system.stem+'_slot'){assert(prelude);item.exact(node,`${item.system.stem}_slot('result');`);generated.push(node);return;}
        if(blue&&item.format==='system'&&ts.isCallExpression(e)&&e.expression.getText()===item.system.stem+'_refresh'){item.exact(node,`${item.system.stem}_refresh();`);generated.push(node);return;}
      }
      assert(!prelude,'no original statement hidden in generated prelude');user.push(node);
    }
    for(const node of item.prelude)classify(node,item.format==='system');
    for(const node of item.body)classify(node);
    assert.equal(imported,dependencies.length>0,'complete helper import dependency');
    assert.equal(user.length,4,'complete object module statement inventory');
    if(item.format==='system')assert.deepEqual([...slots].sort(),['source','a','remaining','copy',...(!blue?['result']:[]),...(namespace?[namespace]:[])].sort(),'complete System hoisted bindings');
    if(!options.importHelpers&&!options.noEmitHelpers&&lowered)assert.deepEqual([...helpers.values()].sort(),[...((blue||assign)?['assign']:[]),'rest'].sort(),'whole private helper inventory');else assert.equal(helpers.size,0);
    item.classifyTransport();for(const node of generated)view.noSource(node.getStart(view.source),node.end,'complete generated object helper/import/header');
    return {...item,user,helpers,namespace,generated,blue};
  }
  const n=read(native,false),a=read(actual,true),extra=new Map([[native,[]],[actual,[]]]);
  function uniform(view,start,end,owner,label){for(let i=start;i<end;i++)assert.deepEqual(view.owner(i),owner,label+' at '+i);view.cover(start,end);}
  function helper(item,node) {
    const name=node.getText(item.view.source);if(item.helpers.has(name))return item.helpers.get(name);
    if(ts.isPropertyAccessExpression(node)&&node.expression.getText()===item.namespace)return node.name.text==='__rest'?'rest':node.name.text==='__assign'?'assign':null;
    if(ts.isPropertyAccessExpression(node)&&node.expression.getText()==='Object'&&node.name.text==='assign')return 'assign';
    if(ts.isIdentifier(node)&&['__rest','__assign'].includes(node.text)&&(!dependencies.length||!item.namespace))return node.text==='__rest'?'rest':'assign';
    return null;
  }
  function canonical(item,node) {
    const view=item.view,replacements=[],removed=[],calls={rest:0,assign:0};
    function generated(start,end){view.noSource(start,end,'generated object invocation transport');removed.push([start,end]);extra.get(view).push([start,end]);}
    function visit(node) {
      if(ts.isCallExpression(node)) {
        if(item.format==='system'&&!item.blue&&node.expression.getText()===item.system.publisher){assert.equal(node.arguments.length,2);assert(ts.isStringLiteral(node.arguments[0])&&node.arguments[0].text==='result');const assignment=node.arguments[1];assert(ts.isBinaryExpression(assignment)&&assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken&&assignment.left.getText()==='result');visit(assignment.right);return;}
        const role=helper(item,node.expression);assert(role,'complete reviewed object call inventory');calls[role]++;
        const expression=node.expression,start=expression.getStart(view.source),owner=view.owner(start);assert(owner);uniform(view,start,expression.end,owner,'complete helper reference interior');
        replacements.push({start,end:expression.end,token:{text:'$'+role,start,end:expression.end,owners:Array(role.length+1).fill(owner)}});
        const args=node.arguments,privateHelper=item.blue&&item.helpers.has(expression.getText());
        if(role==='rest') {
          assert.equal(args.length,privateHelper?4:2);const operand=args[privateHelper?1:0],keys=args[privateHelper?3:1];assert(ts.isIdentifier(operand)&&operand.text==='source');assert(ts.isArrayLiteralExpression(keys)&&keys.elements.length===1&&ts.isStringLiteral(keys.elements[0])&&keys.elements[0].text==='a');
          if(privateHelper){assert(ts.isObjectLiteralExpression(args[0])&&!args[0].properties.length);assert.equal(args[2].kind,ts.SyntaxKind.TrueKeyword);generated(args[0].getStart(view.source),operand.getStart(view.source));const comma=view.tokens(node).find(t=>t.text===','&&operand.end<=t.start&&t.end<=args[2].getStart(view.source));assert(comma);generated(comma.end,keys.getStart(view.source));}
        } else {
          assert.equal(args.length,privateHelper?3:2);
          if(privateHelper){assert([ts.SyntaxKind.TrueKeyword,ts.SyntaxKind.FalseKeyword].includes(args[2].kind));const inner=ts.isObjectLiteralExpression(args[0])&&args[0].properties.length===0;assert.equal(args[2].kind,inner?ts.SyntaxKind.TrueKeyword:ts.SyntaxKind.FalseKeyword);generated(args[1].end,node.end-1);}
          assert(ts.isObjectLiteralExpression(args[0])&&!args[0].properties.length||ts.isCallExpression(args[0]));
        }
      }
      ts.forEachChild(node,visit);
    }
    visit(node);
    let result=view.tokens(node).filter(token=>!removed.some(([start,end])=>start<=token.start&&token.end<=end)&&!replacements.some(r=>r.start<=token.start&&token.end<=r.end));result.push(...replacements.map(r=>r.token));result.sort((x,y)=>x.start-y.start);
    if(item.format==='system'&&!item.blue&&ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText()===item.system.publisher) {
      const call=node.expression,assignment=call.arguments[1],name=assignment.left,start=call.getStart(view.source),owner=view.owner(start),nameStart=name.getStart(view.source);uniform(view,start,nameStart,owner,'Native System publication prefix');
      result=result.filter(token=>token.start>=nameStart&&token.end<=assignment.end||token.text===';'&&token.start>=call.end);
      result.unshift({text:'exports',start,end:nameStart,owners:Array(7).fill(owner)},{text:'.',start:nameStart,end:nameStart,owners:[owner]});
      uniform(view,assignment.end,call.end,view.owner(assignment.end),'Native System publication completion');
    }
    return {tokens:result,calls};
  }
  const roles=[];
  for(let index=0;index<4;index++) {
    const x=n.user[index],y=a.user[index],nx=canonical(n,x),ay=canonical(a,y);assert.deepEqual(ay.calls,nx.calls,'complete operation identity');assert.deepEqual(nx.calls,index===1?{rest:rest?1:0,assign:0}:index===2?{rest:0,assign:lowered?2:0}:{rest:0,assign:0});
    compareTokens(native,nx.tokens,actual,ay.tokens,'complete object source statement '+index,extra.get(actual));
    compareGap(native,index?n.user[index-1].end:0,x.getStart(native.source),actual,index?a.user[index-1].end:0,y.getStart(actual.source),'ordered object statement gap',extra.get(actual));roles.push({native:x,actual:y});
  }
  compareGap(native,n.user.at(-1).end,native.code.length,actual,a.user.at(-1).end,actual.code.length,'complete object trailing gaps');
  function comments(code){const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,code),out=[];for(let k=scanner.scan();k!==ts.SyntaxKind.EndOfFileToken;k=scanner.scan())if([ts.SyntaxKind.SingleLineCommentTrivia,ts.SyntaxKind.MultiLineCommentTrivia].includes(k))out.push({start:scanner.getTokenPos(),end:scanner.getTextPos(),text:scanner.getTokenText()});return out;}
  const originalComments=comments(original).map(c=>c.text),nc=comments(native.code).filter(c=>originalComments.includes(c.text)),ac=comments(actual.code).filter(c=>originalComments.includes(c.text));assert.deepEqual(nc.map(c=>c.text),originalComments);assert.deepEqual(ac.map(c=>c.text),originalComments);
  for(let i=0;i<nc.length;i++){assert.equal(ac[i].end-ac[i].start,nc[i].end-nc[i].start);for(let j=0;j<nc[i].end-nc[i].start;j++)assert.deepEqual(actual.owner(ac[i].start+j),native.owner(nc[i].start+j),'original comment interiors');native.cover(nc[i].start,nc[i].end);actual.cover(ac[i].start,ac[i].end);}
  for(const view of [native,actual]) {
    for(const c of comments(view.code))if(!originalComments.includes(c.text))view.noSource(c.start,c.end,'complete generated object comment');
    for(let offset=0;offset<view.code.length;offset++)if(!view.covered(offset)){assert(/\s/.test(view.code[offset]),'unclassified object code '+offset);assert.equal(view.owner(offset),null,'unclassified object gap '+offset);view.cover(offset,offset+1);}view.complete('whole object module');
  }
  assert.deepEqual(actual.owner(actual.code.length),native.owner(native.code.length));return {native:n,actual:a,roles};
}
module.exports={compare};
