// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Strict module transport parsing shared by independent source-role contracts.
// Original statements, imports, exports and setter bindings remain caller roles.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
function read(ts,view,options,{blue=false,originals=[],dependencies=[]}={}) {
  const identifiers=new Set();
  for(const original of originals) {
    const source=typeof original==='string'?ts.createSourceFile('original.ts',original,ts.ScriptTarget.Latest,true):original;
    function visit(node){if(ts.isIdentifier(node))identifiers.add(node.text);ts.forEachChild(node,visit);}visit(source);
  }
  const text=node=>node.getText(view.source);
  const unwrap=node=>{while(ts.isParenthesizedExpression(node))node=node.expression;return node;};
  function tokens(node,aliases=new Map()) {
    return view.tokens(node).map(token=>{const scanner=ts.createScanner(ts.ScriptTarget.Latest,true,ts.LanguageVariant.Standard,token.text),kind=scanner.scan();return kind===ts.SyntaxKind.StringLiteral?JSON.stringify(scanner.getTokenValue()):aliases.get(token.text)||token.text;});
  }
  function words(code) {
    const anonymous=code.startsWith('function(');
    const source=ts.createSourceFile('module-transport.js',anonymous?'('+code+')':code,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS),result=[];
    assert.equal(source.parseDiagnostics.length,0,'valid envelope specification');
    function visit(node){if(ts.isToken(node)){if(node.kind!==ts.SyntaxKind.EndOfFileToken)result.push(ts.isStringLiteral(node)?JSON.stringify(node.text):node.getText(source));return;}for(const child of node.getChildren(source))visit(child);}visit(anonymous?source.statements[0].expression.expression:source);return result;
  }
  const exact=(node,code,aliases)=>assert.deepEqual(tokens(node,aliases),words(code),'complete module envelope transport');
  function fresh(name,label) {assert(ts.isIdentifier(name)&&!identifiers.has(name.text),label+' fresh transport binding');return name.text;}
  function plainFunction(node,parameters) {
    assert(ts.isFunctionExpression(node)&&!node.name&&!node.asteriskToken&&!node.modifiers?.length,'plain module transport function');
    assert.deepEqual(node.parameters.map(p=>{assert(ts.isIdentifier(p.name)&&!p.initializer&&!p.dotDotDotToken&&!p.questionToken&&!p.type&&!p.modifiers?.length);return p.name.text;}),parameters,'module transport parameters');return node;
  }
  function dependenciesOf(node) {
    assert(ts.isArrayLiteralExpression(node));return node.elements.map(element=>{assert(ts.isStringLiteral(element));return element.text;});
  }
  const holes=[],generated=[],wrapperTokens=[];
  let body=[...view.source.statements],prelude=[],format='plain',factory=null,execute=null,system=null;
  let setterNodes=[],parameterBindings=[];
  const module=String(options.module).toLowerCase();
  const wrapped=['amd','umd','system'].includes(module);
  if(wrapped) {
    // Inline Native helpers may precede AMD/UMD registration. Callers validate
    // their complete algorithms; they are never mistaken for the envelope.
    const registrations=body.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression));
    const candidates=registrations.filter(node=>module==='system'?text(node.expression.expression)==='System.register':module==='amd'?text(node.expression.expression)==='define':ts.isFunctionExpression(unwrap(node.expression.expression)));
    assert.equal(candidates.length,1,'one complete module registration');
    const registration=candidates[0];prelude=body.filter(node=>node!==registration);
    assert(!blue||prelude.length===0,'Blue wrapper has no unreviewed outer statements');
    const call=registration.expression;
    if(module==='amd') {
      assert.equal(call.arguments.length,2);assert.deepEqual(dependenciesOf(call.arguments[0]),['require','exports',...dependencies]);
      factory=call.arguments[1];
      const parameters=['require','exports'];
      if(!blue)for(const [index,dependency]of dependencies.entries())parameters.push(fresh(factory.parameters[index+2]?.name,'AMD dependency '+dependency));
      plainFunction(factory,parameters);
      parameterBindings=dependencies.map((dependency,index)=>({dependency,binding:blue?null:parameters[index+2]}));
      body=[...factory.body.statements];holes.push([factory.body.getStart(view.source)+1,factory.body.end-1]);format='wrapped';
    } else if(module==='umd') {
      assert.equal(call.arguments.length,1);const outer=plainFunction(unwrap(call.expression),['factory']);
      assert.equal(outer.body.statements.length,1);const branch=outer.body.statements[0];assert(ts.isIfStatement(branch)&&ts.isBlock(branch.thenStatement));
      const declaration=branch.thenStatement.statements[0];assert(ts.isVariableStatement(declaration)&&declaration.declarationList.declarations.length===1);
      const name=declaration.declarationList.declarations[0].name;assert(ts.isIdentifier(name));
      exact(outer,`function(factory){if(typeof module === 'object' && typeof module.exports === 'object'){var $return=factory(require,exports);if($return!==undefined)module.exports=$return;}else if(typeof define === 'function' && define.amd){define(${JSON.stringify(['require','exports',...dependencies])},factory);}}`,new Map([[name.text,'$return']]));
      factory=plainFunction(call.arguments[0],['require','exports']);body=[...factory.body.statements];holes.push([factory.body.getStart(view.source)+1,factory.body.end-1]);format='wrapped';
    } else {
      assert.equal(call.arguments.length,2);assert.deepEqual(dependenciesOf(call.arguments[0]),dependencies);
      factory=call.arguments[1];assert(ts.isFunctionExpression(factory)&&factory.parameters.length===2);
      const publisher=fresh(factory.parameters[0].name,'System publisher'),context=fresh(factory.parameters[1].name,'System context');assert.notEqual(publisher,context);
      plainFunction(factory,[publisher,context]);
      const statements=[...factory.body.statements],returned=statements.pop();assert(ts.isReturnStatement(returned)&&ts.isObjectLiteralExpression(returned.expression));
      const properties=returned.expression.properties;assert.equal(properties.length,2);assert(properties.every(ts.isPropertyAssignment));assert.deepEqual(properties.map(p=>p.name.getText(view.source)),['setters','execute']);
      const setters=properties[0].initializer;assert(ts.isArrayLiteralExpression(setters));setterNodes=[...setters.elements];assert.equal(setterNodes.length,dependencies.length);
      setterNodes.forEach((setter,index)=>{assert(ts.isFunctionExpression(setter)&&setter.parameters.length===1);const parameter=setter.parameters[0].name;assert(ts.isIdentifier(parameter)&&![publisher,context].includes(parameter.text),'System setter retains publisher/context bindings');plainFunction(setter,[parameter.text]);holes.push([setter.getStart(view.source),setter.end]);});
      execute=plainFunction(properties[1].initializer,[]);body=[...execute.body.statements];prelude.push(...statements);holes.push([execute.body.getStart(view.source)+1,execute.body.end-1]);
      for(const statement of statements)holes.push([statement.getStart(view.source),statement.end]);
      format='system';system={publisher,context,stem:null,returned};
      if(blue) {
        assert(publisher.endsWith('_export'));const stem=publisher.slice(0,-7);assert.equal(context,stem+'_context');assert(![...identifiers].some(name=>name===stem||name.startsWith(stem+'_')));system.stem=stem;
        const common=prelude.splice(0,3);assert.equal(common.length,3);
        exact(common[0],`'use strict';`);exact(common[1],`var exports={},${stem}_imports=Object.create(null);`);exact(common[2],`function require(name){return ${stem}_imports[name];}`);generated.push(...common);
        const asset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/system_helpers.v1.js'),'utf8').replaceAll('__SYS',stem);
        const source=ts.createSourceFile('system-transport.js',asset,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
        for(const wanted of source.statements){const node=prelude.shift();assert(node,'complete System helper inventory');exact(node,wanted.getText(source));generated.push(node);}
      }else assert(/^exports_\d+$/.test(publisher)&&/^context_\d+$/.test(context),'Native System factory ABI');
    }
    const outside=node=>!holes.some(([start,end])=>start<=node.start&&node.end<=end);
    wrapperTokens.push(...view.tokens(registration).filter(outside));
  }
  if(format==='wrapped'&&blue) {
    const strict=body.shift(),moduleBinding=body.shift(),returned=body.pop();assert(strict&&moduleBinding&&returned);
    exact(strict,`'use strict';`);exact(moduleBinding,'var module={exports:exports};');exact(returned,'return module.exports;');generated.push(strict,moduleBinding,returned);
  }
  function classifyTransport() {
    for(const node of generated)view.noSource(node.getStart(view.source),node.end,'generated module envelope helper');
    for(const token of wrapperTokens)view.noSource(token.start,token.end,'generated module envelope syntax');
    // Gaps bordering original bodies are left to the caller's ordered role
    // accounting. Pure envelope interiors retain no user coordinate.
    for(let i=1;i<wrapperTokens.length;i++) {
      const before=wrapperTokens[i-1],after=wrapperTokens[i];
      if(holes.some(([start,end])=>before.end<=start&&end<=after.start))continue;
      view.noSource(before.end,after.start,'generated module envelope gap');
    }
  }
  return {view,format,module:options.module,body,prelude,sourceNodes:[...prelude,...body],factory,execute,system,setterNodes,parameterBindings,dependencies:[...dependencies],generated,wrapperTokens,holes,exact,tokens,plainFunction,classifyTransport};
}
module.exports={read};
