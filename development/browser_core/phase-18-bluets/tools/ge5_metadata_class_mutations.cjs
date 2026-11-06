// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each effective attack re-enters the complete two-module Native-derived contract.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate,encode}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs');
function run(ts,native,actual,options,files,entry,filename,proof) {
  const controls=[],source=files[filename],claimed=new Set(native.decoded.segments.filter(point=>point.length>2).map(point=>JSON.stringify(point.slice(2))));
  const first=source.split('\n')[0],column=Array.from({length:first.length+1},(_,index)=>index).find(column=>!claimed.has(JSON.stringify([0,0,column])));
  assert.notEqual(column,undefined,'bounded original coordinate outside all Native roles');const wrong=[0,0,column];
  function complete(view) {
    const n=stream(ts,native.code,native.raw);assert.deepEqual(view.decoded.metadata,n.decoded.metadata,'complete decoded map metadata');
    if(filename===entry)return require('./ge5_metadata_class_module.cjs').compare(ts,n,view,options,files,entry);
    return require('./ge5_metadata_class_outputs.cjs').compare(ts,n,view,options,source);
  }
  complete(stream(ts,actual.code,actual.raw)); // The identical full check accepts fresh unchanged artifacts first.
  function rejected(label,view,offset) {assert.throws(()=>complete(view),undefined,label);controls.push({label,offset,rejected:true});}
  function mapping(label,offset,owner) {
    assert.notDeepEqual(actual.owner(offset),owner,'effective '+label);
    rejected(label,coherent(ts,mutate(ts,actual,offset,owner)),offset);
  }
  function code(label,start,end,replacement) {
    assert.equal(replacement.length,end-start,'fixed-width '+label);assert.notEqual(actual.code.slice(start,end),replacement,'effective '+label);
    const view=stream(ts,actual.code.slice(0,start)+replacement+actual.code.slice(end),actual.raw);
    assert.equal(view.source.parseDiagnostics.length,0,'valid JavaScript '+label);
    rejected(label,coherent(ts,view),start);
  }
  const tokens=actual.tokens(actual.source),seen=new Set(),challengedTokens=new Set(),challengedGaps=new Set();
  if(proof.delegated.graph&&(proof.delegated.graph.aliases.length||proof.delegated.graph.interfaces.length||proof.delegated.graph.typeOnlyImports.length||proof.delegated.graph.enums.length)) {
    const graph=proof.delegated.graph;
    if(!graph.runtime) {
    const statement=options.module==='CommonJS'?`require(${JSON.stringify(graph.dependency)});`:`import ${JSON.stringify(graph.dependency)};`;
    const changed=stream(ts,actual.code+'\n'+statement+'\n',actual.raw);assert.equal(changed.source.parseDiagnostics.length,0,'added alias dependency is valid JavaScript');
    rejected('an erased type cannot gain an effectful runtime provider dependency',coherent(ts,changed));
    }
    if(graph.enums.length&&graph.runtime){const node=proof.delegated.dependency.actual.node;code('numeric enum metadata retains the original provider acquisition phase',node.getStart(actual.source),node.end,' '.repeat(node.end-node.getStart(actual.source)));}
    const builtins=new Set(graph.expected.flatMap(role=>[role.type,...(role.types??[])].filter(Boolean)).map(type=>type.builtin).filter(Boolean));
    for(const pair of proof.delegated.phases.pairs) {
      function object(node) {if(ts.isIdentifier(node)&&builtins.has(node.text)){const start=node.getStart(actual.source);code('an erased type metadata retains its exact builtin',start,start+1,'X');}ts.forEachChild(node,object);}
      function metadata(node) {if(ts.isCallExpression(node)&&ts.isIdentifier(node.expression)&&proof.delegated.phases.actual.bindings.get(node.expression.text)?.kind==='metadata')object(node.arguments[1]);ts.forEachChild(node,metadata);}
      metadata(pair.actual);
    }
  }
  function endpoint(label,offset) {if(seen.has(offset))return;seen.add(offset);mapping(label+' retarget exact endpoint',offset,wrong);if(actual.owner(offset))mapping(label+' erase exact endpoint',offset,null);}
  for(const pair of proof.delegated.reexportPairs??[])endpoint('complete original re-export '+pair.label,pair.actual.end);
  if(proof.delegated.reexport&&proof.actual.fn){
    const keyword=actual.tokens(proof.actual.fn).find(token=>token.text==='function'),start=keyword.start-1;assert.equal(actual.code[start],' ');code('live forwarding getter preserves its ordinary callable kind',start,keyword.end,'function*');
    const value=proof.actual.descriptor.properties[0].initializer;code('live forwarding descriptor retains enumerable true',value.getStart(actual.source),value.end,'!1  ');
  }
  for(const enumeration of proof.delegated.enumProofs??[]) {
    for(const pair of enumeration.pairs)endpoint(pair.label,pair.actual.end);
    const item=enumeration.actual,parameter=item.factory.parameters[0].name;
    const keyword=actual.tokens(item.factory).find(token=>token.text==='function'),space=actual.code.indexOf(' ',keyword.end);assert(space<parameter.getStart(actual.source));code('enum factory retains its ordinary callable execution phase',space,space+1,'*');
    code('enum factory binder belongs to its original declaration',parameter.getStart(actual.source),parameter.getStart(actual.source)+1,'Z');
    for(const member of item.writes) {
      const value=member.write.right,start=value.getStart(actual.source);
      if(member.source.reverse){code('numeric enum automatic member ordinal',start,value.end,value.text==='0'?'1':'0');const reverse=member.statement.expression.right;code('numeric enum reverse map exact original member',reverse.getStart(actual.source)+1,reverse.getStart(actual.source)+2,'Z');}
      else code('string enum exact original literal value',start+1,start+2,value.text[0]==='Z'?'Y':'Z');
    }
    const object=item.initialization.right;code('enum initial object construction phase',object.getStart(actual.source),object.end,'[]');
    if(item.publication){const base=item.publication.left.expression;code('enum publication retains its exact exports receiver',base.getStart(actual.source),base.end,'exportx');}
  }
  for(const[index,item]of proof.classes.entries())endpoint('original constructor '+index,item.actual.node.end);
  for(const[index,item]of proof.classes.entries())for(const pair of item.declarationChain)endpoint('original declaration '+index+' '+pair.label,pair.actual.end);
  if(proof.delegated.phases) {
    for(const[index,pair]of proof.delegated.phases.statementPairs.entries())endpoint('decorator statement '+index,pair.actual.end);
    for(const[index,pair]of proof.delegated.phases.pairs.entries())if(pair.kind==='decorate') {
      let node=pair.actual;while(node.parent&&!ts.isExpressionStatement(node))node=node.parent;
      if(ts.isExpressionStatement(node))endpoint('complete class/member replacement '+index,node.end);
    }
    for(const[index,pair]of proof.delegated.phases.pairs.entries())endpoint('complete helper-call '+index,pair.actual.end);
    for(const[index,definition]of proof.delegated.phases.actual.definitions.entries()) {
      const token=actual.tokens(definition).find(token=>token.text==='function');assert(token);
      mapping('generated decorator helper '+index+' cannot acquire source',token.start+1,wrong);
      const next=actual.tokens(definition).find(item=>item.start>=token.end&&item.text==='(');assert(next);
      const space=actual.code.slice(token.end,next.start).indexOf(' ');assert(space>=0,'ordinary helper has a fixed-width generator control');
      code('complete helper ordinary/generator kind '+index,token.end+space,token.end+space+1,'*');
    }
  }
  if(proof.delegated.dependency?.actual)endpoint('original dependency acquisition',proof.delegated.dependency.actual.node.end);
  for(const[index,node]of proof.actual.sourceNodes.entries()) {
    const words=actual.tokens(node);
    if(words.length&&words.every(token=>token.owners.every(owner=>owner===null)))mapping('generated module statement '+index+' cannot acquire source',words[0].start+Math.min(1,words[0].text.length-1),wrong);
    if(ts.isFunctionDeclaration(node)&&proof.helpers.has(node.name?.text)) {
      const keyword=words.find(token=>token.text==='function'),space=actual.code.indexOf(' ',keyword.end);assert(space<node.name.getStart(actual.source));
      code('generated class helper '+index+' preserves ordinary function kind',space,space+1,'*');
      const operation=words.find(token=>token.text==='apply'||token.text==='Object');assert(operation);
      code('complete generated class helper '+index+' algorithm',operation.start,operation.end,operation.text==='apply'?'applx':'Xbject');
    }
  }
  function region(node) {
    if(ts.isStatement(node)||ts.isFunctionLike(node)||ts.isClassElement(node)) {
      const items=actual.tokens(node),mapped=items.filter(token=>token.owners.some(Boolean));
      for(const token of new Set([mapped[0],mapped.at(-1)]))if(token&&!challengedTokens.has(token.start)) {
        challengedTokens.add(token.start);const at=token.owners.findIndex(Boolean),offset=token.start+Math.min(at+Number(!!token.owners[at+1]),token.end-token.start-1);
        mapping(ts.SyntaxKind[node.kind]+' token retarget complete UTF-16 interior',offset,wrong);
        mapping(ts.SyntaxKind[node.kind]+' token erase retained UTF-16 interior',offset,null);
      }
      for(let index=0;index+1<items.length;index++) {
        const token=items[index],next=items[index+1];let found=false;
        for(let offset=token.end;offset<next.start;offset++)if(actual.owner(offset)) {
          if(!challengedGaps.has(offset)){challengedGaps.add(offset);mapping(ts.SyntaxKind[node.kind]+' retarget ordered source gap',offset,wrong);}found=true;break;
        }
        if(found)break;
      }
    }
    ts.forEachChild(node,region);
  }
  region(actual.source);
  for(const[index,comment]of actual.comments().entries()) {
    for(let offset=comment.start;offset<comment.end;offset++)if(actual.owner(offset)){mapping('comment '+index+' retarget UTF-16 interior',offset,wrong);mapping('comment '+index+' erase UTF-16 interior',offset,null);}
    if(actual.owner(comment.end))endpoint('comment '+index,comment.end);
    else mapping('generated comment '+index+' cannot acquire source',comment.start+Math.min(4,comment.end-comment.start-1),wrong);
    if(!comment.text.startsWith('//# sourceMappingURL='))code('complete original/generated comment '+index,comment.start+2,comment.start+3,actual.code[comment.start+2]==='X'?'Y':'X');
  }
  const footer=actual.code.indexOf('//# sourceMappingURL=');assert(footer>=0);
  const position=actual.source.getLineAndCharacterOfPosition(footer+4),segments=actual.decoded.segments.map(point=>[...point]);
  const at=segments.findIndex(point=>point[0]>position.line||point[0]===position.line&&point[1]>=position.character),insert=at<0?segments.length:at;
  segments.splice(insert,0,[position.line,position.character,...wrong],[position.line,position.character]);
  rejected('hidden zero-width source grant',coherent(ts,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),mappings:encode(segments)}))),footer+4);
  for(const[field,value]of [['sourceRoot','../forged/'],['names',['forged']],['file','forged.js']])rejected('complete map metadata '+field,coherent(ts,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[field]:value}))));
  const changedNames=new Set();
  function visit(node) {
    if(ts.isIdentifier(node)&&actual.owner(node.getStart(actual.source))&&!changedNames.has(node.text)) {
      changedNames.add(node.text);const start=node.getStart(actual.source),dependency=proof.delegated.dependency,graph=proof.delegated.graph;
      const generatedNamespace=dependency?.actual&&options.module==='CommonJS'&&!graph.namespace&&ts.isVariableStatement(dependency.actual.node)&&dependency.actual.node.declarationList.declarations[0].name===node;
      if(generatedNamespace){const collision=graph.sourceClass.name.text;assert(collision.length<=node.text.length);code('fresh acquired provider namespace cannot collide with an original source binding',start,node.end,collision.padEnd(node.text.length,' '));}
      else code('original source binding/reference '+node.text,start,start+1,node.text[0]==='Z'?'Y':'Z');
    }
    if(ts.isCallExpression(node)&&ts.isIdentifier(node.expression)&&node.expression.text==='require'&&node.arguments.length===1&&ts.isStringLiteral(node.arguments[0])) {
      const literal=node.arguments[0],start=literal.getStart(actual.source)+1;code('exact original dependency module identity',start,start+1,literal.text[0]==='X'?'Y':'X');
    }
    if(ts.isImportDeclaration(node)) {const start=node.moduleSpecifier.getStart(actual.source)+1;code('exact retained import dependency identity',start,start+1,'X');}
    if(ts.isStringLiteral(node)&&node.text.startsWith('design:'))code('complete constructor metadata key '+node.text,node.end-2,node.end-1,'X');
    ts.forEachChild(node,visit);
  }
  visit(actual.source);
  for(const[index,token]of tokens.entries()) {
    if(/^[0-9]$/.test(token.text)&&actual.owner(token.start))code('complete original numeric value '+index,token.start,token.end,token.text==='0'?'1':'0');
    if(['+','||','&&','==='].includes(token.text)&&actual.owner(token.start))code('complete original operation '+index,token.start,token.end,token.text==='+'?'-':token.text==='||'?'&&':token.text==='&&'?'||':'!==');
  }
  return controls;
}
module.exports={run};
