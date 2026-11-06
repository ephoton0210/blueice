// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Private helper capabilities come from exact complete algorithms/import bindings.
const assert=require('node:assert/strict');
const names={Get:'__classPrivateFieldGet',Set:'__classPrivateFieldSet',In:'__classPrivateFieldIn'};
const privateNames={Get:'__bluetsClassPrivateGet',Set:'__bluetsClassPrivateSet',In:'__bluetsClassPrivateIn'};
const proofs=new WeakMap(),profiles=new WeakMap();
function prepare(ts,native,actual,n,a,original,options) {
  const identifiers=new Set();function originalNames(node){if(ts.isIdentifier(node))identifiers.add(node.text);ts.forEachChild(node,originalNames);}originalNames(original);
  const roots=new Set(original.statements.flatMap(node=>ts.isClassDeclaration(node)?[node.name.text]:ts.isVariableStatement(node)?node.declarationList.declarations.map(d=>d.name.getText(original)):[]));
  function read(item,blue) {
    const view=item.view,helpers=new Map(),bindings=new Map(),generated=[],namespaces=new Set(),namedImports=new Set();let storage;
    const classes=[];function visit(node){if((ts.isClassDeclaration(node)||ts.isClassExpression(node))&&roots.has(node.name?.text))classes.push(node);ts.forEachChild(node,visit);}visit(view.source);assert.equal(classes.length,1);
    const ctor=classes[0].members.find(ts.isConstructorDeclaration);assert(ctor&&!ctor.parameters.length&&ctor.body.statements.length===1,'one complete private-field constructor operation');
    const init=ctor.body.statements[0];assert(ts.isExpressionStatement(init)&&ts.isCallExpression(init.expression));
    const call=init.expression;assert(ts.isPropertyAccessExpression(call.expression)&&call.expression.name.text==='set'&&ts.isIdentifier(call.expression.expression));storage=call.expression.expression.text;
    assert(!identifiers.has(storage),'private field state is fresh in the original program');assert.equal(call.arguments.length,2);assert.equal(call.arguments[0].kind,ts.SyntaxKind.ThisKeyword);
    const cover=(node,label)=>{view.noSource(node.getStart(view.source),node.end,label);generated.push(node);};
    if(options.importHelpers&&item.format==='system')for(const setter of item.setterNodes){
      assert.equal(item.dependencies.length,1);assert.equal(item.dependencies[0],'tslib');
      const parameter=setter.parameters[0].name.text,assignment=setter.body.statements[blue?1:0];
      assert(ts.isExpressionStatement(assignment)&&ts.isBinaryExpression(assignment.expression)&&ts.isIdentifier(assignment.expression.left));
      const namespace=assignment.expression.left.text;assert(!identifiers.has(namespace));namespaces.add(namespace);
      item.exact(setter,blue?`function(${parameter}){${item.system.stem}_imports["tslib"]=${parameter};${namespace}=${parameter};${item.system.stem}_refresh();}`:`function(${parameter}){${namespace}=${parameter};}`);
      cover(setter,'complete private-helper System dependency binding');
    }
    for(const p of item.parameterBindings)if(p.dependency==='tslib'&&p.binding)namespaces.add(p.binding);
    for(const node of item.sourceNodes){
      if(blue&&item.format==='system'&&ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isIdentifier(node.expression.left)){
        const expression=node.expression,role=Object.keys(privateNames).find(role=>privateNames[role]===expression.left.text);
        if(role){assert(!options.importHelpers&&!options.noEmitHelpers&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&!helpers.has(role));const fn=expression.right;assert(ts.isFunctionExpression(fn)&&!fn.name&&!fn.asteriskToken&&!fn.modifiers?.length);helpers.set(role,{node,fn});bindings.set(expression.left.text,role);cover(node,'complete relocated private-helper algorithm');continue;}
      }
      if(ts.isImportDeclaration(node)&&ts.isStringLiteral(node.moduleSpecifier)&&node.moduleSpecifier.text==='tslib'){
        assert(options.importHelpers,'only the imported-helper profile imports tslib');
        if(blue){assert(ts.isNamespaceImport(node.importClause.namedBindings));const name=node.importClause.namedBindings.name.text;assert(!identifiers.has(name));namespaces.add(name);item.exact(node,`import * as ${name} from 'tslib';`);}
        else{assert(ts.isNamedImports(node.importClause.namedBindings));const imported=node.importClause.namedBindings.elements;assert.deepEqual(imported.map(i=>i.propertyName?.text??i.name.text).sort(),Object.values(names).sort());for(const i of imported){assert(!i.propertyName);namedImports.add(i.name.text);}}
        cover(node,'complete private-helper library import');continue;
      }
      if(ts.isVariableStatement(node)){
        const declarations=node.declarationList.declarations;
        if(declarations.length===1&&ts.isIdentifier(declarations[0].name)){
          const d=declarations[0],name=d.name.text,initializer=d.initializer;
          if(initializer&&ts.isCallExpression(initializer)&&initializer.expression.getText(view.source)==='require'&&initializer.arguments.length===1&&ts.isStringLiteral(initializer.arguments[0])&&initializer.arguments[0].text==='tslib'){
            assert(options.importHelpers&&!identifiers.has(name));namespaces.add(name);item.exact(node,`${blue?'var':'const'} ${name}=require('tslib');`);cover(node,'complete private-helper require binding');continue;
          }
          const role=Object.keys(names).find(role=>name===(blue?privateNames[role]:names[role]));
          if(role){assert(!options.importHelpers&&!options.noEmitHelpers&&!identifiers.has(name)&&!helpers.has(role));let fn=initializer;
            if(!blue){assert(ts.isBinaryExpression(initializer)&&initializer.operatorToken.kind===ts.SyntaxKind.BarBarToken);item.exact(initializer.left,`(this&&this.${name})`);fn=initializer.right;}
            assert(ts.isFunctionExpression(fn)&&!fn.name&&!fn.asteriskToken&&!fn.modifiers?.length);helpers.set(role,{node,fn});bindings.set(name,role);cover(node,'complete generated private-helper algorithm');continue;
          }
        }
        if(declarations.some(d=>ts.isIdentifier(d.name)&&d.name.text===storage)){
          const expected=new Set([storage]);
          if(item.format==='system'){
            for(const name of roots)expected.add(name);
            if(blue)for(const source of original.statements.filter(ts.isVariableStatement))if(source.modifiers?.some(m=>m.kind===ts.SyntaxKind.ExportKeyword))for(const d of source.declarationList.declarations)expected.delete(d.name.getText(original));
            for(const name of namespaces)expected.add(name);
            if(blue&&!options.importHelpers&&!options.noEmitHelpers)for(const name of Object.values(privateNames))expected.add(name);
          }
          assert(declarations.every(d=>ts.isIdentifier(d.name)&&!d.initializer),'plain complete hoisted binding declarations');
          assert.deepEqual(declarations.map(d=>d.name.text).sort(),[...expected].sort(),'complete private/transport hoisted binding inventory');cover(node,'complete private state declaration');continue;
        }
      }
      if(ts.isExpressionStatement(node)&&ts.isIdentifier(node.expression)&&node.expression.text===storage){assert(blue&&item.format==='system');cover(node,'complete relocated private-state declaration');continue;}
      if(ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isIdentifier(node.expression.left)&&node.expression.left.text===storage){item.exact(node,`${storage}=new WeakMap();`);cover(node,'complete private-state allocation');}
    }
    if(options.importHelpers){assert.equal(helpers.size,0);assert.equal(namespaces.size+Number(namedImports.size>0),1,'one exact helper-library identity');}
    else if(options.noEmitHelpers){assert.equal(helpers.size,0);for(const[role,name]of Object.entries(names)){assert(!identifiers.has(name));bindings.set(name,role);}}
    else assert.deepEqual([...helpers.keys()].sort(),Object.keys(names).sort(),'complete private helper inventory');
    let references=0;function uses(node){if(ts.isIdentifier(node)&&node.text===storage)references++;ts.forEachChild(node,uses);}uses(view.source);assert.equal(references,6+Number(blue&&item.format==='system'),'complete private-state binding reference inventory');
    return {item,view,blue,helpers,bindings,namespaces,namedImports,storage,generated,classNode:classes[0]};
  }
  const np=read(n,false),ap=read(a,true);
  function functionTokens(view,fn){const parameters=fn.parameters.map(p=>{assert(ts.isIdentifier(p.name)&&!p.initializer&&!p.dotDotDotToken&&!p.type);return p.name.text;});assert.equal(new Set(parameters).size,parameters.length);const aliases=new Map(parameters.map((name,i)=>[name,'$p'+i]));const replacements=new Map();function walk(node){if(ts.isIdentifier(node)&&aliases.has(node.text)&&!(ts.isPropertyAccessExpression(node.parent)&&node.parent.name===node)&&!(ts.isPropertyAssignment(node.parent)&&node.parent.name===node))replacements.set(node.getStart(view.source),aliases.get(node.text));ts.forEachChild(node,walk);}walk(fn);return view.tokens(fn).map(token=>replacements.get(token.start)||token.text);}
  for(const[role,helper]of np.helpers){const actualHelper=ap.helpers.get(role);assert(actualHelper);assert.deepEqual(functionTokens(actual,actualHelper.fn),functionTokens(native,helper.fn),'complete private '+role+' algorithm with exact parameter binding identities');}
  const result={native:np,actual:ap};
  for(const profile of[np,ap])profiles.set(profile,{view:profile.view,bindings:new Map(profile.bindings),namedImports:new Set(profile.namedImports),namespaces:new Set(profile.namespaces)});
  proofs.set(result,{ts,native:np,actual:ap,original,options});return result;
}
function reference(ts,proof,profile,expression) {
  const capability=proofs.get(proof);assert(capability&&capability.ts===ts&&(profile===capability.native||profile===capability.actual),'owned exact private-helper provider proof');
  const binding=profiles.get(profile);assert(binding&&expression.getSourceFile()===binding.view.source,'exact emitted private helper reference node');
  if(ts.isIdentifier(expression)){
    const role=binding.bindings.get(expression.text)||[...binding.namedImports].map(name=>[name,Object.keys(names).find(role=>names[role]===name)]).find(([name])=>name===expression.text)?.[1];
    assert(role,'private operation uses its exact helper binding');return role;
  }
  assert(ts.isPropertyAccessExpression(expression)&&ts.isIdentifier(expression.expression)&&binding.namespaces.has(expression.expression.text),'private helper namespace comes from the exact module dependency');
  const role=Object.keys(names).find(role=>names[role]===expression.name.text);assert(role);return role;
}
module.exports={prepare,reference};
