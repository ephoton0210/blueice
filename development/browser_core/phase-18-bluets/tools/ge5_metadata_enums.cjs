// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original enum declarations own every member, reverse map, factory and publication phase.
const assert=require('node:assert/strict'),vm=require('node:vm');
function source(ts,declaration) {
  assert(ts.isEnumDeclaration(declaration)&&declaration.members.length,'original nonempty enum declaration');
  assert(!declaration.modifiers?.some(modifier=>[ts.SyntaxKind.ConstKeyword,ts.SyntaxKind.DeclareKeyword].includes(modifier.kind)),'runtime enum source phase');
  const names=new Set();for(const member of declaration.members){assert(ts.isIdentifier(member.name));assert(!names.has(member.name.text));names.add(member.name.text);}
  const numeric=declaration.members.every(member=>!member.initializer),string=declaration.members.every(member=>member.initializer&&ts.isStringLiteral(member.initializer));
  assert(numeric||string,'closed automatic numeric or literal string enum member AST');
  return {declaration,name:declaration.name.text,builtin:numeric?'Number':'String',members:declaration.members.map((member,index)=>({source:member,name:member.name.text,value:numeric?index:member.initializer.text,reverse:numeric}))};
}
function unwrap(ts,node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function read(ts,item,cap,options) {
  const view=item.view,name=cap.name;
  const variables=item.sourceNodes.filter(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&ts.isIdentifier(node.declarationList.declarations[0].name)&&node.declarationList.declarations[0].name.text===name);
  assert.equal(variables.length,1,'one exact enum variable declaration');const variable=variables[0],binding=variable.declarationList.declarations[0];assert(!binding.initializer);assert.equal(variable.declarationList.flags&ts.NodeFlags.BlockScoped,ts.NodeFlags.None);
  const exported=!!cap.declaration.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword);assert.equal(!!variable.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword),exported&&options.module!=='CommonJS','exact enum export declaration phase');
  const factories=item.sourceNodes.filter(node=>{if(!ts.isExpressionStatement(node)||!ts.isCallExpression(node.expression))return false;const fn=unwrap(ts,node.expression.expression);return ts.isFunctionExpression(fn)&&fn.parameters.length===1&&ts.isIdentifier(fn.parameters[0].name)&&fn.parameters[0].name.text===name;});
  assert.equal(factories.length,1,'one exact enum factory invocation');const statement=factories[0],call=statement.expression,factory=unwrap(ts,call.expression);assert(!factory.name&&!factory.asteriskToken&&!factory.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.AsyncKeyword));assert(!factory.parameters[0].initializer&&!factory.parameters[0].dotDotDotToken);assert.equal(factory.body.statements.length,cap.members.length,'complete enum member statement inventory');
  function identifier(node){assert(ts.isIdentifier(node)&&node.text===name,'exact original enum binding identity');}
  function property(node,member){assert(ts.isElementAccessExpression(node));identifier(node.expression);assert(ts.isStringLiteral(node.argumentExpression)&&node.argumentExpression.text===member.name,'original enum member property name');}
  const writes=[];
  for(let index=0;index<cap.members.length;index++) {
    const member=cap.members[index],node=factory.body.statements[index];assert(ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&node.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken);let write=node.expression;
    if(member.reverse){assert(ts.isElementAccessExpression(write.left));identifier(write.left.expression);assert(ts.isStringLiteral(write.right)&&write.right.text===member.name,'numeric reverse mapping points to the exact original member');write=write.left.argumentExpression;assert(ts.isBinaryExpression(write)&&write.operatorToken.kind===ts.SyntaxKind.EqualsToken);property(write.left,member);assert(ts.isNumericLiteral(write.right)&&write.right.text===String(member.value),'automatic enum member value derives from original member order');}
    else {property(write.left,member);assert(ts.isStringLiteral(write.right)&&write.right.text===member.value,'string enum value derives from its original literal');}
    writes.push({source:member,statement:node,write});
  }
  assert.equal(call.arguments.length,1);const argument=unwrap(ts,call.arguments[0]);assert(ts.isBinaryExpression(argument)&&argument.operatorToken.kind===ts.SyntaxKind.BarBarToken);identifier(argument.left);let initialization=unwrap(ts,argument.right),publication=null;
  if(options.module==='CommonJS'&&exported){publication=initialization;assert(ts.isBinaryExpression(publication)&&publication.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isPropertyAccessExpression(publication.left)&&ts.isIdentifier(publication.left.expression)&&publication.left.expression.text==='exports'&&publication.left.name.text===name,'exact enum module publication');initialization=publication.right;}
  assert(ts.isBinaryExpression(initialization)&&initialization.operatorToken.kind===ts.SyntaxKind.EqualsToken);identifier(initialization.left);assert(ts.isObjectLiteralExpression(initialization.right)&&initialization.right.properties.length===0,'enum factory initializes one empty original binding');
  return {view,variable,binding,statement,call,factory,argument,initialization,publication,writes};
}
function audit(ts,native,actual,n,a,cap,options) {
  const nn=read(ts,n,cap,options),an=read(ts,a,cap,options),pairs=[];
  function walk(x,y,label){assert.equal(y.kind,x.kind,label+' complete paired enum AST kind');pairs.push({label,native:x,actual:y,nativeOwner:native.owner(x.end),actualOwner:actual.owner(y.end)});const nx=[],ax=[];ts.forEachChild(x,node=>{nx.push(node);});ts.forEachChild(y,node=>{ax.push(node);});assert.equal(ax.length,nx.length,label+' complete enum scaffold AST child inventory');for(let i=0;i<nx.length;i++)walk(nx[i],ax[i],label+'/'+ts.SyntaxKind[nx[i].kind]+':'+i);}
  walk(nn.variable,an.variable,'enum '+cap.name+' declaration');walk(nn.statement,an.statement,'enum '+cap.name+' invocation');return {source:cap,native:nn,actual:an,pairs};
}
function compare(ts,native,actual,n,a,cap,options) {
  const proof=audit(ts,native,actual,n,a,cap,options);for(const pair of proof.pairs)assert.deepEqual(pair.actualOwner,pair.nativeOwner,'exact original enum scaffold logical endpoint '+pair.label);return proof;
}
async function execute(ts,code,options,originalText) {
  const original=ts.createSourceFile('types.ts',originalText,ts.ScriptTarget.Latest,true),enums=original.statements.filter(ts.isEnumDeclaration).map(node=>source(ts,node)),context=vm.createContext({});let namespace;
  if(options.module==='CommonJS'){const exports={},module={exports};context.exports=exports;context.module=module;vm.runInContext(code,context,{timeout:1000});namespace=module.exports;}
  else {assert.equal(options.module,'ESNext');const module=new vm.SourceTextModule(code,{context});await module.link(()=>{throw new Error('standalone original enum provider has no runtime dependencies');});await module.evaluate({timeout:1000});namespace=module.namespace;}
  const result=[];for(const cap of enums){const object=namespace[cap.name];assert(object&&typeof object==='object');const wanted={};for(const member of cap.members){wanted[member.name]=member.value;if(member.reverse)wanted[String(member.value)]=member.name;}assert.deepEqual(JSON.parse(JSON.stringify(object)),wanted,'actual enum runtime own mappings');assert.deepEqual(Object.keys(object).sort(),Object.keys(wanted).sort(),'complete enum runtime member/reverse-map key inventory');result.push({name:cap.name,values:wanted});}
  assert.equal(context.ge3Provided,1,'the actual standalone provider ran its original side effect once');return result;
}
module.exports={source,read,audit,compare,execute};
