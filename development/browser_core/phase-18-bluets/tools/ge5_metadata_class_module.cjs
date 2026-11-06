// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The complete consumer module proves original dependency, decorator, class and publication phases.
const assert=require('node:assert/strict'),moduleRoles=require('./ge5_class_module.cjs'),calls=require('./ge5_decorator_roles.cjs'),graphRoles=require('./ge5_metadata_class_graph.cjs'),sourceRoles=require('./ge5_metadata_class_source.cjs'),functions=require('./ge5_class_expression_functions.cjs'),{compareGap}=require('./ge5_role_stream.cjs');
function compare(ts,native,actual,options,files,entry) {
  const graph=graphRoles.read(ts,files,entry,options),erasedImportComments=graph.runtime?[]:[require('./ge5_erased_import_comments.cjs').create(ts,native,actual,graph)];let phases;
  require('./ge5_metadata_class_artifacts.cjs').compare(ts,native,actual,options,files[entry],erasedImportComments);
  function units(contract,nn,an,label) {
    const wanted=phases.native.units(nn),observed=phases.actual.units(an);assert.equal(observed.length,wanted.length,label+' complete token inventory');
    for(let index=0;index<wanted.length;index++) {
      const n=wanted[index],a=observed[index];assert.equal(a.text,n.text,label+' ordered token');assert.equal(!!a.reference,!!n.reference);
      if(n.reference)assert.deepEqual(a.owner,n.owner,label+' complete generated-reference UTF-16 interior');else assert.deepEqual(a.owners,n.owners,label+' complete token UTF-16 interior');
      for(const[view,token]of[[native,n],[actual,a]]){view.cover(token.start,token.end);view.roleStarts.set(token.start,native.owner(n.start));view.roleEnds.set(token.end,native.owner(n.end));}actual.nativeStarts.set(a.start,n.start);actual.nativeEnds.set(a.end,n.end);
      if(index)compareGap(native,wanted[index-1].end,n.start,actual,observed[index-1].end,a.start,label+' ordered gap');
    }
  }
  const proof=moduleRoles.compare(ts,native,actual,options,files[entry],{erasedImportComments,dependencies:graph.runtime?[graph.dependency]:[],prepare({n,a,contract,original}) {
    phases=calls.compare(ts,native,actual,Object.values(files));
    for(const pair of phases.statementPairs)units(contract,pair.native,pair.actual,'complete original member/class decorator statement phase');
    const dependency=graphRoles.compare(ts,native,actual,n,a,contract,graph,options,phases),classNames=new Set(original.statements.filter(ts.isClassDeclaration).map(node=>node.name.text));
    for(const pair of phases.pairs) {
      if(pair.kind!=='decorate')continue;const nn=pair.native.parent,an=pair.actual.parent;if(!ts.isBinaryExpression(nn)||!ts.isBinaryExpression(an))continue;
      for(const node of[nn,an])assert(node.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(node.left)&&classNames.has(node.left.text),'original class decorator replacement binding');
      units(contract,nn,an,'complete source class decorator replacement');const wrapper=nn.parent;
      if(ts.isBinaryExpression(wrapper)&&wrapper.right===nn) {
        assert(wrapper.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isPropertyAccessExpression(wrapper.left)&&wrapper.left.expression.getText(native.source)==='exports'&&wrapper.left.name.text===nn.left.text);
        assert(ts.isExpressionStatement(wrapper.parent)&&ts.isExpressionStatement(an.parent));const ns=wrapper.parent,as=an.parent,position=a.sourceNodes.indexOf(as),publication=a.sourceNodes[position+1];
        assert(publication&&ts.isExpressionStatement(publication)&&ts.isBinaryExpression(publication.expression));const update=publication.expression;
        assert(update.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isPropertyAccessExpression(update.left)&&update.left.expression.getText(actual.source)==='exports'&&update.left.name.text===an.left.text&&ts.isIdentifier(update.right)&&update.right.text===an.left.text,'immediate publication of the exact decorated constructor');
        native.noSource(ns.getStart(native.source),nn.getStart(native.source),'generated Native combined class publication prefix');actual.noSource(publication.getStart(actual.source),publication.end,'generated final decorated class publication');
        const nt=native.tokens(ns).at(-1),at=actual.tokens(as).at(-1);assert.equal(nt.text,';');assert.equal(at.text,';');contract.pair([nt],[at],'complete class replacement statement completion');
        assert.deepEqual(actual.owner(as.end),native.owner(ns.end),'complete combined publication/decorator replacement logical endpoint');
      }
    }
    const callables=[];
    for(const source of original.statements.filter(ts.isFunctionDeclaration)) {
      const nf=n.sourceNodes.filter(node=>ts.isFunctionDeclaration(node)&&node.name?.text===source.name.text),af=a.sourceNodes.filter(node=>ts.isFunctionDeclaration(node)&&node.name?.text===source.name.text);assert.equal(nf.length,1);assert.equal(af.length,1);
      callables.push(functions.compare(ts,native,nf[0],actual,af[0],source,options));
    }
    const collector=original.statements.filter(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isFunctionExpression(node.expression.right));assert.equal(collector.length,1);
    const select=(item,source)=>item.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&node.expression.left.getText(item.view.source)===source.expression.left.getText(original));
    const nf=select(n,collector[0]),af=select(a,collector[0]);assert.equal(nf.length,1);assert.equal(af.length,1);const collection=sourceRoles.compare(ts,native,nf[0],actual,af[0],collector[0],options,contract);
    const exported=original.statements.filter(node=>ts.isVariableStatement(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));assert.equal(exported.length,1);assert.equal(exported[0].declarationList.declarations.length,1);const name=exported[0].declarationList.declarations[0].name.text;
    function value(item){const nodes=item.sourceNodes.filter(node=>options.module==='CommonJS'?ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isPropertyAccessExpression(node.expression.left)&&node.expression.left.expression.getText(item.view.source)==='exports'&&node.expression.left.name.text===name&&ts.isArrayLiteralExpression(node.expression.right):ts.isVariableStatement(node)&&node.declarationList.declarations[0].name.text===name);assert.equal(nodes.length,1);return nodes[0];}
    const published=sourceRoles.assertion(ts,native,value(n),actual,value(a),exported[0],contract);
    return {graph,dependency,phases,callables,collection,published};
  },classNode(item,name) {
    const nodes=item.sourceNodes.filter(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&node.declarationList.declarations[0].name.text===name&&node.declarationList.declarations[0].initializer);assert.equal(nodes.length,1,'one complete source class construction');return nodes[0];
  },classRoles(source,nn,an) {
    return {nativeExcluded:phases.statementPairs.map(pair=>pair.native).filter(node=>nn.pos<=node.pos&&node.end<=nn.end),actualExcluded:phases.statementPairs.map(pair=>pair.actual).filter(node=>an.pos<=node.pos&&node.end<=an.end)};
  }});
  for(const item of proof.classes)assert.deepEqual(actual.owner(item.actual.node.end),native.owner(item.native.node.end),'complete original class construction logical endpoint');
  for(const pair of phases.statementPairs)assert.deepEqual(actual.owner(pair.actual.end),native.owner(pair.native.end),'complete original decorator statement logical endpoint');
  return proof;
}
module.exports={compare};
