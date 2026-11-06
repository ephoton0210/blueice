// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Class roles are derived from independent complete Native/Blue syntax trees.
const assert=require('node:assert/strict');
const {compareTokens,compareGap}=require('./ge5_role_stream.cjs');
function unwrap(ts,node) {while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function readClass(ts,view,node,original,{excludedNodes=[],generatedConstructor=null}={}) {
  const name=generatedConstructor?.name?.text??original.name?.text;assert(name,'class binding identity');
  if(generatedConstructor) {
    assert(ts.isClassExpression(original),'generated class binding belongs to an original class expression');
    assert(ts.isFunctionDeclaration(generatedConstructor)&&generatedConstructor.getSourceFile()===view.source&&generatedConstructor.name,'exact generated constructor syntax node');
    const identifiers=new Set();function names(node){if(ts.isIdentifier(node))identifiers.add(node.text);ts.forEachChild(node,names);}names(original.getSourceFile());
    assert(!identifiers.has(name),'generated constructor binding is fresh in the original program');
  }
  if(ts.isClassDeclaration(node)||ts.isClassExpression(node)) {
    assert.equal(node.name?.text,name,'original class binding');
    return {view,node,name,native:true,methods:node.members};
  }
  let declaration=node;
  if(generatedConstructor) {
    assert(ts.isCallExpression(node),'generated class proof supplies the complete factory call');
    declaration={name:generatedConstructor.name,initializer:node};
  } else if(ts.isVariableStatement(node)) {
    assert.equal(node.declarationList.declarations.length,1);
    declaration=node.declarationList.declarations[0];
  }
  if(ts.isExpressionStatement(node)||ts.isBinaryExpression(node)) {
    const expression=ts.isExpressionStatement(node)?node.expression:node;assert(ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(expression.left)&&expression.left.text===name,'System class assignment');declaration={name:expression.left,initializer:expression.right};
  }else assert(generatedConstructor||ts.isVariableDeclaration(declaration),'class variable declaration');
  assert(ts.isIdentifier(declaration.name)&&declaration.name.text===name);
  const initial=unwrap(ts,declaration.initializer);
  if(ts.isClassExpression(initial))return readClass(ts,view,initial,original);
  const call=initial;assert(ts.isCallExpression(call));
  const factory=unwrap(ts,call.expression);assert(ts.isFunctionExpression(factory)&&!factory.name&&!factory.asteriskToken);
  assert(factory.parameters.every(p=>ts.isIdentifier(p.name)&&!p.initializer&&!p.dotDotDotToken));
  const derived=!!original.heritageClauses?.length;
  assert.equal(factory.parameters.length,derived?1:0);assert.equal(call.arguments.length,derived?1:0);
  const base=derived?factory.parameters[0].name.text:null;
  if(base) {
    const sourceNames=new Set();function identifiers(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,identifiers);}identifiers(original.getSourceFile());
    assert(!sourceNames.has(base),'fresh class heritage transport');
  }
  const statements=[...factory.body.statements],returned=statements.pop();
  assert(ts.isReturnStatement(returned)&&ts.isIdentifier(returned.expression)&&returned.expression.text===name,'complete class return');
  const constructors=statements.filter(ts.isFunctionDeclaration);assert.equal(constructors.length,1);
  const constructor=constructors[0];assert.equal(constructor.name.text,name);if(generatedConstructor)assert.equal(constructor,generatedConstructor,'factory contains exactly the supplied generated constructor');
  let inheritance=null;const methods=[];
  for(const statement of statements) {
    if(statement===constructor)continue;
    if(excludedNodes.includes(statement)){for(let offset=statement.getStart(view.source);offset<statement.end;offset++)assert(view.covered(offset),'delegated decorator statement must already have complete role coverage');continue;}
    assert(ts.isExpressionStatement(statement));const expression=statement.expression;
    if(ts.isCallExpression(expression)&&expression.arguments.length===2&&ts.isIdentifier(expression.arguments[0])&&expression.arguments[0].text===name&&ts.isIdentifier(expression.arguments[1])&&expression.arguments[1].text===base) {
      assert(!inheritance);inheritance=statement;continue;
    }
    if(ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken) {
      const target=expression.left;assert(ts.isPropertyAccessExpression(target)||ts.isElementAccessExpression(target));
      const fn=expression.right;assert(ts.isFunctionExpression(fn)&&!fn.name&&!fn.asteriskToken);
      methods.push({statement,target,fn,operation:expression.operatorToken,key:ts.isPropertyAccessExpression(target)?target.name:target.argumentExpression,descriptor:false});continue;
    }
    assert(ts.isCallExpression(expression)&&expression.expression.getText(view.source)==='Object.defineProperty'&&expression.arguments.length===3,'complete class member definition');
    const [target,key,descriptor]=expression.arguments;assert(ts.isObjectLiteralExpression(descriptor));
    const fields=descriptor.properties;
    assert(fields.every(ts.isPropertyAssignment));
    const labels=fields.map(p=>p.name.getText(view.source));
    const functionField=fields.find(property=>['value','get','set'].includes(property.name.getText(view.source)));assert(functionField);
    const kind=functionField.name.getText(view.source);
    assert.deepEqual(labels,labels[0]==='enumerable'?['enumerable','configurable','writable','value']:kind==='value'?['value','writable','enumerable','configurable']:[kind,'enumerable','configurable']);
    for(const property of fields.filter(property=>property!==functionField))assert.equal(property.initializer.kind,property.name.getText(view.source)==='enumerable'?ts.SyntaxKind.FalseKeyword:ts.SyntaxKind.TrueKeyword);
    const fn=functionField.initializer;assert(ts.isFunctionExpression(fn)&&!fn.name&&!fn.asteriskToken);
    const operation=functionField.getChildren(view.source).find(n=>n.kind===ts.SyntaxKind.ColonToken);assert(operation);
    methods.push({statement,target,key,fn,operation,descriptor:true,kind});
  }
  assert.equal(!!inheritance,derived,'complete heritage invocation');
  return {view,node,declaration,call,factory,base,constructor,inheritance,methods,returned,name,excludedNodes,native:false};
}
function compareClass(ts,native,nativeNode,actual,actualNode,original,options={}) {
  const n=readClass(ts,native,nativeNode,original,{excludedNodes:options.classRoles?.nativeExcluded||[],generatedConstructor:options.classRoles?.generatedBindings?.native??null}),a=readClass(ts,actual,actualNode,original,{excludedNodes:options.classRoles?.actualExcluded||[],generatedConstructor:options.classRoles?.generatedBindings?.actual??null});
  const holes=require('./ge5_class_field_holes.cjs').create(ts,native,actual,n,a,original,options.classRoles?.pairedExpressions??[],options.classRoles?.nativeConstructorReceiverCapture??null);
  const callables=require('./ge5_class_method_holes.cjs').create(ts,native,actual,n,a,original,options.classRoles?.pairedCallables??[]);
  const generated=[];let restProof=null,declarationChain=[];
  function constructionEndpoint() {
    if(ts.isClassDeclaration(original))declarationChain=require('./ge5_class_declaration_chain.cjs').compare(ts,native,nativeNode,n,actual,actualNode,a);
  }
  function noSource(view,start,end,label){view.noSource(start,end,label);if(view===actual)generated.push([start,end]);}
  function uniform(view,start,end,owner,label) {for(let i=start;i<end;i++)assert.deepEqual(view.owner(i),owner,label+' at '+i);view.cover(start,end);(view.roleStarts??=new Map()).set(start,owner);}
  function token(nt,at,label,rename=false) {
    assert.equal(rename?nt.text:at.text,nt.text,label+' spelling');
    if(rename||nt.owners.length!==at.owners.length) {
      const owner=nt.owners[0];assert(nt.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),label+' uniform Native role');
      assert(at.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),label+' complete transformed UTF-16 role');
    } else assert.deepEqual(at.owners,nt.owners,label+' complete UTF-16 role');
    native.cover(nt.start,nt.end);actual.cover(at.start,at.end);(actual.nativeStarts??=new Map()).set(at.start,nt.start);(actual.nativeEnds??=new Map()).set(at.end,nt.end);
    (native.roleEnds??=new Map()).set(nt.end,native.owner(nt.end));(actual.roleEnds??=new Map()).set(at.end,native.owner(nt.end));(native.roleStarts??=new Map()).set(nt.start,native.owner(nt.start));(actual.roleStarts??=new Map()).set(at.start,native.owner(nt.start));
  }
  function pair(wanted,observed,label,aliases=new Map()) {
    wanted=callables.project(native,holes.project(native,wanted));observed=callables.project(actual,holes.project(actual,observed));
    assert.equal(observed.length,wanted.length,label+' complete ordered token inventory');
    for(let i=0;i<wanted.length;i++) {
      const nt=wanted[i],at=observed[i],alias=aliases.get(at.text);
      assert(!alias||alias===nt.text,label+' coherent fresh binding');
      if(nt.methodHole||at.methodHole){assert.equal(at.text,nt.text,label+' typed method identity');callables.tokensPaired(nt,at);}
      else if(nt.fieldHole||at.fieldHole){assert.equal(at.text,nt.text,label+' typed initializer identity');holes.paired(nt,at);}else token(nt,at,label+' token '+i,!!alias);
      if(i&&wanted[i-1].end<=nt.start&&observed[i-1].end<=at.start)compareGap(native,wanted[i-1].end,nt.start,actual,observed[i-1].end,at.start,label+' ordered gap '+i,generated);
    }
  }
  if(n.native||a.native) {
    assert(n.native&&a.native,'both class targets retain class syntax');
    const relocated=options.classRoles?.actualHeritageExpression;
    if(relocated) {
      const nh=n.node.heritageClauses[0].types[0].expression,ah=a.node.heritageClauses[0].types[0].expression;assert(ts.isIdentifier(ah));
      pair(native.tokens(n.node).filter(t=>t.end<=nh.getStart(native.source)),actual.tokens(a.node).filter(t=>t.end<=ah.getStart(actual.source)),'retained class heritage prefix');
      pair(native.tokens(nh),actual.tokens(relocated),'complete relocated heritage expression');
      uniform(actual,ah.getStart(actual.source),ah.end,native.owner(nh.getStart(native.source)),'captured heritage forwarding binding');(actual.roleEnds??=new Map()).set(ah.end,native.owner(nh.end));
      pair(native.tokens(n.node).filter(t=>t.start>=nh.end),actual.tokens(a.node).filter(t=>t.start>=ah.end),'retained class body after captured heritage');
    }else pair(native.tokens(n.node),actual.tokens(a.node),'complete retained class');
    constructionEndpoint();
    return {native:n,actual:a,generated,declarationChain,delegatedExpressions:holes,delegatedCallables:callables};
  }
  const aliases=new Map(n.base?[[a.base,n.base]]:[]);
  if(options.classRoles?.generatedBindings){assert(options.classRoles.generatedBindings.native&&options.classRoles.generatedBindings.actual,'both generated constructor identities are supplied');aliases.set(a.name,n.name);}
  function body(nf,af) {
    // A base reference is recognized from the class IIFE binding, never a map grant.
    function project(view,fn,base,blue) {
      const replacements=[];
      function baseNode(node) {
        return ts.isIdentifier(node)&&node.text===base||ts.isPropertyAccessExpression(node)&&node.name.text==='prototype'&&ts.isIdentifier(node.expression)&&node.expression.text===base;
      }
      function visit(node) {
        if(blue&&ts.isCallExpression(node)&&node.expression.getText(view.source)==='Reflect.get') {
          assert.equal(node.arguments.length,3);const [target,key,receiver]=node.arguments;
          assert(baseNode(target)&&receiver.kind===ts.SyntaxKind.ThisKeyword,'retained super receiver');
          const children=node.getChildren(view.source),open=children.find(n=>n.kind===ts.SyntaxKind.OpenParenToken),close=children.find(n=>n.kind===ts.SyntaxKind.CloseParenToken);
          const all=view.tokens(node),commas=all.filter(t=>t.text===','&&target.end<=t.start&&t.end<=receiver.getStart(view.source));
          assert.equal(commas.length,2);
          noSource(view,node.getStart(view.source),target.getStart(view.source),'generated Reflect.get prefix');
          noSource(view,key.end,close.getStart(view.source),'generated explicit super receiver');
          let keyTokens,computed=!ts.isStringLiteral(key);
          if(computed)keyTokens=view.tokens(key);
          else keyTokens=[{...view.token(key),text:key.text}];
          const separator={...commas[0],text:'$property'};
          const end=computed?[{...view.token(close),text:'$close'}]:[];
          if(!computed)noSource(view,close.getStart(view.source),close.end,'generated named-super close');
          replacements.push({start:node.getStart(view.source),end:node.end,tokens:[...view.tokens(target),separator,...keyTokens,...end]});return;
        }
        if(!blue&&(ts.isPropertyAccessExpression(node)||ts.isElementAccessExpression(node))&&baseNode(node.expression)) {
          const target=node.expression,key=ts.isPropertyAccessExpression(node)?node.name:node.argumentExpression;
          const all=view.tokens(node),delimiter=all.find(t=>target.end<=t.start),computed=ts.isElementAccessExpression(node);
          const end=computed?[{...all.at(-1),text:'$close'}]:[];
          const keyTokens=computed?view.tokens(key):[{...view.token(key),text:key.text}];
          replacements.push({start:node.getStart(view.source),end:node.end,tokens:[...view.tokens(target),{...delimiter,text:'$property'},...keyTokens,...end]});return;
        }
        ts.forEachChild(node,visit);
      }
      visit(fn.body);replacements.sort((x,y)=>x.start-y.start);
      const output=[];let cursor=0;for(const t of view.tokens(fn.body)) {
        while(cursor<replacements.length&&t.start>=replacements[cursor].end)cursor++;
        const item=replacements[cursor];if(item&&item.start<=t.start&&t.end<=item.end){if(t.start===item.start)output.push(...item.tokens);continue;}output.push(t);
      }
      return output;
    }
    pair(project(native,nf,n.base,false),project(actual,af,a.base,true),'complete class method body',aliases);
  }
  assert.equal(a.methods.length,n.methods.length,'complete class method inventory');
  for(let i=0;i<n.methods.length;i++) {
    const nm=n.methods[i],am=a.methods[i];
    if(!am.descriptor){
      assert(!nm.descriptor,'same Native method assignment mode');
      pair(native.tokens(nm.target),actual.tokens(am.target),'complete original method assignment reference',aliases);
      token(native.token(nm.operation),actual.token(am.operation),'original method assignment operation');
      if(!callables.paired(nm.fn,am.fn)){pair(native.tokens(nm.fn).filter(token=>token.end<=nm.fn.body.getStart(native.source)),actual.tokens(am.fn).filter(token=>token.end<=am.fn.body.getStart(actual.source)),'original method assignment signature');body(nm.fn,am.fn);}
      token(native.tokens(nm.statement).at(-1),actual.tokens(am.statement).at(-1),'original method assignment completion');
      continue;
    }
    assert(am.descriptor,'reviewed class descriptor ABI');
    const source=original.members.filter(member=>ts.isMethodDeclaration(member)||ts.isGetAccessorDeclaration(member)||ts.isSetAccessorDeclaration(member))[i];assert(source);
    const expectedName=source.name.getText(source.getSourceFile());
    assert(ts.isStringLiteral(am.key)&&am.key.text===expectedName,'original method property name');
    assert.equal(ts.isStringLiteral(nm.key)?nm.key.text:nm.key.getText(native.source),expectedName);
    const ownerTokens=native.tokens(nm.descriptor?nm.target:nm.target.expression);
    pair(ownerTokens,actual.tokens(am.target),'method target receiver',aliases);
    token({...native.token(nm.key),text:expectedName},{...actual.token(am.key),text:expectedName},'method property',true);
    token(native.token(nm.operation),{...actual.token(am.operation),text:nm.descriptor?':':'='},'method definition operation');
    const nh=native.tokens(nm.fn).filter(t=>t.end<=nm.fn.body.getStart(native.source)),ah=actual.tokens(am.fn).filter(t=>t.end<=am.fn.body.getStart(actual.source));
    const delegated=callables.paired(nm.fn,am.fn);
    if(!delegated){pair(nh,ah,'method signature');body(nm.fn,am.fn);}
    const ns=native.tokens(nm.statement).at(-1),as=actual.tokens(am.statement).at(-1);token(ns,as,'method completion');
    const represented=new Set(actual.tokens(am.target).concat(actual.tokens(am.key),actual.tokens(am.fn),[actual.token(am.operation),as]).map(t=>t.start));
    const used=new Set(native.tokens(nm.descriptor?nm.target:nm.target.expression).concat([native.token(nm.key),native.token(nm.operation)],native.tokens(nm.fn),[ns]).map(t=>t.start));
    if(nm.descriptor) {
      // Keep a delegated method as its exact typed hole, so transport gaps
      // end at its boundaries rather than rechecking its reordered states.
      const transport=(view,member,used)=>view.tokens(member.statement).filter(t=>!used.has(t.start)||delegated&&member.fn.getStart(view.source)<=t.start&&t.end<=member.fn.end);
      pair(transport(native,nm,used),transport(actual,am,represented),'complete paired class accessor descriptor transport',aliases);
    }else {
      for(const t of actual.tokens(am.statement))if(!represented.has(t.start))noSource(actual,t.start,t.end,'generated class descriptor transport');
      for(const t of native.tokens(nm.statement))if(!used.has(t.start)) {
        assert.equal(t.text,'.');uniform(native,t.start,t.end,native.owner(nm.target.getStart(native.source)),'Native method property transport');
      }
    }
  }
  function compareSpreadValues(nn,an) {
    function read(view,node,blue) {
      const root=unwrap(ts,node);let leaf=root;
      const helperName=node=>ts.isIdentifier(node)?node.text:ts.isPropertyAccessExpression(node)?node.name.text:null;
      if(ts.isCallExpression(root)) {
        const kind=helperName(root.expression);
        if(blue&&/^__blueice_target_spread_array_\d+$/.test(kind)) {
          assert(options.classRoles?.helpers?.get(kind)==='__blueice_target_spread_array','reviewed complete array helper');
          assert.equal(root.arguments.length,4);assert.equal(root.arguments[2].kind,ts.SyntaxKind.TrueKeyword);assert.equal(root.arguments[3].kind,options.downlevelIteration?ts.SyntaxKind.TrueKeyword:ts.SyntaxKind.FalseKeyword);
          assert(ts.isArrayLiteralExpression(root.arguments[0])&&root.arguments[0].elements.length===0);leaf=unwrap(ts,root.arguments[1]);
        }else if(kind==='__spreadArray') {
          assert.equal(root.arguments.length,3);assert(ts.isArrayLiteralExpression(root.arguments[0])&&root.arguments[0].elements.length===0);assert.equal(root.arguments[2].kind,options.downlevelIteration?ts.SyntaxKind.FalseKeyword:ts.SyntaxKind.TrueKeyword);leaf=unwrap(ts,root.arguments[1]);
        }else assert(blue&&kind==='__read','reviewed direct iterator-copy helper');
        if(options.downlevelIteration&&(!blue||kind==='__read')) {
          const iterator=kind==='__read'?root:leaf;assert(ts.isCallExpression(iterator)&&helperName(iterator.expression)==='__read'&&iterator.arguments.length===1);leaf=unwrap(ts,iterator.arguments[0]);
        }
      }
      assert(ts.isIdentifier(leaf)||ts.isCallExpression(leaf),'complete source spread operand');return {root:node,leaf};
    }
    const n=read(native,nn,false),a=read(actual,an,true);pair(native.tokens(n.leaf),actual.tokens(a.leaf),'complete constructor spread operand',aliases);
    const entry=native.owner(nn.getStart(native.source)),end=native.owner(n.leaf.end);
    uniform(native,nn.getStart(native.source),n.leaf.getStart(native.source),entry,'Native constructor spread prefix');uniform(actual,an.getStart(actual.source),a.leaf.getStart(actual.source),entry,'constructor spread prefix');
    uniform(native,n.leaf.end,nn.end,end,'Native constructor spread ending');uniform(actual,a.leaf.end,an.end,end,'constructor spread ending');
  }
  const signature=(view,fn)=>view.tokens(fn).filter(t=>t.end<=fn.body.getStart(view.source));
  pair(signature(native,n.constructor),signature(actual,a.constructor),'constructor signature',aliases);
  if(!original.heritageClauses?.length)pair(native.tokens(n.constructor.body),actual.tokens(a.constructor.body),'complete base constructor body');
  else {
    const sourceConstructor=original.members.find(ts.isConstructorDeclaration);
    if(!sourceConstructor) {
      assert.equal(n.constructor.body.statements.length,1);assert.equal(a.constructor.body.statements.length,1);
      const nr=n.constructor.body.statements[0],ar=a.constructor.body.statements[0];assert(ts.isReturnStatement(nr)&&ts.isReturnStatement(ar));
      const ac=unwrap(ts,ar.expression);assert(ts.isCallExpression(ac)&&ac.arguments.length===3);
      assert.equal(ac.arguments[0].getText(actual.source),a.base);assert.equal(ac.arguments[1].kind,ts.SyntaxKind.ThisKeyword);assert.equal(ac.arguments[2].getText(actual.source),'arguments');
      const nt=native.tokens(nr).map(t=>t.text===n.base?'$base':t.text);
      assert.deepEqual(nt,['return','$base','!==','null','&&','$base','.','apply','(','this',',','arguments',')','||','this',';']);
      noSource(native,nr.getStart(native.source),nr.end,'Native implicit constructor transport');noSource(actual,ar.getStart(actual.source),ar.end,'implicit constructor transport');
      const nb=native.tokens(n.constructor.body),ab=actual.tokens(a.constructor.body);token(nb[0],ab[0],'implicit constructor opening');token(nb.at(-1),ab.at(-1),'implicit constructor completion');
    }else {
      assert.equal(sourceConstructor.body.statements.length,1,'complete explicit constructor source statements');
      const sourceStatement=sourceConstructor.body.statements[0];assert(ts.isExpressionStatement(sourceStatement)&&ts.isCallExpression(sourceStatement.expression)&&sourceStatement.expression.expression.kind===ts.SyntaxKind.SuperKeyword);
      let nStatements=[...n.constructor.body.statements],aStatements=[...a.constructor.body.statements];
      const rest=sourceConstructor.parameters.find(p=>p.dotDotDotToken);
      if(rest) {
        assert.equal(sourceConstructor.parameters.length,1,'complete constructor single-rest signature');
        const prologue=require('./ge5_decorator_rest.cjs').comparePrologue(ts,native,nStatements,actual,aStatements.slice(1),rest,original.getSourceFile());
        restProof=prologue;
        for(const [nn,an]of [[prologue.native.array,prologue.actual.array],[prologue.native.loop,prologue.actual.loop]]) {
          (native.roleStarts??=new Map()).set(nn.getStart(native.source),native.owner(nn.getStart(native.source)));(actual.roleStarts??=new Map()).set(an.getStart(actual.source),native.owner(nn.getStart(native.source)));
          (native.roleEnds??=new Map()).set(nn.end,native.owner(nn.end));(actual.roleEnds??=new Map()).set(an.end,native.owner(nn.end));
        }
        nStatements=nStatements.slice(2);aStatements=[aStatements[0],...aStatements.slice(3)];
      }
      assert.equal(nStatements.length,1,'Native complete constructor statements');
      assert.equal(aStatements.length,3,'complete saved-receiver constructor statements');
      const nr=nStatements[0],[slot,assignment,returned]=aStatements;
      assert(ts.isReturnStatement(nr)&&ts.isBinaryExpression(nr.expression)&&nr.expression.operatorToken.kind===ts.SyntaxKind.BarBarToken&&nr.expression.right.kind===ts.SyntaxKind.ThisKeyword);
      const nc=unwrap(ts,nr.expression.left);assert(ts.isCallExpression(nc)&&ts.isPropertyAccessExpression(nc.expression)&&ts.isIdentifier(nc.expression.expression)&&nc.expression.expression.text===n.base&&['call','apply'].includes(nc.expression.name.text));
      assert.equal(nc.arguments[0].kind,ts.SyntaxKind.ThisKeyword);
      assert(ts.isVariableStatement(slot)&&slot.declarationList.declarations.length===1);const declaration=slot.declarationList.declarations[0];assert(ts.isIdentifier(declaration.name)&&!declaration.initializer);
      const receiver=declaration.name.text;assert(!original.getSourceFile().text.includes(receiver),'fresh saved class receiver');
      assert(ts.isExpressionStatement(assignment)&&ts.isBinaryExpression(assignment.expression)&&assignment.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&assignment.expression.left.getText(actual.source)===receiver);
      assert(ts.isReturnStatement(returned)&&returned.expression.getText(actual.source)===receiver,'complete class receiver completion');
      const ac=unwrap(ts,assignment.expression.right);assert(ts.isCallExpression(ac)&&ac.arguments.length===3&&ac.arguments[0].getText(actual.source)===a.base&&ac.arguments[1].kind===ts.SyntaxKind.ThisKeyword);
      const callee=unwrap(ts,ac.expression),baseToken=native.token(nc.expression.expression),invokeOwner=native.owner(nc.expression.name.getStart(native.source));
      if(ts.isIdentifier(callee)) {assert(/^__blueice_target_call_base_\d+$/.test(callee.text),'reviewed class construction helper');token(baseToken,actual.token(callee),'construction helper logical callee',true);}
      else {
        assert(ts.isFunctionExpression(callee));
        const expected=ts.createSourceFile('construct-abi.js','(function(base,receiver,values){var result=Function.prototype.apply.call(base,receiver,values);return result!==null&&(typeof result===\'object\'||typeof result===\'function\')?result:receiver;})',ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
        const expectedWords=[];function words(node){if(ts.isToken(node)){if(node.kind!==ts.SyntaxKind.EndOfFileToken)expectedWords.push(ts.isStringLiteral(node)?JSON.stringify(node.text):node.getText(expected));return;}for(const child of node.getChildren(expected))words(child);}words(expected.statements[0].expression.expression);
        const observed=actual.tokens(callee).map(t=>{const scanner=ts.createScanner(ts.ScriptTarget.Latest,true,ts.LanguageVariant.Standard,t.text),kind=scanner.scan();return kind===ts.SyntaxKind.StringLiteral?JSON.stringify(scanner.getTokenValue()):t.text;});assert.deepEqual(observed,expectedWords,'complete inline construction helper ABI');
        noSource(actual,ac.expression.getStart(actual.source),ac.expression.end,'generated inline construction helper');
      }
      token(baseToken,actual.token(ac.arguments[0]),'retained constructor base',true);
      const values=unwrap(ts,ac.arguments[2]);
      if(nc.expression.name.text==='apply') {
        assert.equal(nc.arguments.length,2);
        if(ts.isArrayLiteralExpression(values))pair(native.tokens(nc.arguments[1]),actual.tokens(values),'complete spread constructor values');
        else compareSpreadValues(nc.arguments[1],ac.arguments[2]);
      }
      else {
        assert(ts.isArrayLiteralExpression(values),'complete direct constructor argument array');
        pair(nc.arguments.slice(1).flatMap(node=>native.tokens(node)),values.elements.flatMap(node=>actual.tokens(node)),'complete constructor argument values');
        const tokens=actual.tokens(values);noSource(actual,tokens[0].start,tokens[0].end,'generated construction values opening');noSource(actual,tokens.at(-1).start,tokens.at(-1).end,'generated construction values closing');
      }
      noSource(actual,slot.getStart(actual.source),slot.end,'generated saved class receiver');
      noSource(actual,assignment.getStart(actual.source),ac.expression.getStart(actual.source),'generated receiver capture');
      noSource(actual,returned.getStart(actual.source),returned.end,'generated receiver return');
      uniform(actual,ac.expression.end,ac.arguments[0].getStart(actual.source),invokeOwner,'constructor invocation opening');
      uniform(actual,ac.arguments[0].end,values.getStart(actual.source),invokeOwner,'constructor explicit receiver transport');
      const nclose=native.tokens(nc).at(-1),aclose=actual.tokens(ac).at(-1);token(nclose,aclose,'constructor invocation closing');
      const ns=native.tokens(nr).at(-1),as=actual.tokens(assignment).at(-1);token(ns,as,'constructor statement completion');
      const nvalues=nc.arguments.slice(1).map(node=>[node.getStart(native.source),node.end]);
      const nt=native.tokens(nr);
      for(const t of nt)if(!native.covered(t.start)) {
        const owner=t.end<=nc.expression.expression.getStart(native.source)?baseToken.owners[0]:t.start>=nc.end?native.owner(nc.end):invokeOwner;
        uniform(native,t.start,t.end,owner,'complete Native constructor fallback ABI');
      }
      const nb=native.tokens(n.constructor.body),ab=actual.tokens(a.constructor.body);token(nb[0],ab[0],'constructor body opening');token(nb.at(-1),ab.at(-1),'constructor body completion');
    }
    const nc=n.inheritance.expression,ac=a.inheritance.expression;
    assert(ts.isIdentifier(ac.expression)&&(/^__blueice_target_inherit_class_\d+$/.test(ac.expression.text)||ac.expression.text==='__extends'),'reviewed class inheritance helper reference');
    const owner=native.owner(n.inheritance.getStart(native.source));
    uniform(native,n.inheritance.getStart(native.source),n.inheritance.end,owner,'Native inheritance role');uniform(actual,a.inheritance.getStart(actual.source),a.inheritance.end,owner,'inheritance role');
    (native.roleEnds??=new Map()).set(n.inheritance.end,native.owner(n.inheritance.end));(actual.roleEnds??=new Map()).set(a.inheritance.end,native.owner(n.inheritance.end));(actual.nativeEnds??=new Map()).set(a.inheritance.end,n.inheritance.end);
  }
  pair(native.tokens(n.returned),actual.tokens(a.returned),'class return',aliases);
  const excluded=(item,t)=>item.excludedNodes.some(node=>node.getStart(item.view.source)<=t.start&&t.end<=node.end)||item.constructor.getStart(item.view.source)<=t.start&&t.end<=item.constructor.end||item.inheritance&&item.inheritance.getStart(item.view.source)<=t.start&&t.end<=item.inheritance.end||item.methods.some(m=>m.statement.getStart(item.view.source)<=t.start&&t.end<=m.statement.end)||item.returned.getStart(item.view.source)<=t.start&&t.end<=item.returned.end;
  const classOwner=native.owner(n.factory.getStart(native.source)),closing=native.tokens(n.factory.body).at(-1),closeOwner=closing.owners[0];
  const invocation=native.tokens(n.call).find(token=>token.start>=n.factory.end);assert(invocation,'complete class invocation delimiter');const invocationOwner=native.owner(invocation.start);
  for(const item of [n,a])for(const t of item.view.tokens(item.node).filter(t=>!excluded(item,t))) {
    const functionClose=t.start===item.factory.body.end-1;
    if(item.call.arguments.some(argument=>argument.getStart(item.view.source)<=t.start&&t.end<=argument.end))continue;
    const afterArgument=item.call.arguments.length&&t.start>=item.call.arguments.at(-1).end;
    const owner=functionClose?closeOwner:afterArgument?native.owner(n.call.arguments.at(-1).end):t.start>=item.factory.end?invocationOwner:classOwner;
    uniform(item.view,t.start,t.end,owner,'complete class wrapper role');(item.view.roleEnds??=new Map()).set(t.end,owner);
  }
  (native.roleEnds??=new Map()).set(n.node.end,native.owner(n.node.end));(actual.roleEnds??=new Map()).set(a.node.end,native.owner(n.node.end));(actual.nativeEnds??=new Map()).set(a.node.end,n.node.end);
  if(n.call.arguments.length) {
    const relocated=options.classRoles?.actualHeritageExpression;
    if(relocated) {
      const forwarding=a.call.arguments[0];assert(ts.isIdentifier(forwarding));
      pair(native.tokens(n.call.arguments[0]),actual.tokens(relocated),'complete relocated heritage expression',aliases);
      uniform(actual,forwarding.getStart(actual.source),forwarding.end,native.owner(n.call.arguments[0].getStart(native.source)),'class heritage forwarding binding');(actual.roleEnds??=new Map()).set(forwarding.end,native.owner(n.call.arguments[0].end));
    }else pair(native.tokens(n.call.arguments[0]),actual.tokens(a.call.arguments[0]),'complete retained heritage expression',aliases);
  }
  constructionEndpoint();
  return {native:n,actual:a,generated,declarationChain,rest:restProof,delegatedExpressions:holes,delegatedCallables:callables};
}
module.exports={readClass,compareClass,unwrap};
