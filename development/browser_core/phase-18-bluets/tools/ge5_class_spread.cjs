// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Source new/spread operands retain evaluation order across independently read ABIs.
const assert=require('node:assert/strict');
function unwrap(ts,node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function collect(ts,root,predicate){const output=[];function visit(node){if(predicate(node))output.push(node);ts.forEachChild(node,visit);}visit(root);return output;}
function name(ts,node){return ts.isIdentifier(node)?node.text:ts.isPropertyAccessExpression(node)?node.name.text:null;}
function compare(ts,native,actual,n,a,original,options,helpers,contract) {
  if(options.target!=='ES5')return [];
  const originals=collect(ts,original,node=>ts.isNewExpression(node)&&node.arguments?.some(ts.isSpreadElement));
  const sourceNames=new Set(collect(ts,original,ts.isIdentifier).map(node=>node.text));
  const nn=collect(ts,native.source,node=>{
    if(!ts.isNewExpression(node)||node.arguments?.length)return false;
    const call=unwrap(ts,node.expression);
    return ts.isCallExpression(call)&&ts.isPropertyAccessExpression(call.expression)&&call.expression.name.text==='apply'
      &&ts.isPropertyAccessExpression(call.expression.expression)&&call.expression.expression.name.text==='bind';
  });
  function inline(node) {
    const fn=unwrap(ts,node);if(!ts.isFunctionExpression(fn))return false;
    const expected=ts.createSourceFile('construct.js','(function(callee,values){var bound=Function.prototype.bind.apply(callee,[null].concat(values));return new bound();})',ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
    const words=(source,node)=>{const scanner=ts.createScanner(ts.ScriptTarget.Latest,true,ts.LanguageVariant.Standard,node.getText(source)),output=[];for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())output.push(scanner.getTokenText());return output;};
    assert.deepEqual(words(actual.source,fn),words(expected,expected.statements[0].expression.expression),'complete inline construction helper algorithm');return true;
  }
  const aa=collect(ts,actual.source,node=>ts.isCallExpression(node)&&(helpers.get(node.expression.getText(actual.source))==='__blueice_target_spread_construct'
    ||ts.isFunctionExpression(unwrap(ts,node.expression))&&unwrap(ts,node.expression).parameters.map(parameter=>parameter.name.getText(actual.source)).join(',')==='callee,values'&&inline(node.expression)));
  assert.equal(nn.length,originals.length,'complete Native construction/spread inventory');assert.equal(aa.length,originals.length,'complete construction/spread inventory');
  const temporaries=new Set(),proofs=[];
  function uniform(view,start,end,owner,label){for(let offset=start;offset<end;offset++)assert.deepEqual(view.owner(offset),owner,label+' UTF-16 '+offset);view.cover(start,end);(view.roleStarts??=new Map()).set(start,owner);(view.roleEnds??=new Map()).set(end,owner);}
  function sourceGrouping(node,wanted) {
    function layers(node){let count=0;while(ts.isParenthesizedExpression(node)){count++;node=node.expression;}return count;}
    const count=layers(wanted);while(ts.isParenthesizedExpression(node)&&layers(node)>count)node=node.expression;assert.equal(layers(node),count,'exact original callee grouping');return node;
  }
  function operand(view,node,blue) {
    const root=unwrap(ts,node),kind=name(ts,root.expression);let leaf=root;
    if(ts.isCallExpression(root)&&kind==='__spreadArray') {
      assert.equal(root.arguments.length,3);const [initial,input,pack]=root.arguments;assert(ts.isArrayLiteralExpression(initial));
      if(!blue){assert.equal(initial.elements.length,1);assert(ts.isVoidExpression(initial.elements[0])&&ts.isNumericLiteral(initial.elements[0].expression)&&initial.elements[0].expression.text==='0');}else assert.equal(initial.elements.length,0);
      assert.equal(pack.kind,options.downlevelIteration?ts.SyntaxKind.FalseKeyword:ts.SyntaxKind.TrueKeyword);leaf=unwrap(ts,input);
    }else if(blue&&ts.isCallExpression(root)&&helpers.get(root.expression.getText(view.source))==='__blueice_target_spread_array') {
      assert.equal(root.arguments.length,4);assert(ts.isArrayLiteralExpression(root.arguments[0])&&root.arguments[0].elements.length===0);
      assert.equal(root.arguments[2].kind,ts.SyntaxKind.TrueKeyword);assert.equal(root.arguments[3].kind,options.downlevelIteration?ts.SyntaxKind.TrueKeyword:ts.SyntaxKind.FalseKeyword);leaf=unwrap(ts,root.arguments[1]);
    }else assert(blue,'Native construction uses complete argument packing');
    if(options.downlevelIteration&&(!blue||kind==='__read'||kind==='__spreadArray')) {
      assert(ts.isCallExpression(leaf)&&name(ts,leaf.expression)==='__read'&&leaf.arguments.length===1,'complete iterable read');leaf=unwrap(ts,leaf.arguments[0]);
    }
    return leaf;
  }
  for(let index=0;index<originals.length;index++) {
    const source=originals[index],nc=nn[index],ac=aa[index];assert.equal(source.arguments.length,1);assert(ts.isSpreadElement(source.arguments[0]));
    assert.equal(ac.arguments.length,2,'complete construction helper signature');
    const call=unwrap(ts,nc.expression);assert.equal(call.arguments.length,2);
    const bound=call.expression.expression,base=bound.expression,transport=unwrap(ts,base);let callee=base;
    if(ts.isBinaryExpression(transport)) {
      assert(transport.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(transport.left));const temporary=transport.left.text;
      assert(!sourceNames.has(temporary)&&!temporaries.has(temporary),'one fresh computed construction receiver');temporaries.add(temporary);
      assert(ts.isIdentifier(call.arguments[0])&&call.arguments[0].text===temporary,'computed callee evaluated once and reused');callee=transport.right;
    }else assert(ts.isIdentifier(transport)&&ts.isIdentifier(call.arguments[0])&&transport.text===call.arguments[0].text,'direct constructor identity');
    const ncallee=sourceGrouping(callee,source.expression),acallee=sourceGrouping(ac.arguments[0],source.expression);
    const noperand=operand(native,call.arguments[1],false),aoperand=operand(actual,ac.arguments[1],true);
    contract.pair(native.tokens(ncallee),actual.tokens(acallee),'complete construction source callee');contract.pair(native.tokens(noperand),actual.tokens(aoperand),'complete construction source spread operand');
    const entry=native.owner(nc.getStart(native.source)),calleeStart=native.owner(ncallee.getStart(native.source)),calleeEnd=native.owner(ncallee.end),end=native.owner(noperand.end);
    uniform(native,nc.getStart(native.source),base.getStart(native.source),entry,'Native construction opening');
    uniform(native,base.getStart(native.source),ncallee.getStart(native.source),calleeStart,'Native computed receiver capture');
    if(ts.isFunctionExpression(unwrap(ts,ac.expression)))actual.noSource(ac.expression.getStart(actual.source),ac.expression.end,'complete inline construction helper');
    else uniform(actual,ac.getStart(actual.source),ac.expression.end,entry,'construction helper reference');
    uniform(actual,ac.expression.end,acallee.getStart(actual.source),entry,'construction helper opening');
    const repeated=call.arguments[0];
    uniform(native,ncallee.end,repeated.getStart(native.source),calleeEnd,'Native bind/apply invocation');
    uniform(native,repeated.getStart(native.source),repeated.end,ts.isBinaryExpression(transport)?calleeEnd:calleeStart,'Native retained constructor receiver');
    uniform(native,repeated.end,noperand.getStart(native.source),calleeEnd,'Native packed constructor arguments');uniform(actual,acallee.end,aoperand.getStart(actual.source),calleeEnd,'construction explicit callee and packed arguments');
    uniform(native,noperand.end,nc.end,end,'Native construction completion');uniform(actual,aoperand.end,ac.end,end,'construction completion');
    (native.roleEnds??=new Map()).set(nc.end,native.owner(nc.end));(actual.roleEnds??=new Map()).set(ac.end,native.owner(nc.end));
    proofs.push({original:source,native:nc,actual:ac,nativeCallee:ncallee,actualCallee:acallee,nativeOperand:noperand,actualOperand:aoperand});
  }
  const declared=new Set();
  for(const node of n.sourceNodes)if(ts.isVariableStatement(node)&&node.declarationList.declarations.some(declaration=>ts.isIdentifier(declaration.name)&&temporaries.has(declaration.name.text))) {
    for(const declaration of node.declarationList.declarations){assert(ts.isIdentifier(declaration.name)&&temporaries.has(declaration.name.text)&&!declaration.initializer&&!declared.has(declaration.name.text),'complete fresh construction temporary declaration');declared.add(declaration.name.text);}
    native.noSource(node.getStart(native.source),node.end,'complete generated construction temporary declaration');
  }
  assert.deepEqual([...declared].sort(),[...temporaries].sort(),'complete declared computed construction inventory');return proofs;
}
module.exports={compare};
