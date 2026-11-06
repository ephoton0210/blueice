// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A lexical receiver is granted only by its exact class-field/factory binding chain.
const assert=require('node:assert/strict'),classes=require('./ge5_class_roles.cjs');
function contains(parent,node){for(let current=node;current;current=current.parent)if(current===parent)return true;return false;}
function proof(ts,item){const {view,origin,expression,callback,property,driver,blue}=item,start=node=>node.getStart(view.source),names=new Set();function collect(node){if(ts.isIdentifier(node))names.add(node.text);ts.forEachChild(node,collect);}collect(origin.file);
  assert(ts.isPropertyAccessExpression(origin.arrow.body.left)&&origin.arrow.body.left.expression.kind===ts.SyntaxKind.ThisKeyword&&origin.arrow.body.left.name.text==='value');assert(!origin.box.members.some(ts.isConstructorDeclaration));
  const classNodes=view.source.statements.filter(node=>ts.isClassDeclaration(node)&&node.name?.text==='Box'||ts.isVariableStatement(node)&&node.declarationList.declarations.some(d=>d.name.getText(view.source)==='Box'&&d.initializer));assert.equal(classNodes.length,1);const klass=classes.readClass(ts,view,classNodes[0],origin.box);
  const constructor=klass.native?klass.node.members.find(ts.isConstructorDeclaration):klass.constructor;assert(constructor&&contains(constructor,expression),'lowered lexical receiver is initialized in the exact Box constructor');
  let capture=null;
  if(blue){const factory=item.factory,invocation=item.capture.invocation;assert(ts.isFunctionExpression(factory)&&factory.parameters.length===2&&invocation.arguments.length===2&&invocation.arguments[0].kind===ts.SyntaxKind.ThisKeyword);assert.strictEqual(invocation.expression.expression??invocation.expression,factory);
    const [receiver,arguments_]=factory.parameters.map(p=>{assert(ts.isIdentifier(p.name)&&!p.initializer&&!p.dotDotDotToken);return p.name;});assert(!names.has(receiver.text)&&!names.has(arguments_.text)&&receiver.text!==arguments_.text);assert.strictEqual(property.expression.getSourceFile(),view.source);assert(ts.isIdentifier(property.expression)&&property.expression.text===receiver.text);
    const refs=new Map([[receiver.text,[]],[arguments_.text,[]]]);function references(node){if(ts.isIdentifier(node)&&refs.has(node.text))refs.get(node.text).push(node);ts.forEachChild(node,references);}references(invocation);
    assert.deepEqual(refs.get(receiver.text).map(start).sort((a,b)=>a-b),[receiver,property.expression].map(start).sort((a,b)=>a-b),'complete lexical receiver declaration/read inventory');assert.deepEqual(refs.get(arguments_.text),[arguments_],'unused original lexical arguments cannot replace call arguments');
    assert.strictEqual(factory.body.statements[0].expression,item.entryFunction);assert.strictEqual(driver.arguments[2],callback);assert.equal(driver.arguments[0].kind,ts.SyntaxKind.ThisKeyword);assert(ts.isIdentifier(driver.arguments[1])&&driver.arguments[1].text==='arguments');assert(contains(item.entryFunction,driver));
  }else{
    assert(property.expression.kind===ts.SyntaxKind.ThisKeyword);assert.strictEqual(driver.arguments[3],callback);
    if(ts.isIdentifier(driver.arguments[0])){const statement=constructor.body.statements[0];assert(ts.isVariableStatement(statement)&&statement.declarationList.declarations.length===1);const declaration=statement.declarationList.declarations[0];assert(ts.isIdentifier(declaration.name)&&declaration.initializer?.kind===ts.SyntaxKind.ThisKeyword&&!names.has(declaration.name.text));assert.equal(driver.arguments[0].text,declaration.name.text);
      const refs=[];function references(node){if(ts.isIdentifier(node)&&node.text===declaration.name.text)refs.push(node);ts.forEachChild(node,references);}references(constructor);assert.deepEqual(refs.map(start).sort((a,b)=>a-b),[declaration.name,driver.arguments[0]].map(start).sort((a,b)=>a-b),'complete Native constructor receiver capture binding');
      const owner=view.owner(start(classNodes[0]));assert(owner);for(let offset=start(statement);offset<statement.end;offset++)assert.deepEqual(view.owner(offset),owner,'complete Native constructor-entry receiver phase');view.cover(start(statement),statement.end);(view.roleStarts??=new Map()).set(start(statement),owner);(view.roleEnds??=new Map()).set(statement.end,view.owner(statement.end));
      assert.deepEqual(view.owner(statement.end),view.owner(classNodes[0].end),'Native receiver capture retains class logical completion');capture={source:origin.arrow,native:statement,nativeUse:driver.arguments[0]};
    }else assert.equal(driver.arguments[0].kind,ts.SyntaxKind.ThisKeyword);
  }
  assert(contains(callback,property));return Object.freeze({kind:'closed-class-field-lexical-receiver-v1',item,read:property.expression,capture});
}
function bind(ts,machine,capability){assert.equal(capability?.kind,'closed-class-field-lexical-receiver-v1');const {item,read}=capability;assert.strictEqual(item.view,machine.view);assert.strictEqual(item.callback,machine.node);assert(machine.originals.includes(item.origin.file.text));assert(contains(machine.callback,read),'receiver read is within the exact source dispatcher');
  // Revalidate the whole binding proof rather than accepting a caller's offsets.
  const checked=proof(ts,item);assert.strictEqual(checked.read,read);
  let conciseResultGroup=null;
  if(machine.blue){
    assert(ts.isBinaryExpression(item.origin.arrow.body),'the exact source arrow has a concise binary body');
    const results=[];
    for(const statements of machine.states.values())for(const statement of statements){
      if(!ts.isReturnStatement(statement)||!statement.expression||!ts.isObjectLiteralExpression(statement.expression))continue;
      const properties=statement.expression.properties;assert(properties.every(ts.isPropertyAssignment));
      const kind=properties.find(property=>property.name.getText(machine.view.source)==='kind'),value=properties.find(property=>property.name.getText(machine.view.source)==='value');
      if(kind&&ts.isStringLiteral(kind.initializer)&&kind.initializer.text==='return'&&value&&!ts.isVoidExpression(value.initializer))results.push(value.initializer);
    }
    assert.equal(results.length,1,'one explicit concise-arrow result completion');
    const outer=results[0];assert(ts.isParenthesizedExpression(outer)&&ts.isParenthesizedExpression(outer.expression));
    conciseResultGroup=outer.expression;assert(ts.isIdentifier(conciseResultGroup.expression)&&machine.slots.has(conciseResultGroup.expression.text),'concise return grouping encloses the exact completed value slot');
  }
  return {read,role:'lexical class-field receiver',conciseResultGroup};
}
module.exports={proof,bind,contains};
