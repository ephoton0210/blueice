// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Typed source callable and erased-assertion grouping roles compose with module accounting.
const assert=require('node:assert/strict');
function functions(ts,root) {
  const result=[];function visit(node){if(ts.isFunctionExpression(node)||ts.isArrowFunction(node))result.push(node);ts.forEachChild(node,visit);}visit(root);return result;
}
function compare(ts,native,nn,actual,an,source,options,contract) {
  const originals=functions(ts,source),wanted=functions(ts,nn),observed=functions(ts,an),removed=new Set();
  assert.equal(wanted.length,originals.length,'complete original Native source callable inventory');assert.equal(observed.length,originals.length,'complete original actual source callable inventory');
  function uniform(token,owner,label) {
    assert(token.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),label+' complete generated delimiter UTF-16 role');actual.cover(token.start,token.end);actual.roleStarts.set(token.start,owner);actual.roleEnds.set(token.end,owner);removed.add(token.start);
  }
  for(let index=0;index<originals.length;index++) {
    const original=originals[index],n=wanted[index],a=observed[index];
    assert(!n.asteriskToken&&!a.asteriskToken&&!n.modifiers?.some(node=>node.kind===ts.SyntaxKind.AsyncKeyword)&&!a.modifiers?.some(node=>node.kind===ts.SyntaxKind.AsyncKeyword),'ordinary source callable kind');
    const arrow=ts.isArrowFunction(original),effective=original.parameters.filter(parameter=>!(ts.isIdentifier(parameter.name)&&parameter.name.text==='this'));
    if(options.target==='ES5')assert(ts.isFunctionExpression(n)&&ts.isFunctionExpression(a),'complete lowered source callable kind');else assert.equal(ts.isArrowFunction(n)&&ts.isArrowFunction(a),arrow,'complete retained source callable kind');
    for(const fn of[n,a]){assert(!fn.name);assert.deepEqual(fn.parameters.map(parameter=>parameter.name.getText(fn.getSourceFile())),effective.map(parameter=>parameter.name.getText(original.getSourceFile())),'complete original source callable parameters');}
    if(arrow&&!ts.isBlock(original.body)&&options.target==='ES5') {
      for(const fn of[n,a])assert(ts.isBlock(fn.body)&&fn.body.statements.length===1&&ts.isReturnStatement(fn.body.statements[0]),'closed concise-arrow return ABI');
      const nr=n.body.statements[0],ar=a.body.statements[0];
      if(ts.isParenthesizedExpression(ar.expression)&&!ts.isParenthesizedExpression(nr.expression)) {
        const group=ar.expression,tokens=actual.tokens(group);assert.equal(tokens[0].text,'(');assert.equal(tokens.at(-1).text,')');
        uniform(tokens[0],null,'generated concise arrow expression opening');uniform(tokens.at(-1),null,'generated concise arrow expression completion');
        assert.deepEqual(actual.owner(group.end),native.owner(nr.expression.end),'concise arrow expression endpoint');
      }
    }
  }
  contract.pair(native.tokens(nn),actual.tokens(an).filter(token=>!removed.has(token.start)),'complete original metadata collector callable tree');
  assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete metadata collector statement endpoint');
  return {source,native:nn,actual:an,callables:originals.length};
}
function assertion(ts,native,nn,actual,an,source,contract) {
  const original=[];function visit(node){if(ts.isParenthesizedExpression(node)&&ts.isAsExpression(node.expression))original.push(node);ts.forEachChild(node,visit);}visit(source);
  const actualGroups=[];function groups(node){if(ts.isParenthesizedExpression(node)&&ts.isIdentifier(node.expression)&&original.some(source=>source.expression.expression.getText(source.getSourceFile())===node.expression.text))actualGroups.push(node);ts.forEachChild(node,groups);}groups(an);
  const removed=new Set();
  for(const group of actualGroups) {
    const matching=original.filter(source=>source.expression.expression.getText(source.getSourceFile())===group.expression.text);assert.equal(matching.length,1,'one exact original parenthesized assertion');
    const refs=[];function find(node){if(ts.isIdentifier(node)&&node.text===group.expression.text)refs.push(node);ts.forEachChild(node,find);}find(nn);assert.equal(refs.length,1,'one Native surviving assertion operand');
    const tokens=actual.tokens(group),first=tokens[0],last=tokens.at(-1);assert.equal(first.text,'(');assert.equal(last.text,')');
    for(const [token,owner]of[[first,native.owner(refs[0].getStart(native.source))],[last,native.owner(refs[0].end)]]) {
      assert(token.owners.every(value=>JSON.stringify(value)===JSON.stringify(owner)),'complete transparent assertion delimiter Native expression role');actual.cover(token.start,token.end);actual.roleStarts.set(token.start,owner);actual.roleEnds.set(token.end,owner);removed.add(token.start);
    }
  }
  contract.pair(native.tokens(nn),actual.tokens(an).filter(token=>!removed.has(token.start)),'complete published source value after typed assertion erasure');
  assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete published source value endpoint');
  return {source,native:nn,actual:an,assertions:original};
}
module.exports={compare,assertion};
