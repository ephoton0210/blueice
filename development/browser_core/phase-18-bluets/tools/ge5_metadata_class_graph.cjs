// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Constructor metadata identities follow the original import and provider export ASTs.
const assert=require('node:assert/strict'),proofs=new WeakSet();
function read(ts,files,entry,options) {
  const sources=new Map(Object.entries(files).map(([name,text])=>[name,ts.createSourceFile(name,text,ts.ScriptTarget.Latest,true)])),original=sources.get(entry);
  assert(original,'complete original graph entry');
  const imports=original.statements.filter(ts.isImportDeclaration);assert.equal(imports.length,1,'one original constructor dependency');
  const imported=imports[0];assert(ts.isStringLiteral(imported.moduleSpecifier)&&imported.importClause);
  const dependency=imported.moduleSpecifier.text,filename=require('./ge5_metadata_export_graph.cjs').filename(entry,dependency),resolver=require('./ge5_metadata_export_graph.cjs').create(ts,sources);assert(sources.has(filename),'original relative dependency resolves to a preserved input module');
  const clause=imported.importClause,named=clause.namedBindings,clauseTypeOnly=clause.isTypeOnly;let local,exportName,namespace=false,typeOnlySpecifier;
  if(clause.name){assert(!named);local=clause.name.text;exportName='default';}
  else if(ts.isNamespaceImport(named)){local=named.name.text;namespace=true;}
  else {assert(ts.isNamedImports(named)&&named.elements.length===1);const specifier=named.elements[0];assert(!(clauseTypeOnly&&specifier.isTypeOnly),'one original type-only import marker');if(specifier.isTypeOnly)typeOnlySpecifier=specifier;local=specifier.name.text;exportName=(specifier.propertyName??specifier.name).text;}
  const classes=original.statements.filter(ts.isClassDeclaration);assert.equal(classes.length,1);const sourceClass=classes[0];assert(ts.getDecorators(sourceClass)?.length);
  const localAliases=new Set();
  function type(type) {
    assert(ts.isTypeReferenceNode(type)&&!type.typeArguments,'constructor metadata uses an original non-generic imported type');
    let localAlias;
    if(ts.isIdentifier(type.typeName)&&type.typeName.text!==local) {
      const aliases=original.statements.filter(node=>ts.isTypeAliasDeclaration(node)&&node.name.text===type.typeName.text);assert.equal(aliases.length,1,'original metadata type resolves to one local class type-alias');
      localAlias=aliases[0];assert(!localAlias.typeParameters?.length&&ts.isTypeReferenceNode(localAlias.type)&&!localAlias.type.typeArguments,'local class type-alias directly targets an original import reference');
      localAliases.add(localAlias);type=localAlias.type;
    }
    let exported=exportName;
    if(namespace){assert(ts.isQualifiedName(type.typeName)&&ts.isIdentifier(type.typeName.left)&&type.typeName.left.text===local);exported=type.typeName.right.text;}
    else assert(ts.isIdentifier(type.typeName)&&type.typeName.text===local,'original type reference resolves to its import binding');
    const resolved=resolver.resolve(filename,exported),provider=resolved.source,providerName=resolved.name;
    const declarations=provider.statements.filter(node=>ts.isClassDeclaration(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)&&(node.modifiers.some(modifier=>modifier.kind===ts.SyntaxKind.DefaultKeyword)?'default':node.name.text)===providerName);
    const aliases=provider.statements.filter(node=>ts.isTypeAliasDeclaration(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)&&node.name.text===providerName);
    const interfaces=provider.statements.filter(node=>ts.isInterfaceDeclaration(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)&&(node.modifiers.some(modifier=>modifier.kind===ts.SyntaxKind.DefaultKeyword)?'default':node.name.text)===providerName);
    const enums=provider.statements.filter(node=>ts.isEnumDeclaration(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)&&node.name.text===providerName);
    assert.equal(declarations.length+aliases.length+interfaces.length+enums.length,1,'metadata type resolves to one original exported provider symbol');
    if(clauseTypeOnly||typeOnlySpecifier){assert(!localAlias&&declarations.length===1,'original type-only import resolves the exact provider class AST');return {dependency,exportName:exported,sourceClass:declarations[0],typeOnlyImport:imported,typeOnlySpecifier,builtin:'Function',runtime:false};}
    if(enums.length){assert(!localAlias,'this capability requires a direct imported enum reference');const enumeration=require('./ge5_metadata_enums.cjs').source(ts,enums[0]);return {dependency,exportName:exported,enumeration,builtin:enumeration.builtin,acquire:true,runtime:false};}
    if(interfaces.length){assert(!localAlias,'this capability requires a direct imported interface reference');return {dependency,exportName:exported,interface:interfaces[0],builtin:'Object',runtime:false};}
    if(localAlias){assert.equal(declarations.length,1,'local class type-alias target is one original imported provider class');return {dependency,exportName:exported,alias:localAlias,sourceClass:declarations[0],builtin:'Object',runtime:false};}
    if(aliases.length) {
      const alias=aliases[0];assert(!alias.typeParameters?.length,'exact original nongeneric type-alias declaration');
      if(alias.type.kind===ts.SyntaxKind.StringKeyword)return {dependency,exportName:exported,alias,builtin:'String',runtime:false};
      if(ts.isFunctionTypeNode(alias.type))return {dependency,exportName:exported,alias,callable:alias.type,builtin:'Function',runtime:false};
      let reference=alias.type,nullable;
      if(ts.isUnionTypeNode(alias.type)){
        assert.equal(alias.type.types.length,2,'exact nullable class union has two original members');
        const classes=alias.type.types.filter(ts.isTypeReferenceNode),nulls=alias.type.types.filter(type=>ts.isLiteralTypeNode(type)&&type.literal.kind===ts.SyntaxKind.NullKeyword);
        assert.equal(classes.length,1,'nullable metadata alias has one original class reference');assert.equal(nulls.length,1,'nullable metadata alias has one exact null literal');reference=classes[0];nullable=alias.type;
      }
      assert(ts.isTypeReferenceNode(reference)&&ts.isIdentifier(reference.typeName)&&!reference.typeArguments,'exact original class type-alias declaration');
      const targets=provider.statements.filter(node=>ts.isClassDeclaration(node)&&node.name?.text===reference.typeName.text&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));assert.equal(targets.length,1,'original alias target resolves to one provider class AST');
      return {dependency,exportName:exported,alias,nullable,sourceClass:targets[0],builtin:'Object',runtime:false};
    }
    return {dependency,exportName:exported,sourceClass:declarations[0],runtime:true};
  }
  const expected=[];
  for(const member of sourceClass.members) {
    if(ts.isConstructorDeclaration(member))continue;
    assert(ts.getDecorators(member)?.length,'complete decorated original member inventory');
    if(ts.isPropertyDeclaration(member))expected.push({key:'design:type',type:type(member.type),source:member});
    else {assert(ts.isMethodDeclaration(member));expected.push({key:'design:type',builtin:'Function',source:member},{key:'design:paramtypes',types:member.parameters.map(parameter=>type(parameter.type)),source:member},{key:'design:returntype',type:type(member.type),source:member});}
  }
  const constructor=sourceClass.members.filter(ts.isConstructorDeclaration);assert.equal(constructor.length,1);assert(constructor[0].parameters.some(parameter=>ts.getDecorators(parameter)?.length));
  expected.push({key:'design:paramtypes',types:constructor[0].parameters.map(parameter=>type(parameter.type)),source:sourceClass});
  function sourceUse(node) {
    if(ts.isIdentifier(node)&&node.text===local) {
      const parent=node.parent;
      const binding=parent===clause||ts.isNamespaceImport(parent)||ts.isImportSpecifier(parent);
      const reference=namespace?ts.isQualifiedName(parent)&&parent.left===node&&ts.isTypeReferenceNode(parent.parent):ts.isTypeReferenceNode(parent)&&parent.typeName===node;
      assert(binding||reference,'runtime dependency is induced solely by original constructor metadata types');
    }
    if(ts.isIdentifier(node)&&[...localAliases].some(alias=>alias.name.text===node.text)) {
      const parent=node.parent,binding=ts.isTypeAliasDeclaration(parent)&&localAliases.has(parent)&&parent.name===node,reference=ts.isTypeReferenceNode(parent)&&parent.typeName===node;
      assert(binding||reference,'local class type-alias has no runtime source use');
    }
    ts.forEachChild(node,sourceUse);
  }
  sourceUse(original);
  const runtime=!!options.emitDecoratorMetadata&&expected.some(role=>role.type?.runtime||role.type?.acquire||role.types?.some(type=>type.runtime||type.acquire));
  const aliases=[...new Set(expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).flatMap(type=>type.alias?[type.alias]:[]))];
  const interfaces=[...new Set(expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).flatMap(type=>type.interface?[type.interface]:[]))];
  const enums=[...new Set(expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).flatMap(type=>type.enumeration?[type.enumeration.declaration]:[]))];
  const typeOnlyImports=[...new Set(expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).flatMap(type=>type.typeOnlyImport?[type.typeOnlyImport]:[]))];
  const typeOnlySpecifiers=[...new Set(expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).flatMap(type=>type.typeOnlySpecifier?[type.typeOnlySpecifier]:[]))];
  const resolved=resolver.complete(entry);
  const graph={original,provider:resolved.provider,reexports:resolved.reexports,modules:resolved.modules,sourceClass,constructor:constructor[0],imported,dependency,filename,local,namespace,exportName,expected:options.emitDecoratorMetadata?expected:[],aliases,interfaces,enums,typeOnlyImports,typeOnlySpecifiers,runtime};proofs.add(graph);return Object.freeze(graph);
}
function compare(ts,native,actual,n,a,contract,graph,options,phases) {
  const acquired=[];
  for(const [item,blue]of [[n,false],[a,true]]) {
    const requireCall=node=>ts.isCallExpression(node)&&ts.isIdentifier(node.expression)&&node.expression.text==='require';
    const candidates=item.sourceNodes.filter(node=>ts.isImportDeclaration(node)||ts.isExpressionStatement(node)&&requireCall(node.expression)||ts.isVariableStatement(node)&&node.declarationList.declarations.some(declaration=>declaration.initializer&&requireCall(declaration.initializer)));
    assert.equal(candidates.length,graph.runtime?1:0,'exact metadata-induced dependency acquisition/elision');
    if(!options.emitDecoratorMetadata)assert(![...(blue?phases.actual:phases.native).bindings.values()].some(binding=>binding.kind==='metadata'),'metadata-disabled module has no metadata helper');
    let node,binding;
    if(graph.runtime) {
    node=candidates[0];
    if(options.module==='CommonJS') {
      assert(ts.isVariableStatement(node)&&node.declarationList.declarations.length===1);const declaration=node.declarationList.declarations[0];assert(ts.isIdentifier(declaration.name));binding=declaration.name.text;
      assert.equal(declaration.initializer.arguments.length,1);assert(ts.isStringLiteral(declaration.initializer.arguments[0])&&declaration.initializer.arguments[0].text===graph.dependency);
      assert.equal(node.declarationList.flags&ts.NodeFlags.BlockScoped,options.target==='ES5'?ts.NodeFlags.None:ts.NodeFlags.Const);
      if(graph.namespace)assert.equal(binding,graph.local,'namespace import retains its original source binding');
      else {const sourceNames=new Set();function names(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,names);}names(graph.original);assert(!sourceNames.has(binding),'require namespace is fresh in the original module');}
    }else {
      assert(ts.isImportDeclaration(node)&&ts.isStringLiteral(node.moduleSpecifier)&&node.moduleSpecifier.text===graph.dependency);
      assert.equal(node.getText(item.view.source),graph.imported.getText(graph.original),'retained import preserves its complete original clause and local binding');binding=graph.local;
    }
    }
    const state=blue?phases.actual:phases.native,metadata=[];
    function visit(call) {
      if(state.definitions.includes(call))return;
      if(ts.isCallExpression(call)&&ts.isIdentifier(call.expression)&&state.bindings.get(call.expression.text)?.kind==='metadata')metadata.push(call);
      ts.forEachChild(call,visit);
    }
    visit(item.view.source);assert.equal(metadata.length,graph.expected.length,'complete original AST-derived metadata-call inventory');
    function reference(expression,type) {
      if(type.builtin){assert(ts.isIdentifier(expression)&&expression.text===type.builtin,'original erased type emits its exact builtin metadata without a constructor read');return;}
      assert(graph.runtime,'provider constructor references require a proved runtime dependency');
      if(options.module==='CommonJS'||graph.namespace)assert(ts.isPropertyAccessExpression(expression)&&ts.isIdentifier(expression.expression)&&expression.expression.text===binding&&expression.name.text===type.exportName,'metadata references the exact acquired provider constructor export');
      else assert(ts.isIdentifier(expression)&&expression.text===graph.local,'metadata references the exact retained local import binding');
    }
    for(let index=0;index<metadata.length;index++) {
      const call=metadata[index],expected=graph.expected[index];assert.equal(call.arguments.length,2);assert(ts.isStringLiteral(call.arguments[0])&&call.arguments[0].text===expected.key);
      if(expected.type)reference(call.arguments[1],expected.type);
      else if(expected.builtin)assert(ts.isIdentifier(call.arguments[1])&&call.arguments[1].text===expected.builtin,'method metadata constructor kind');
      else {assert(ts.isArrayLiteralExpression(call.arguments[1]));assert.equal(call.arguments[1].elements.length,expected.types.length);call.arguments[1].elements.forEach((expression,index)=>reference(expression,expected.types[index]));}
    }
    acquired.push(graph.runtime?{node,binding,metadata}:null);
  }
  if(graph.runtime){const aliases=new Map(options.module==='CommonJS'&&!graph.namespace?[[acquired[1].binding,acquired[0].binding]]:[]);contract.pair(native.tokens(acquired[0].node),actual.tokens(acquired[1].node),'complete original constructor dependency import',aliases);assert.deepEqual(actual.owner(acquired[1].node.end),native.owner(acquired[0].node.end),'complete constructor import logical endpoint');}
  return {graph,native:acquired[0],actual:acquired[1]};
}
module.exports={read,compare,valid:graph=>proofs.has(graph)};
