// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The complete provider module retains class, enum, publication and original side-effect roles.
const assert=require('node:assert/strict'),moduleRoles=require('./ge5_class_module.cjs');
function compare(ts,native,actual,options,originalText) {
  require('./ge5_metadata_class_artifacts.cjs').compare(ts,native,actual,options,originalText);
  const proof=moduleRoles.compare(ts,native,actual,options,originalText,{prepare({native,actual,n,a,contract,original}) {
    const exports=original.statements.filter(node=>ts.isExportDeclaration(node)||node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));
    const localExports=exports.filter(node=>ts.isExportDeclaration(node)&&!node.moduleSpecifier);const localAliases=localExports.flatMap(declaration=>{assert(!declaration.isTypeOnly&&ts.isNamedExports(declaration.exportClause),'exact original local export clause');return declaration.exportClause.elements.map(specifier=>{assert(!specifier.isTypeOnly&&ts.isIdentifier(specifier.propertyName??specifier.name));const local=(specifier.propertyName??specifier.name).text,targets=original.statements.filter(node=>ts.isClassDeclaration(node)&&node.name?.text===local);assert.equal(targets.length,1,'local export retains its exact original class binding');return {declaration,specifier,local,name:specifier.name.text,target:targets[0]};});});
    const runtime=exports.filter(node=>ts.isClassDeclaration(node)||ts.isEnumDeclaration(node));
    assert(exports.every(node=>runtime.includes(node)||localExports.includes(node)||ts.isInterfaceDeclaration(node)||ts.isTypeAliasDeclaration(node)),'complete provider runtime/erased export inventory');
    const classes=runtime.filter(ts.isClassDeclaration),enums=runtime.filter(ts.isEnumDeclaration);
    assert(classes.every(node=>node.name&&node.members.every(ts.isPropertyDeclaration)),'provider constructor identity comes from original class declarations');
    assert(enums.every(node=>node.members.every(member=>ts.isIdentifier(member.name)&&(!member.initializer||ts.isStringLiteral(member.initializer)))),'closed numeric/string enum source grammar');
    if(options.module==='CommonJS')for(const [item,blue]of [[n,false],[a,true]]) {
      const found=[];
      for(const node of item.sourceNodes) {
        if(!ts.isExpressionStatement(node))continue;
        let expression=node.expression;const names=[];
        while(ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&(ts.isPropertyAccessExpression(expression.left)||ts.isElementAccessExpression(expression.left))&&ts.isIdentifier(expression.left.expression)&&expression.left.expression.text==='exports') {
          const key=ts.isPropertyAccessExpression(expression.left)?expression.left.name:expression.left.argumentExpression;assert(ts.isIdentifier(key)||ts.isStringLiteral(key));names.push(key.text);expression=expression.right;
        }
        if(!ts.isVoidExpression(expression))continue;
        assert(ts.isNumericLiteral(expression.expression)&&expression.expression.text==='0');
        const expected=exports.flatMap(node=>runtime.includes(node)?blue||!node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.DefaultKeyword)?[node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.DefaultKeyword)?'default':node.name.text]:[]:localAliases.filter(alias=>alias.declaration===node).map(alias=>alias.name)).reverse();
        assert.deepEqual(names,expected,'complete AST-derived CommonJS provider export predeclarations');
        found.push(node);item.view.noSource(node.getStart(item.view.source),node.end,'generated complete provider export predeclarations');
      }
      assert.equal(found.length,1,'one provider predeclaration statement');
    }
    const localExportPairs=[];
    function walk(x,y,label){assert.equal(x.kind,y.kind,label+' AST kind');assert.deepEqual(actual.owner(y.end),native.owner(x.end),label+' exact logical endpoint');localExportPairs.push({label,native:x,actual:y});const nx=[],ax=[];ts.forEachChild(x,node=>{nx.push(node);});ts.forEachChild(y,node=>{ax.push(node);});assert.equal(ax.length,nx.length,label+' complete AST inventory');for(let i=0;i<nx.length;i++)walk(nx[i],ax[i],label+'/'+ts.SyntaxKind[nx[i].kind]+':'+i);}
    for(const alias of localAliases){let nn,an;if(options.module==='CommonJS'){
      function select(item){const found=item.sourceNodes.filter(node=>{if(!ts.isExpressionStatement(node)||!ts.isBinaryExpression(node.expression)||node.expression.operatorToken.kind!==ts.SyntaxKind.EqualsToken)return false;const left=node.expression.left,right=node.expression.right;if(!ts.isPropertyAccessExpression(left)&&!ts.isElementAccessExpression(left))return false;const key=ts.isPropertyAccessExpression(left)?left.name:left.argumentExpression;return ts.isIdentifier(left.expression)&&left.expression.text==='exports'&&(ts.isIdentifier(key)||ts.isStringLiteral(key))&&key.text===alias.name&&ts.isIdentifier(right)&&right.text===alias.local;});assert.equal(found.length,1,'one exact original local class export publication');return found[0];}
      nn=select(n);an=select(a);
    }else{const wanted=alias.declaration.getText(original),ns=n.sourceNodes.filter(node=>ts.isExportDeclaration(node)&&node.getText(native.source)===wanted),as=a.sourceNodes.filter(node=>ts.isExportDeclaration(node)&&node.getText(actual.source)===wanted);assert.equal(ns.length,1);assert.equal(as.length,1);nn=ns[0];an=as[0];}
      contract.pair(native.tokens(nn),actual.tokens(an),'complete original local class export clause/publication');walk(nn,an,'local class export '+alias.name);
    }
    const enumProofs=enums.map(declaration=>require('./ge5_metadata_enums.cjs').compare(ts,native,actual,n,a,require('./ge5_metadata_enums.cjs').source(ts,declaration),options));
    return {original,classes,enums,enumProofs,localAliases,localExportPairs};
  }});
  for(const item of proof.classes)assert.deepEqual(actual.owner(item.actual.node.end),native.owner(item.native.node.end),'complete provider class construction logical endpoint');
  return proof;
}
module.exports={compare};
