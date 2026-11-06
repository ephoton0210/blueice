// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A closed, independent symbolic evaluator for static super reference programs.
// Physical AST nodes remain attached to every effect for a separate role proof.
const assert=require('node:assert/strict');
function read(ts,view,node,{original,bindings=new Map(),slots=[],helperReference,sourceSemantics=false,nativeHome=false,nativeClassSemantics=false}={}){
 assert(original&&node.getSourceFile()===view.source,'original source and exact emitted expression');
 if(sourceSemantics)assert(view.source===original,'source semantics only reads the original AST');
 const sourceNames=new Set();function names(n){if(ts.isIdentifier(n))sourceNames.add(n.text);ts.forEachChild(n,names);}names(original);
 const declared=new Set(),environment=new Map(bindings),effects=[],physicalEffects=[];let localBindings=new Set();
 for(const declaration of slots){assert(ts.isVariableDeclaration(declaration)&&ts.isIdentifier(declaration.name));if(node.getStart(view.source)<=declaration.getStart(view.source)&&declaration.end<=node.end)continue;assert(!sourceNames.has(declaration.name.text),'fresh generated reference temporary');declared.add(declaration.name.text);}
 const literal=(kind,value)=>({kind,value}),effect=(kind,fields,node)=>{const index=effects.length;const operation={kind,...fields,node,logicalNode:node};effects.push(operation);physicalEffects.push(operation);return {kind:'effect',index};};
 function unwrap(n){while(ts.isParenthesizedExpression(n))n=n.expression;return n;}
 function primitive(n){return n&&['string','number','boolean','null','undefined','key'].includes(n.kind);}
 function propertyKey(value,n){if(value.kind==='key')return value;if(primitive(value))return literal('string',String(value.kind==='null'?null:value.kind==='undefined'?undefined:value.value));return {kind:'key',value:effect('ToPropertyKey',{value},n)};}
 function get(name){if(environment.has(name))return environment.get(name);assert(sourceNames.has(name)||['Reflect','Object','TypeError','Symbol'].includes(name),'unbound generated reference '+name);return {kind:'source',name};}
 function write(name,value,n){assert(declared.has(name)&&!sourceNames.has(name),'assignment only to a proved fresh generated temporary');environment.set(name,value);return value;}
 function reference(n){n=unwrap(n);if(!(sourceSemantics||nativeHome)||!(ts.isPropertyAccessExpression(n)||ts.isElementAccessExpression(n))||n.expression.kind!==ts.SyntaxKind.SuperKeyword)return null;assert(bindings.has('$base')&&bindings.has('$receiver'),'typed original static home object');return {base:bindings.get('$base'),receiver:bindings.get('$receiver'),key:ts.isPropertyAccessExpression(n)?literal('string',n.name.text):nativeClassSemantics?propertyKey(value(n.argumentExpression),n.argumentExpression):value(n.argumentExpression)};}
 function load(ref,n){return effect('get',{base:ref.base,key:propertyKey(ref.key,n),receiver:ref.receiver},n);}
 function store(ref,v,n){const written=effect('set',{base:ref.base,key:propertyKey(ref.key,n),value:v,receiver:ref.receiver},n);if(nativeClassSemantics)effect('strictWrite',{written,error:'TypeError'},n);return written;}
 function sourceUpdate(n,prefix,ref){const numeric={kind:'numeric',value:load(ref,n.operand)},delta=n.operator===ts.SyntaxKind.PlusPlusToken?1:-1,next={kind:'binary',operator:'+',left:numeric,right:literal('number',delta)};store(ref,next,n);return prefix?next:numeric;}
 function statements(body){assert(ts.isBlock(body));let returned=false,result;
  for(const statement of body.statements){assert(!returned,'no dead statements in a reference helper');
   if(ts.isVariableStatement(statement)){for(const declaration of statement.declarationList.declarations){assert(ts.isIdentifier(declaration.name)&&!sourceNames.has(declaration.name.text),'fresh reference helper local');assert(!localBindings.has(declaration.name.text),'unique helper local');localBindings.add(declaration.name.text);declared.add(declaration.name.text);if(declaration.initializer)environment.set(declaration.name.text,value(declaration.initializer));}}
   else if(ts.isExpressionStatement(statement))value(statement.expression);
   else if(ts.isIfStatement(statement)){assert(nativeClassSemantics&&!statement.elseStatement&&ts.isPrefixUnaryExpression(statement.expression)&&statement.expression.operator===ts.SyntaxKind.ExclamationToken&&ts.isCallExpression(statement.expression.operand),'exact strict super-write guard');const rejected=statement.thenStatement;assert(ts.isThrowStatement(rejected)&&ts.isNewExpression(rejected.expression)&&rejected.expression.expression.getText(view.source)==='TypeError'&&rejected.expression.arguments?.length===1&&ts.isStringLiteral(rejected.expression.arguments[0])&&rejected.expression.arguments[0].text==='super assignment failed','exact strict super-write failure');const written=value(statement.expression.operand);assert(written.kind==='effect'&&effects[written.index]?.kind==='set');effect('strictWrite',{written,error:'TypeError'},statement.expression.operand);}
   else if(ts.isReturnStatement(statement)){assert(statement.expression);result=value(statement.expression);returned=true;}
   else assert.fail('unreviewed reference helper statement '+ts.SyntaxKind[statement.kind]);
  }assert(returned,'complete reference helper returns its value');return result;
 }
 function inline(fn,args){assert((ts.isArrowFunction(fn)||ts.isFunctionExpression(fn))&&!fn.name&&!fn.asteriskToken&&!fn.modifiers?.length,'plain generated reference helper');assert.equal(fn.parameters.length,args.length,'complete reference helper arguments');
  const previous=new Map(environment),previousDeclared=new Set(declared),previousLocals=localBindings;localBindings=new Set();
  for(let i=0;i<fn.parameters.length;i++){const p=fn.parameters[i];assert(ts.isIdentifier(p.name)&&!p.dotDotDotToken&&!p.initializer&&!sourceNames.has(p.name.text),'fresh plain helper formal');assert(!localBindings.has(p.name.text),'unique helper formal');localBindings.add(p.name.text);declared.add(p.name.text);environment.set(p.name.text,args[i]);}
  const result=ts.isBlock(fn.body)?statements(fn.body):value(fn.body);
  environment.clear();for(const [name,v]of previous)environment.set(name,v);declared.clear();for(const name of previousDeclared)declared.add(name);localBindings=previousLocals;return result;
 }
 function call(n){const fn=unwrap(n.expression);
  const ref=reference(fn);if(ref){const target=load(ref,fn),args=n.arguments.map(value);return effect('invoke',{target,receiver:ref.receiver,arguments:args},n);}
  if(ts.isArrowFunction(fn)||ts.isFunctionExpression(fn)){const args=n.arguments.map(value),result=inline(fn,args);let returned=result;if(result.kind==='array'&&result.values.length===4&&result.values[0].kind==='base'&&result.values[2].kind==='receiver')returned=result.values[3];if(returned.kind==='effect'&&effects[returned.index]?.kind==='get')effects[returned.index].logicalNode=n;return result;}
  if(ts.isIdentifier(fn)){const helper=require('./ge5_static_super_helpers.cjs').resolve(helperReference,ts,view,original,fn);if(helper){const args=n.arguments.map(value);if(helper==='property-key'){assert.equal(args.length,1);return propertyKey(args[0],n);}assert.equal(helper,'define-computed');assert(args.length===3&&args[0].kind==='emptyObject'&&args[2].kind==='number'&&args[2].value===0,'exact ToPropertyKey object helper use');return {kind:'computedKeyObject',key:propertyKey(args[1],n)};}}
  if(ts.isPropertyAccessExpression(fn)){
   if(nativeClassSemantics&&fn.expression.getText(view.source)==='Object'&&fn.name.text==='getPrototypeOf'){const args=n.arguments.map(value);assert(args.length===1&&args[0].kind==='receiver'&&bindings.has('$base'),'exact original static class home base');return bindings.get('$base');}
   if(fn.expression.getText(view.source)==='Reflect'){
    const args=n.arguments.map(value);
    if(fn.name.text==='get'){assert.equal(args.length,3);const key=propertyKey(args[1],n.arguments[1]);return effect('get',{base:args[0],key,receiver:args[2]},n);}
    if(fn.name.text==='set'){assert.equal(args.length,4);const key=propertyKey(args[1],n.arguments[1]);return effect('set',{base:args[0],key,value:args[2],receiver:args[3]},n);}
    if(fn.name.text==='ownKeys'){assert.equal(args.length,1);assert(args[0].kind==='computedKeyObject');return {kind:'array',values:[args[0].key]};}
    assert.fail('unreviewed Reflect operation '+fn.name.text);
   }
   if(fn.name.text==='call'){const target=value(fn.expression),args=n.arguments.map(value);assert(args.length);return effect('invoke',{target,receiver:args[0],arguments:args.slice(1)},n);}
  }
  const target=value(fn),args=n.arguments.map(value);assert(target.kind==='source','only exact original source calls are opaque');return effect('sourceCall',{target,arguments:args},n);
 }
 function update(n,prefix){const operand=n.operand;assert(ts.isIdentifier(operand));const old=get(operand.text),numeric={kind:'numeric',value:old},delta=n.operator===ts.SyntaxKind.PlusPlusToken?1:-1,next={kind:'binary',operator:'+',left:numeric,right:literal('number',delta)};write(operand.text,next,n);return prefix?next:numeric;}
 function value(n){n=unwrap(n);
  if(ts.isIdentifier(n))return get(n.text);
  if(n.kind===ts.SyntaxKind.ThisKeyword){assert(bindings.has('this'),'receiver identity is independently proved');return bindings.get('this');}
  if(ts.isStringLiteral(n)||ts.isNoSubstitutionTemplateLiteral(n))return literal('string',n.text);
  if(ts.isNumericLiteral(n))return literal('number',Number(n.text));
  if(n.kind===ts.SyntaxKind.NullKeyword)return {kind:'null'};
  if(n.kind===ts.SyntaxKind.TrueKeyword||n.kind===ts.SyntaxKind.FalseKeyword)return literal('boolean',n.kind===ts.SyntaxKind.TrueKeyword);
  if(ts.isVoidExpression(n)){assert(ts.isNumericLiteral(n.expression)&&n.expression.text==='0');return {kind:'undefined'};}
  if(ts.isCallExpression(n))return call(n);
  if(ts.isArrayLiteralExpression(n)){assert(n.elements.every(e=>!ts.isSpreadElement(e)&&!ts.isOmittedExpression(e)));return {kind:'array',values:n.elements.map(value)};}
  if(ts.isObjectLiteralExpression(n)){if(n.properties.length===0)return {kind:'emptyObject'};assert.equal(n.properties.length,1);const p=n.properties[0];assert(ts.isPropertyAssignment(p)&&ts.isComputedPropertyName(p.name)&&ts.isNumericLiteral(p.initializer)&&p.initializer.text==='0','complete ToPropertyKey object');return {kind:'computedKeyObject',key:propertyKey(value(p.name.expression),p.name.expression)};}
  if(ts.isElementAccessExpression(n)){const ref=reference(n);if(ref)return load(ref,n);const target=value(n.expression),key=value(n.argumentExpression);if(target.kind==='array'){assert(key.kind==='number'&&Number.isInteger(key.value)&&key.value>=0&&key.value<target.values.length);return target.values[key.value];}return {kind:'element',target,key};}
  if(ts.isPropertyAccessExpression(n)){const ref=reference(n);if(ref)return load(ref,n);return {kind:'property',target:value(n.expression),name:n.name.text};}
  if(ts.isPrefixUnaryExpression(n)){if([ts.SyntaxKind.PlusPlusToken,ts.SyntaxKind.MinusMinusToken].includes(n.operator)){const ref=reference(n.operand);return ref?sourceUpdate(n,true,ref):update(n,true);}return {kind:'unary',operator:ts.tokenToString(n.operator),operand:value(n.operand)};}
  if(ts.isPostfixUnaryExpression(n)){const ref=reference(n.operand);return ref?sourceUpdate(n,false,ref):update(n,false);}
  if(ts.isBinaryExpression(n)){
   const op=n.operatorToken.kind;
   const ref=reference(n.left);
   if(ref&&op>=ts.SyntaxKind.FirstAssignment&&op<=ts.SyntaxKind.LastAssignment){let result;if(op===ts.SyntaxKind.EqualsToken)result=value(n.right);else{const old=load(ref,n.left),operator=ts.tokenToString(op).slice(0,-1);if(['||','&&','??'].includes(operator)){const prior=effects.length,right=value(n.right);if(nativeClassSemantics){store(ref,right,n);const branch=effects.splice(prior);return {kind:'choice',operator,left:old,right,effects:branch.map(({node,logicalNode,...effect})=>effect)};}const branch=effects.splice(prior);result={kind:'choice',operator,left:old,right,effects:branch.map(({node,logicalNode,...effect})=>effect)};}else result={kind:'binary',operator,left:old,right:value(n.right)};}store(ref,result,n);return result;}
   if(op===ts.SyntaxKind.CommaToken){value(n.left);return value(n.right);}
   if(op===ts.SyntaxKind.EqualsToken){assert(ts.isIdentifier(n.left));return write(n.left.text,value(n.right),n);}
   if([ts.SyntaxKind.BarBarToken,ts.SyntaxKind.AmpersandAmpersandToken,ts.SyntaxKind.QuestionQuestionToken].includes(op)){
    const left=value(n.left),prior=effects.length,right=value(n.right),branch=effects.splice(prior);return {kind:'choice',operator:ts.tokenToString(op),left,right,effects:branch.map(({node,logicalNode,...effect})=>effect)};
   }
   return {kind:'binary',operator:ts.tokenToString(op),left:value(n.left),right:value(n.right)};
  }
  if(ts.isConditionalExpression(n)){const test=value(n.condition),prior=effects.length,yes=value(n.whenTrue),yesEffects=effects.splice(prior),no=value(n.whenFalse),noEffects=effects.splice(prior),same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);if(test.kind==='choice'&&test.operator==='&&'&&test.effects.length===0&&test.left.kind==='binary'&&test.left.operator==='!=='&&test.left.right.kind==='null'&&test.right.kind==='binary'&&test.right.operator==='!=='&&test.right.right.kind==='undefined'&&same(test.left.left,test.right.left)&&same(yes,test.left.left)&&yesEffects.length===0)return {kind:'choice',operator:'??',left:yes,right:no,effects:noEffects.map(({node,logicalNode,...e})=>e)};return {kind:'conditional',test,yes,no,yesEffects:yesEffects.map(({node,logicalNode,...e})=>e),noEffects:noEffects.map(({node,logicalNode,...e})=>e)};}
  assert.fail('unreviewed reference expression '+ts.SyntaxKind[n.kind]);
 }
 const result=value(node),semantic={effects:effects.map(({node,logicalNode,...effect})=>effect),result};return {node,effects,physicalEffects,result,semantic,sourceNames};
}
function compare(ts,native,n,actual,a,options){const expected=read(ts,native,n,options.native),observed=read(ts,actual,a,options.actual);assert.deepEqual(observed.semantic,expected.semantic,'complete static-super evaluation, captures, key conversion and receiver flow');return {native:expected,actual:observed};}
module.exports={read,compare};
