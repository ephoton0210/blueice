// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Validate the entire external iterator adapter before exposing its source DFA.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const asset=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/es5/suspension/native_abi.v1.js'),'utf8');
function reference(ts,node,capability,identifiers,expected='__generator') {
  assert(capability&&capability.member===expected&&['__generator','__values'].includes(expected),'reviewed exact helper capability');
  assert.equal(Boolean(capability.namespace),!Boolean(capability.binding),'one lexical helper reference');
  if(capability.namespace) {
    assert(!identifiers.has(capability.namespace),'fresh helper namespace from the module import proof');
    assert(ts.isPropertyAccessExpression(node)&&!node.questionDotToken&&ts.isIdentifier(node.expression));
    assert.equal(node.expression.text,capability.namespace,'exact imported helper namespace');assert.equal(node.name.text,capability.member,'exact imported helper member');
  } else {assert(ts.isIdentifier(node));assert.equal(node.text,capability.binding,'exact global/named helper binding');assert(!identifiers.has(node.text),'generated or imported helper cannot shadow a source binding');}
}
function read(ts,view,node,originals,options,identifiers,bindings) {
  const capability=options.generatorReference;assert.equal(capability.abi,'tslib-adapter');
  const switched=options.exceptions==='switch-delegate-finally';if(switched)require('./ge5_generator_switch_machine.cjs').adapter(options.switchContract,ts,view,node,options);
  const regionProfile=['normal-finally','pending-return-finally'].includes(options.exceptions)||switched;assert(regionProfile||options.exceptions===undefined,'reviewed complete source DFA');if(!switched)assert(!options.delegation);if(options.asynchronous)assert(options.exceptions==='normal-finally'&&options.awaitReference?.member==='__await');
  const start=node=>node.getStart(view.source),core=options.bodyStatements??node.body.statements,body=[...core],returned=body.pop();
  assert(ts.isReturnStatement(returned)&&ts.isCallExpression(returned.expression));const call=returned.expression;reference(ts,call.expression,capability,identifiers);
  assert.equal(call.arguments.length,2);assert.equal(call.arguments[0].kind,ts.SyntaxKind.ThisKeyword);
  const callback=call.arguments[1];assert(ts.isFunctionExpression(callback)&&!callback.name&&!callback.asteriskToken&&!callback.modifiers?.length&&callback.parameters.length===1);
  const parameter=callback.parameters[0];assert(ts.isIdentifier(parameter.name)&&!parameter.initializer&&!parameter.dotDotDotToken&&!parameter.type);
  const native=parameter.name.text,used=new Set();
  function fresh(name,label){assert(typeof name==='string'&&!identifiers.has(name)&&!used.has(name),label+' fresh lexical identity');used.add(name);return name;}
  fresh(native,'native continuation');
  function declaration(statement,label){assert(ts.isVariableStatement(statement)&&(statement.declarationList.flags&ts.NodeFlags.BlockScoped)===0&&statement.declarationList.declarations.length===1,label);const declaration=statement.declarationList.declarations[0];assert(ts.isIdentifier(declaration.name));return declaration;}
  const callbackBody=[...callback.body.statements];assert.equal(callbackBody.length,options.asynchronous?4:3,'complete callback statement inventory');if(options.asynchronous)callbackBody.shift();
  const contextDeclaration=declaration(callbackBody[0],'source context declaration'),context=fresh(contextDeclaration.name.text,'source continuation');
  const contextValue=contextDeclaration.initializer;assert(ts.isObjectLiteralExpression(contextValue)&&contextValue.properties.length===2&&contextValue.properties.every(ts.isPropertyAssignment));assert.deepEqual(contextValue.properties.map(p=>p.name.getText(view.source)),['label','sent']);
  const initial=contextValue.properties[0].initializer;assert(ts.isConditionalExpression(initial)&&ts.isNumericLiteral(initial.whenTrue)&&ts.isBinaryExpression(initial.whenFalse)&&ts.isNumericLiteral(initial.whenFalse.left));
  const entry=Number(initial.whenTrue.text),count=Number(initial.whenFalse.left.text);assert(Number.isSafeInteger(count)&&count>0&&count<=512&&Number.isSafeInteger(entry)&&entry>=0&&entry<count);
  const actionDeclaration=declaration(callbackBody[1],'source action declaration'),action=fresh(actionDeclaration.name.text,'adapter action');
  const invocation=actionDeclaration.initializer;assert(ts.isCallExpression(invocation)&&ts.isPropertyAccessExpression(invocation.expression)&&invocation.expression.name.text==='call');assert.equal(invocation.arguments.length,1);assert.equal(invocation.arguments[0].kind,ts.SyntaxKind.ThisKeyword);
  let closure=invocation.expression.expression;while(ts.isParenthesizedExpression(closure))closure=closure.expression;
  assert(ts.isFunctionExpression(closure)&&!closure.name&&!closure.asteriskToken&&!closure.modifiers?.length&&closure.parameters.length===0&&closure.body.statements.length===1);
  const loop=closure.body.statements[0];assert(ts.isWhileStatement(loop)&&loop.expression.kind===ts.SyntaxKind.TrueKeyword&&ts.isBlock(loop.statement)&&loop.statement.statements.length===1);
  const dispatcher=loop.statement.statements[0];assert(ts.isSwitchStatement(dispatcher)&&dispatcher.expression.getText(view.source)===context+'.label');
  const states=new Map(),sync=[],sourceStatements=[],clauses=[...dispatcher.caseBlock.clauses];assert.equal(clauses.length,count+1);
  const numbers=[];
  function kind(statement){if(!ts.isReturnStatement(statement)||!ts.isObjectLiteralExpression(statement.expression))return null;const property=statement.expression.properties.find(p=>ts.isPropertyAssignment(p)&&p.name.getText(view.source)==='kind');return property&&ts.isStringLiteral(property.initializer)?property.initializer.text:null;}
  function syncStatement(statement){return ts.isExpressionStatement(statement)&&ts.isBinaryExpression(statement.expression)&&statement.expression.left.getText(view.source)===native+'.label';}
  const cases=[];
  for(let index=0;index<count;index++) {
    const clause=clauses[index];assert(ts.isCaseClause(clause)&&ts.isNumericLiteral(clause.expression));const label=Number(clause.expression.text);assert.equal(label,index,'dense ordered source states');numbers.push(label);
    const physical=[...clause.statements];assert(physical.length);const terminal=kind(physical.at(-1)),noSync=['leave','finish'].includes(terminal);
    let statements=physical;
    if(noSync)assert(!syncStatement(physical[0]),'leave/finish preserve the active native region label');
    else {const position=physical[0];assert(syncStatement(position),'every ordinary state starts with native label synchronization');const assignment=position.expression;assert.equal(assignment.operatorToken.kind,ts.SyntaxKind.EqualsToken);assert(ts.isNumericLiteral(assignment.right));assert.equal(Number(assignment.right.text),count-label,'reversed native state synchronization');sync.push(position);statements=physical.slice(1);}
    assert(statements.length,'state contains its complete source operation');
    for(const statement of statements){const names=[];function visit(node){if(ts.isIdentifier(node)&&node.text===native)names.push(node);ts.forEachChild(node,visit);}visit(statement);assert.equal(names.length,0,'native context cannot leak into the source DFA');sourceStatements.push(statement);}
    states.set(label,statements);cases.push('case '+label+': '+(noSync?'':native+'.label='+String(count-label)+';')+statements.map(statement=>statement.getText(view.source)).join('\n'));
  }
  const fallback=clauses.at(-1);assert(ts.isDefaultClause(fallback));assert(states.has(entry));
  const exitsStatement=body.pop(),pendingStatement=body.pop(),exitsDeclaration=declaration(exitsStatement,'normalized exit table'),pendingDeclaration=declaration(pendingStatement,'native pending yield');
  const exits=fresh(exitsDeclaration.name.text,'normalized exits'),pending=fresh(pendingDeclaration.name.text,'pending yield');assert.equal(pendingDeclaration.initializer.kind,ts.SyntaxKind.FalseKeyword);
  const table=exitsDeclaration.initializer;assert(ts.isObjectLiteralExpression(table)&&table.properties.every(ts.isPropertyAssignment));
  const entries=[];
  for(const [label,statements]of states)if(kind(statements.at(-1))==='enter') {
    const value=statements.at(-1).expression;assert.deepEqual(value.properties.map(p=>p.name.getText(view.source)),['kind','entry','caught','finalizer','after']);const after=value.properties[4].initializer;assert(ts.isNumericLiteral(after)&&states.has(Number(after.text)));entries.push({label,after:Number(after.text)});
  }
  assert.equal(entries.length,regionProfile?1:0,'complete reviewed region inventory');
  const after=entries[0]?.after,afterStatements=after===undefined?[]:states.get(after);if(after!==undefined){assert.equal(afterStatements.length,1);assert.equal(afterStatements[0].getText(view.source).replace(/\s/g,''),'return{kind:"return",value:void0};');}
  const expectedExits={};for(const [label,statements]of states)if(kind(statements.at(-1))==='leave')expectedExits[label]=count-after;
  const foundExits={};for(const property of table.properties){assert(ts.isStringLiteral(property.name)&&/^(?:0|[1-9]\d*)$/.test(property.name.text)&&ts.isNumericLiteral(property.initializer));const label=Number(property.name.text);assert(states.has(label)&&!Object.hasOwn(foundExits,label));foundExits[label]=Number(property.initializer.text);}
  assert.deepEqual(foundExits,expectedExits,'complete transitively normalized exit inventory');
  const argumentsStatement=body.shift(),argumentsDeclaration=declaration(argumentsStatement,'captured arguments'),argumentsName=fresh(argumentsDeclaration.name.text,'captured arguments');assert(ts.isIdentifier(argumentsDeclaration.initializer)&&argumentsDeclaration.initializer.text==='arguments');
  const slots=new Set(),userSlots=new Set();
  for(const statement of body){assert(ts.isVariableStatement(statement)&&(statement.declarationList.flags&ts.NodeFlags.BlockScoped)===0);for(const declaration of statement.declarationList.declarations){assert(ts.isIdentifier(declaration.name)&&!declaration.initializer);const name=declaration.name.text;if(bindings.has(name)){assert(!used.has(name));used.add(name);userSlots.add(name);}else fresh(name,'carried local');slots.add(name);}}
  for(const name of slots){const reads=[];for(const statement of sourceStatements){function visit(node){if(ts.isIdentifier(node)&&node.text===name&&!(ts.isBinaryExpression(node.parent)&&node.parent.left===node&&ts.isAssignmentOperator(node.parent.operatorToken.kind)))reads.push(node);ts.forEachChild(node,visit);}visit(statement);}assert(reads.length,'every declared carried slot is read by the complete source graph');}
  function words(source,node){const result=[];function visit(node){if(ts.isToken(node)){if(node.kind!==ts.SyntaxKind.EndOfFileToken)result.push(node.getText(source));return;}for(const child of node.getChildren(source))visit(child);}visit(node);return result;}
  const awaiting=options.awaitReference,awaitText=awaiting?.namespace?awaiting.namespace+'.'+awaiting.member:awaiting?.binding;
  const pendingBranch=options.asynchronous?`if (${pending}) { ${pending}=false; ${native}.label--; return [4,${native}.sent()]; }`:'';
  const yielding=options.asynchronous?`${pending} = !${action}.value.awaiting; return [4,new ${awaitText}(${action}.value.value)];`:`return [4,${action}.value];`;
  const values=switched?options.valuesReference:null,valuesText=values?.namespace?values.namespace+'.'+values.member:values?.binding;
  const delegate=switched?`case "delegate": ${native}.label = ${count} - ${context}.label - 1; return [5,${valuesText}(${action}.value)];`:'';
  if(switched){assert(values?.member==='__values');const declaration=ts.createSourceFile('bound-values.js',valuesText,ts.ScriptTarget.Latest,true);reference(ts,declaration.statements[0].expression,values,identifiers,'__values');}
  const expected=asset.replaceAll('__BLUEICE_PENDING_BRANCH__',pendingBranch).replaceAll('__BLUEICE_CASES__',cases.join('\n')).replaceAll('__BLUEICE_DELEGATE__',delegate).replaceAll('__BLUEICE_YIELD__',yielding).replaceAll('__BLUEICE_SOURCE__',context).replaceAll('__BLUEICE_NATIVE__',native).replaceAll('__BLUEICE_ACTION__',action).replaceAll('__BLUEICE_EXITS__',exits).replaceAll('__BLUEICE_COUNT__',String(count)).replaceAll('__BLUEICE_ENTRY__',String(entry));
  const expectedSource=ts.createSourceFile('native-adapter-contract.js',expected,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);assert.equal(expectedSource.parseDiagnostics.length,0);assert.deepEqual(words(view.source,callback.body).slice(1,-1),words(expectedSource,expectedSource),'complete versioned external generator adapter AST');
  const ranges=sourceStatements.map(statement=>({start:start(statement),end:statement.end,node:statement})).sort((x,y)=>x.start-y.start);let cursor=core[0].getStart(view.source),previous=null;
  function generated(statement){return ts.isContinueStatement(statement)||['enter','leave','finish'].includes(kind(statement))||statement===afterStatements[0]||ts.isExpressionStatement(statement)&&ts.isBinaryExpression(statement.expression)&&statement.expression.left.getText(view.source)===context+'.label'&&ts.isNumericLiteral(statement.expression.right);}
  function transport(from,to,previous){if(previous&&!generated(previous))while(from<to&&/\s/.test(view.code[from]))from++;if(from<to)view.noSource(from,to,'generated external continuation adapter transport');}
  for(const range of ranges){transport(cursor,range.start,previous);cursor=range.end;previous=range.node;}
  transport(cursor,node.body.end,previous);
  return {view,node,returned,call,callback,dispatcher,context,slots,userSlots,states,entry,blue:true,identifiers,originals,computed:Boolean(options.computed),switch:Boolean(options.switch),exceptions:options.exceptions||null,loop:Boolean(options.loop),delegation:switched,valuesReference:values,helper:call.expression.getText(view.source),adapter:{native,action,exits,pending,argumentsName,count,sync,table,sourceStatements}};
}
module.exports={read,reference};
