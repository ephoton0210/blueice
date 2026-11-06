// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Parse the complete module transport before matching any original statement.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
function read(ts, view, options, blue, originals, profile = 'optional') {
  const original = ts.createSourceFile('original.ts', originals[0], ts.ScriptTarget.Latest, true);
  const names = new Set(), functions = new Set(), exported = new Set(), identifiers = new Set();
  function visit(node) {
    if(ts.isIdentifier(node)) identifiers.add(node.text);
    ts.forEachChild(node,visit);
  }
  visit(original);
  for(const statement of original.statements) {
    if(ts.isFunctionDeclaration(statement)&&['spread','tagged'].includes(profile)){assert(statement.name);names.add(statement.name.text);functions.add(statement.name.text);continue;}
    assert(ts.isVariableStatement(statement),'reviewed variable module source');
    for(const declaration of statement.declarationList.declarations) {
      assert(ts.isIdentifier(declaration.name)); names.add(declaration.name.text);
      if(statement.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)) exported.add(declaration.name.text);
    }
  }
  function unwrap(node) { while(ts.isParenthesizedExpression(node)) node=node.expression; return node; }
  function text(node) { return node.getText(view.source); }
  function words(node, aliases=new Map()) {
    return view.tokens(node).map(token => {
      const ast=ts.createScanner(ts.ScriptTarget.Latest,true,ts.LanguageVariant.Standard,token.text), kind=ast.scan();
      return kind===ts.SyntaxKind.StringLiteral ? JSON.stringify(ast.getTokenValue()) : aliases.get(token.text)||token.text;
    });
  }
  function expected(code) {
    const anonymous=code.startsWith('function(');
    const source=ts.createSourceFile('abi.js',anonymous?'('+code+')':code,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS), out=[];
    assert.deepEqual(source.parseDiagnostics.map(item=>item.code),[],'valid transport specification');
    function tokens(node) {
      if(ts.isToken(node)){if(node.kind!==ts.SyntaxKind.EndOfFileToken)out.push(ts.isStringLiteral(node)?JSON.stringify(node.text):node.getText(source));return;}
      for(const child of node.getChildren(source))tokens(child);
    }
    tokens(anonymous?source.statements[0].expression.expression:source);return out;
  }
  function exact(node, code, aliases) { assert.deepEqual(words(node,aliases),expected(code),'complete module transport ABI'); }
  function plainFunction(node, parameters) {
    assert(ts.isFunctionExpression(node)&&!node.name&&!node.asteriskToken&&!node.modifiers?.length);
    assert.deepEqual(node.parameters.map(p=>p.name.getText()),parameters);
    assert(node.parameters.every(p=>ts.isIdentifier(p.name)&&!p.initializer&&!p.dotDotDotToken&&!p.questionToken&&!p.type&&!p.modifiers?.length),'plain generated parameters');return node;
  }
  function dependencies(node,wanted) {
    assert(ts.isArrayLiteralExpression(node));assert.deepEqual(node.elements.map(item=>{assert(ts.isStringLiteral(item));return item.text;}),wanted);
  }
  const generated=[], helpers=new Map(), slots=new Set(), scope=[];
  const extension=profile==='spread'?'ge5_spread_helpers.cjs':profile==='tagged'?'ge5_tagged_helpers.cjs':null;
  const spread=extension?require('./'+extension).create(ts,view,options,blue,identifiers,exact,generated,helpers):null;
  const dependency=spread?.external?['tslib']:[];
  let body=[...view.source.statements], format='plain', system=null, setterNodes=[];
  if(!blue&&spread&&['AMD','UMD'].includes(options.module))body=body.filter(node=>!spread.statement(node));
  if(['AMD','UMD','System'].includes(options.module)) {
    assert.equal(body.length,1);const statement=body[0];assert(ts.isExpressionStatement(statement)&&ts.isCallExpression(statement.expression));
    const call=statement.expression;assert.equal(call.arguments.length,options.module==='UMD'?1:2);
    if(options.module==='AMD') {
      assert.equal(call.expression.getText(),'define');dependencies(call.arguments[0],['require','exports',...dependency]);
      const callback=call.arguments[1],parameters=['require','exports'];
      if(!blue&&dependency.length){assert(callback.parameters.length===3);spread.namespace(callback.parameters[2].name.getText());parameters.push(spread.state.namespace);}
      body=[...plainFunction(callback,parameters).body.statements];format='wrapped';
    } else if(options.module==='UMD') {
      const outer=unwrap(call.expression);plainFunction(outer,['factory']);
      assert.equal(outer.body.statements.length,1);const branch=outer.body.statements[0];assert(ts.isIfStatement(branch)&&ts.isBlock(branch.thenStatement));
      const binding=branch.thenStatement.statements[0];assert(ts.isVariableStatement(binding)&&binding.declarationList.declarations.length===1);
      const name=binding.declarationList.declarations[0].name;assert(ts.isIdentifier(name));
      exact(outer,`function(factory){if(typeof module === "object" && typeof module.exports === "object") {var $return = factory(require,exports);if($return !== undefined) module.exports=$return;} else if(typeof define === "function" && define.amd){define(${JSON.stringify(["require","exports",...dependency])},factory);}}`,new Map([[name.text,'$return']]));
      assert.equal(call.arguments.length,1,'UMD callback inventory');
      body=[...plainFunction(call.arguments[0],['require','exports']).body.statements];format='wrapped';
    } else {
      assert.equal(call.expression.getText(),'System.register');dependencies(call.arguments[0],dependency);
      const factory=call.arguments[1];assert(ts.isFunctionExpression(factory)&&factory.parameters.length===2);plainFunction(factory,factory.parameters.map(p=>p.name.getText()));
      const [publisher,context]=factory.parameters.map(p=>p.name.getText());
      assert(!identifiers.has(publisher)&&!identifiers.has(context),'fresh System transport bindings');
      const statements=[...factory.body.statements], returned=statements.pop();assert(ts.isReturnStatement(returned)&&ts.isObjectLiteralExpression(returned.expression));
      const properties=returned.expression.properties;assert.deepEqual(properties.map(p=>p.name.getText()),['setters','execute']);
      assert(ts.isArrayLiteralExpression(properties[0].initializer));setterNodes=[...properties[0].initializer.elements];
      assert.equal(setterNodes.length,dependency.length);const execute=plainFunction(properties[1].initializer,[]);
      body=[...execute.body.statements];scope.push(...statements);format='system';system={publisher,context};
      if(blue) {
        assert(publisher.endsWith('_export'));const stem=publisher.slice(0,-7);assert.equal(context,stem+'_context');
        assert(![...identifiers].some(name=>name===stem||name.startsWith(stem+'_')),'fresh System helper namespace');
        system.stem=stem;
        const asset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/system_helpers.v1.js'),'utf8').replaceAll('__SYS',stem);
        const source=ts.createSourceFile('system-abi.js',asset,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
        const assetStatements=[...source.statements];
        const first=scope.shift();exact(first,`'use strict';`);generated.push(first);
        const setup=scope.shift();exact(setup,`var exports = {}, ${stem}_imports=Object.create(null);`);generated.push(setup);
        const require=scope.shift();exact(require,`function require(name){return ${stem}_imports[name];}`);generated.push(require);
        for(const wanted of assetStatements) {const node=scope.shift();assert(node);exact(node,wanted.getText(source));generated.push(node);}
      } else {
        assert(/^exports_\d+$/.test(publisher)&&/^context_\d+$/.test(context));
      }
    }
  }
  if(format==='wrapped'&&blue) {
    const strict=body.shift();exact(strict,`'use strict';`);generated.push(strict);
    const module=body.shift();exact(module,'var module={exports:exports};');generated.push(module);
    const returned=body.pop();exact(returned,'return module.exports;');generated.push(returned);
  }
  const inputStatements=format==='system'?scope:body, user=[];
  if(dependency.length&&format==='system') {
    const setter=setterNodes[0];assert(ts.isFunctionExpression(setter)&&setter.parameters.length===1);const parameter=setter.parameters[0].name.getText();
    const assignment=setter.body.statements[blue?1:0];assert(ts.isExpressionStatement(assignment)&&ts.isBinaryExpression(assignment.expression));
    assert(ts.isIdentifier(assignment.expression.left));spread.namespace(assignment.expression.left.text);
    assert(![spread.state.namespace,system.stem+'_imports',system.stem+'_refresh'].includes(parameter),'setter parameter cannot capture a module binding');
    exact(setter,blue?`function(${parameter}){${system.stem}_imports['tslib']=${parameter};${spread.state.namespace}=${parameter};${system.stem}_refresh();}`:`function(${parameter}){${spread.state.namespace}=${parameter};}`);
  }
  function generatedDeclaration(statement, hoisted) {
    assert(ts.isVariableStatement(statement));
    for(const declaration of statement.declarationList.declarations) {
      assert(ts.isIdentifier(declaration.name));const name=declaration.name.text;
      if(name==='__moduleName'&&!blue&&format==='system') {
        exact(statement,`var __moduleName=${system.context}&&${system.context}.id;`);continue;
      }
      assert(!declaration.initializer,'generated/hoisted slot has no initializer');
      assert(hoisted&&names.has(name)||!identifiers.has(name),'fresh generated slot or original hoisted binding');
      assert(!slots.has(name));slots.add(name);
    }
    generated.push(statement);
  }
  function helper(statement) {
    if(!ts.isFunctionDeclaration(statement))return false;
    const name=statement.name?.text;
    assert(blue&&/^__blueice_target_(?:optional_property|coalesce_nullish)_\d+$/.test(name),'reviewed optional helper');
    assert(!identifiers.has(name));assert(!helpers.has(name));
    const optional=name.includes('optional_property'), code=optional
      ? `function ${name}(value,key){return value===null||value===void 0?void 0:value[key];}`
      : `function ${name}(value){return value===null||value===void 0;}`;
    exact(statement,code);helpers.set(name,optional?'optional':'nullish');generated.push(statement);return true;
  }
  if(format==='system') {
    for(const statement of inputStatements) {
      if(spread?.statement(statement))continue;
      if(ts.isFunctionDeclaration(statement)&&functions.has(statement.name?.text)){user.push(statement);continue;}
      if(ts.isExpressionStatement(statement)&&ts.isStringLiteral(statement.expression)) {exact(statement,`'use strict';`);generated.push(statement);}
      else if(ts.isVariableStatement(statement))generatedDeclaration(statement,true);
      else if(helper(statement))continue;
      else {assert(blue);exact(statement,`${system.stem}_slot("result");`);generated.push(statement);}
    }
  }
  for(const statement of body) {
    if(spread?.statement(statement))continue;
    if(ts.isFunctionDeclaration(statement)&&functions.has(statement.name?.text)){user.push(statement);continue;}
    if(ts.isExpressionStatement(statement)&&ts.isStringLiteral(statement.expression)){exact(statement,`'use strict';`);generated.push(statement);continue;}
    if(ts.isVariableStatement(statement)) {
      const declared=statement.declarationList.declarations;
      if(format!=='system'&&declared.every(d=>ts.isIdentifier(d.name)&&names.has(d.name.text))) {user.push(statement);continue;}
      generatedDeclaration(statement,false);continue;
    }
    if(helper(statement))continue;
    if(ts.isExpressionStatement(statement)) {
      const expression=statement.expression;
      if(ts.isCallExpression(expression)&&expression.expression.getText()==='Object.defineProperty') {
        exact(statement,'Object.defineProperty(exports,"__esModule",{value:true});');generated.push(statement);continue;
      }
      if(ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken) {
        if(ts.isPropertyAccessExpression(expression.left)&&expression.left.expression.getText()==='exports'&&ts.isVoidExpression(expression.right)) {
          assert(exported.has(expression.left.name.text));assert.equal(expression.right.expression.getText(),'0');generated.push(statement);continue;
        }
        user.push(statement);continue;
      }
      if(format==='system'&&!blue&&ts.isCallExpression(expression)&&expression.expression.getText()===system.publisher){user.push(statement);continue;}
      if(format==='system'&&blue&&ts.isCallExpression(expression)&&expression.expression.getText()===system.stem+'_refresh') {exact(statement,system.stem+'_refresh();');generated.push(statement);continue;}
      if(format==='system'&&blue&&ts.isIdentifier(expression)&&slots.has(expression.text)&&!names.has(expression.text)){generated.push(statement);continue;}
    }
    assert.fail('unreviewed module statement: '+text(statement));
  }
  spread?.finish();
  assert.equal(user.length,names.size,'complete original statement inventory');
  return {view,format,system,user,generated,helpers,slots,names,exported,dependency,spread:spread?.state,tagged:profile==='tagged'?spread.state:null};
}
module.exports={read};
