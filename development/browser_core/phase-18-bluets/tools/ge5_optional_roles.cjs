// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Pair original statements independently of their complete module transport.
const assert=require('node:assert/strict');
const {create}=require('./ge5_module_contract.cjs');
function compare(ts,native,actual,options,originals) {
  const contract=create(ts,native,actual,options,originals,'optional');
  const {n,a,cover,uniform,pair,boundary,binding,prefix,suffix,finish}=contract;
  assert.equal(n.user.length,3);
  for(let index=0;index<2;index++) {
    const x=n.user[index],y=a.user[index];pair(native.tokens(x),actual.tokens(y),'complete original module statement '+index);
    boundary(native,x,native.owner(x.end));boundary(actual,y,native.owner(x.end));
  }
  const nb=binding(n,2,'result',true),ab=binding(a,2,'result',true);prefix(nb,ab,'published binding prefix');
  if(options.target==='ES2022') {
    pair(native.tokens(nb.expression),actual.tokens(ab.expression),'complete retained optional expression');
  } else {
    assert(ts.isBinaryExpression(nb.expression)&&ts.isBinaryExpression(ab.expression));
    assert.equal(nb.expression.operatorToken.getText(),'+');assert.equal(ab.expression.operatorToken.getText(),'+');
    const x=nb.expression.left,y=ab.expression.left;assert(ts.isParenthesizedExpression(x)&&ts.isParenthesizedExpression(y));
    const nt=native.tokens(x),at=actual.tokens(y);
    pair([nt[0]],[at[0]],'original coalesce grouping open');
    pair([nt.at(-1)],[at.at(-1)],'original coalesce grouping close');
    require('./ge5_optional_expression.cjs').compare(ts,native,{item:n,node:x.expression},actual,{item:a,node:y.expression},cover,uniform);
    pair([native.token(nb.expression.operatorToken),...native.tokens(nb.expression.right)],[actual.token(ab.expression.operatorToken),...actual.tokens(ab.expression.right)],'original addition after coalesce');
  }
  suffix(nb,ab);finish();
  return {native:n,actual:a,binding:ab};
}
module.exports={compare};
