// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete private-class bodies compose with the existing closed module reader.
const assert=require('node:assert/strict'),moduleRoles=require('./ge5_class_module.cjs');
const helpers=require('./ge5_private_accessor_helpers.cjs');
function compare(ts,native,actual,options,originalText) {
  const lines=originalText.split('\n');
  const originalTree=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true);assert.equal(originalTree.statements.filter(ts.isClassDeclaration).length,1,'one exact original private class source');
  for(const view of[native,actual]){
    if(Object.hasOwn(view.decoded.metadata,'sourcesContent'))assert.deepEqual(view.decoded.metadata.sourcesContent,[originalText],'exact complete original source content');
    assert.equal(view.decoded.metadata.sources.length,1);const seen=new Map();
    for(const segment of view.decoded.segments){
      if(segment.length>2)assert(segment[2]===0&&segment[3]<lines.length&&segment[4]<=lines[segment[3]].length,'every private source coordinate is within original UTF-16 content');
      const key=segment[0]+':'+segment[1];if(seen.has(key))assert.deepEqual(segment.slice(2),seen.get(key),'ordered same-position source transitions remain coherent');seen.set(key,segment.slice(2));
    }
    const publication=/(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(view.code);assert(publication,'one complete source-map publication');
    if(options.inlineSourceMap)assert.equal(Buffer.from(publication[1].split('base64,')[1],'base64').toString(),view.raw,'coherent complete inline artifact');
    else assert.equal(publication[1],/(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(native.code)[1],'complete external map publication');
  }
  const lowered=options.target!=='ES2022';let providers,context;const operationNodes=[];
  const dependencies=lowered&&options.importHelpers?['tslib']:[];
  function sourceClass(source) {
    const fields=source.members.filter(ts.isPropertyDeclaration),gets=source.members.filter(ts.isGetAccessorDeclaration),sets=source.members.filter(ts.isSetAccessorDeclaration),methods=source.members.filter(ts.isMethodDeclaration);
    assert.equal(fields.length,1);assert(ts.isPrivateIdentifier(fields[0].name)&&fields[0].initializer&&!fields[0].modifiers?.length);
    assert.equal(gets.length,1);assert.equal(sets.length,1);assert.equal(methods.length,1);assert.equal(source.members.length,4,'complete original private member inventory');
    const field=fields[0],get=gets[0],set=sets[0],has=methods[0];
    assert.equal(get.parameters.length,0);assert.equal(set.parameters.length,1);assert.equal(has.parameters.length,1);
    assert(!source.heritageClauses?.length);for(const member of[get,set,has])assert(member.parameters.every(parameter=>!parameter.initializer&&!parameter.dotDotDotToken&&!parameter.questionToken),'plain complete original accessor parameters');
    for(const member of[get,set,has])assert(member.body&&member.body.statements.length===1);
    const read=get.body.statements[0],write=set.body.statements[0],membership=has.body.statements[0];
    assert(ts.isReturnStatement(read)&&ts.isPropertyAccessExpression(read.expression)&&read.expression.expression.kind===ts.SyntaxKind.ThisKeyword&&read.expression.name.text===field.name.text);
    assert(ts.isExpressionStatement(write)&&ts.isBinaryExpression(write.expression)&&write.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken);
    assert(ts.isPropertyAccessExpression(write.expression.left)&&write.expression.left.expression.kind===ts.SyntaxKind.ThisKeyword&&write.expression.left.name.text===field.name.text);
    assert(ts.isIdentifier(set.parameters[0].name)&&ts.isIdentifier(write.expression.right)&&write.expression.right.text===set.parameters[0].name.text);
    assert(ts.isReturnStatement(membership)&&ts.isBinaryExpression(membership.expression)&&membership.expression.operatorToken.kind===ts.SyntaxKind.InKeyword&&ts.isPrivateIdentifier(membership.expression.left)&&membership.expression.left.text===field.name.text);
    assert(ts.isIdentifier(has.parameters[0].name)&&ts.isIdentifier(membership.expression.right)&&membership.expression.right.text===has.parameters[0].name.text);
    return {source,field,get,set,has};
  }
  function readClass(profile,source,ir) {
    const {view}=profile,node=profile.classNode;
    assert.equal(profile.storage,'_'+source.name.text+'_'+ir.field.name.text.slice(1),'private-state identity is derived from the original declared name');
    assert.equal(node.name.text,source.name.text);assert.equal(node.members.length,4,'complete emitted private class member inventory');
    const ctor=node.members[0],get=node.members[1],set=node.members[2],has=node.members[3];
    assert(ts.isConstructorDeclaration(ctor)&&ts.isGetAccessorDeclaration(get)&&ts.isSetAccessorDeclaration(set)&&ts.isMethodDeclaration(has));
    const initializer=ctor.body.statements[0].expression.arguments[1];assert(ts.isNumericLiteral(ir.field.initializer)&&ts.isNumericLiteral(initializer));assert.equal(initializer.text,ir.field.initializer.text,'private-field original initializer');
    const originalSource=source.getSourceFile(),owner=position=>{const lc=originalSource.getLineAndCharacterOfPosition(position);return[0,lc.line,lc.character];};
    if(!profile.blue){
      assert.deepEqual(view.owner(ctor.body.statements[0].getStart(view.source)),owner(ir.field.name.getStart(originalSource)),'Native private initializer prefix comes from this exact original field');
      assert.deepEqual(view.owner(initializer.getStart(view.source)),owner(ir.field.initializer.getStart(originalSource)),'Native private initializer preserves its exact original interval');
      assert.deepEqual(view.owner(initializer.end),owner(ir.field.initializer.end),'Native private initializer endpoint');
    }
    assert.equal(get.name.getText(view.source),ir.get.name.getText(source.getSourceFile()));assert.equal(set.name.getText(view.source),ir.set.name.getText(source.getSourceFile()));assert.equal(has.name.getText(view.source),ir.has.name.getText(source.getSourceFile()));
    assert.equal(get.parameters.length,0);assert.equal(set.parameters.length,1);assert.equal(has.parameters.length,1);
    assert.equal(set.parameters[0].name.getText(view.source),ir.set.parameters[0].name.getText(source.getSourceFile()));assert.equal(has.parameters[0].name.getText(view.source),ir.has.parameters[0].name.getText(source.getSourceFile()));
    const calls=[];
    for(const[member,kind]of[[get,'Get'],[set,'Set'],[has,'In']]){
      assert.equal(member.body.statements.length,1);const statement=member.body.statements[0];
      assert(kind==='Set'?ts.isExpressionStatement(statement):ts.isReturnStatement(statement));const call=statement.expression;assert(ts.isCallExpression(call));
      assert.equal(helpers.reference(ts,providers,profile,call.expression),kind,'operation uses its exact proved helper identity');
      const args=call.arguments;assert.equal(args.length,kind==='Set'?4:kind==='Get'?3:2);
      if(!profile.blue){
        const operation=kind==='Get'?ir.get.body.statements[0].expression:kind==='Set'?ir.set.body.statements[0].expression:ir.has.body.statements[0].expression;
        const entry=kind==='In'?ir.has.body.statements[0].getStart(originalSource):kind==='Get'?operation.expression.getStart(originalSource):operation.left.expression.getStart(originalSource);
        assert.deepEqual(view.owner(call.expression.getStart(view.source)),owner(entry),'Native private operation prefix is tied to the original expression/frame');
        const operand=kind==='Get'?operation.expression:operation.right,physical=args[kind==='Get'?0:kind==='Set'?2:1];
        assert.deepEqual(view.owner(physical.getStart(view.source)),owner(operand.getStart(originalSource)),'Native private operation operand interval');
        assert.deepEqual(view.owner(call.end),owner(operation.end),'Native complete private operation logical endpoint');
      }
      const state=args[kind==='In'?0:1];assert(ts.isIdentifier(state)&&state.text===profile.storage,'all private operations use this class exact state');
      if(kind==='In'){assert(ts.isIdentifier(args[1])&&args[1].text===has.parameters[0].name.text);}
      else{assert.equal(args[0].kind,ts.SyntaxKind.ThisKeyword);assert(ts.isStringLiteral(args.at(-1))&&args.at(-1).text==='f');if(kind==='Set')assert(ts.isIdentifier(args[2])&&args[2].text===set.parameters[0].name.text);}
      calls.push({kind,call,statement});
    }
    return {node,calls};
  }
  const proof=moduleRoles.compare(ts,native,actual,options,originalText,{
    dependencies,
    prepare({n,a,original}){if(!lowered)return;assert.equal(original.statements.filter(ts.isClassDeclaration).length,1);context={n,a,original};providers=helpers.prepare(ts,native,actual,n,a,original,options);return providers;},
    compareClass({source,nn,an,contract}){
      const ir=sourceClass(source);if(!lowered){
        for(const node of[nn,an]){
          const cls=ts.isClassDeclaration(node)||ts.isClassExpression(node)?node:node.expression.right;assert(ts.isClassExpression(cls)||ts.isClassDeclaration(cls));
          const field=cls.members[0];assert(ts.isPropertyDeclaration(field)&&ts.isPrivateIdentifier(field.name)&&field.name.text===ir.field.name.text);assert(ts.isNumericLiteral(field.initializer)&&ts.isNumericLiteral(ir.field.initializer)&&field.initializer.text===ir.field.initializer.text,'original retained private initializer');
          assert.deepEqual(cls.members.slice(1).map(member=>member.name.getText(member.getSourceFile())),[ir.get,ir.set,ir.has].map(member=>member.name.getText(source.getSourceFile())),'original retained member names');
        }
        return null;
      }
      assert.equal(source.getSourceFile(),context.original,'exact original class ancestry');
      const n=readClass(providers.native,source,ir),a=readClass(providers.actual,source,ir);
      const aliases=new Map([[providers.actual.storage,providers.native.storage]]);
      for(const item of n.calls)aliases.set('$private'+item.kind,'$private'+item.kind);
      function tokens(view,node,calls){return view.tokens(node).flatMap(token=>{const operation=calls.find(item=>item.call.expression.getStart(view.source)<=token.start&&token.end<=item.call.expression.end);if(!operation)return[token];const callee=operation.call.expression,start=callee.getStart(view.source);return token.start===start?[{text:'$private'+operation.kind,start,end:callee.end,owners:Array.from({length:callee.end-start},(_,i)=>view.owner(start+i))}]:[];});}
      contract.pair(tokens(native,nn,n.calls),tokens(actual,an,a.calls),'complete private class operations and physical transport',aliases);
      assert.deepEqual(actual.owner(a.node.end),native.owner(n.node.end),'exact original private class completion');
      assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete private class binding endpoint');
      for(let i=0;i<n.calls.length;i++)assert.deepEqual(actual.owner(a.calls[i].call.end),native.owner(n.calls[i].call.end),'complete private operation logical endpoint');
      operationNodes.push(...a.calls);
      return {source,native:{node:nn,native:true},actual:{node:an,native:true},generated:[]};
    },
  });
  for(const[actualEnd,nativeEnd]of actual.nativeEnds??[]){
    assert.deepEqual(actual.owner(actualEnd),native.owner(nativeEnd),'complete paired private/module token endpoint '+actualEnd+'/'+nativeEnd);
  }
  if(proof.native.format==='system'){
    const declarations=originalTree.statements.filter(node=>ts.isVariableStatement(node)&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)).flatMap(node=>[...node.declarationList.declarations]);
    for(const declaration of declarations){
      assert(ts.isIdentifier(declaration.name));const name=declaration.name.text;
      const ns=proof.native.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText(native.source)===proof.native.system.publisher&&node.expression.arguments.length===2&&ts.isStringLiteral(node.expression.arguments[0])&&node.expression.arguments[0].text===name);
      const as=proof.actual.sourceNodes.filter(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&node.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isPropertyAccessExpression(node.expression.left)&&node.expression.left.expression.getText(actual.source)==='exports'&&node.expression.left.name.text===name);
      assert.equal(ns.length,1);assert.equal(as.length,1);
      assert.deepEqual(actual.owner(as[0].end),native.owner(ns[0].end),'complete source-bound System variable publication statement endpoint');
    }
  }
  return {proof,providers,operations:operationNodes,lowered};
}
module.exports={compare};
