// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Isolated valid mutations challenge tag, raw/substitution spans and site ABI.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_tagged_roles.cjs');
function run(ts,native,actual,options,originals,proof) {
  const item=proof.actual,binding=proof.binding,fn=item.user[0],tokens=actual.tokens(fn),start=node=>node.getStart(actual.source),controls=[];
  const returned=tokens.find(t=>t.text==='return'),raw=tokens.find(t=>t.text==='raw'),length=tokens.find(t=>t.text==='length'),addition=tokens.find(t=>t.text==='+');assert(returned&&raw&&length&&addition);
  const lowered=options.target==='ES5';let tag,head,tail,operand;
  if(lowered){const p=proof.site.actual;tag=p.node.expression;[head,tail]=p.arrays[1].elements;operand=p.operand;}
  else {const p=binding.expression;tag=p.tag;head=p.template.head;tail=p.template.templateSpans[0].literal;operand=p.template.templateSpans[0].expression;assert.equal(p.template.templateSpans.length,1);}
  function mapping(label,offset,owner){const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,options,originals),undefined,label);controls.push({label,rejected:true});}
  function code(label,from,to,replacement){assert.equal(replacement.length,to-from);const changed=stream(ts,actual.code.slice(0,from)+replacement+actual.code.slice(to),actual.raw);assert.throws(()=>compare(ts,native,changed,options,originals),undefined,label);controls.push({label,rejected:true});}
  mapping('erase original tag function name interior',start(fn.name)+1,null);
  mapping('erase original return interior',returned.start+1,null);
  mapping('retarget raw property interior',raw.start+1,[0,0,0]);
  mapping('erase length property interior',length.start+1,null);
  mapping('erase original function endpoint',fn.end,null);
  mapping('erase called tag interior',start(tag)+1,null);
  mapping('erase raw head interior',start(head)+1,null);
  mapping('retarget raw tail interior',start(tail)+1,[0,0,0]);
  mapping('erase original substitution interior',start(operand)+1,null);
  mapping('erase published binding interior',start(binding.name)+1,null);
  mapping('retarget call closing',binding.expression.end-1,[0,0,0]);
  mapping('retarget statement terminator',binding.statement.end-1,[0,0,0]);
  mapping('erase statement endpoint',binding.statement.end,null);
  mapping('grant generated sourceMappingURL source',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  code('replace original tag function operation',addition.start,addition.end,'-');
  code('replace raw property',raw.start,raw.end,'bad');
  code('replace called tag',start(tag),tag.end,'bad');
  code('replace substitution value',start(operand),operand.end,'42');
  code('replace raw head text',start(head)+1,start(head)+2,'b');
  code('replace published binding',start(binding.name),binding.name.end,'forget');
  const strictPlain=item.format==='plain'&&item.generated.some(node=>ts.isExpressionStatement(node)&&ts.isStringLiteral(node.expression));
  if(item.format!=='plain'||strictPlain) {
    mapping('grant generated module header source',actual.source.statements[0].getStart(actual.source)+1,[0,0,0]);
    if(item.format==='wrapped') {const dep=actual.tokens(actual.source).find(token=>token.text==='"require"');assert(dep);code('replace wrapper dependency',dep.start+1,dep.end-1,'missing');}
    else if(item.format==='system') {
      const registration=actual.source.statements[0].expression.expression.name;code('replace System registration',start(registration),registration.end,'registex');
      const slot=item.generated.find(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText()===item.system.stem+'_slot').expression.arguments[0];code('replace System publication slot',start(slot)+1,slot.end-1,'forget');
    } else {const marker=actual.tokens(actual.source).find(token=>token.text==='"__esModule"');assert(marker);code('replace CommonJS marker',marker.start+1,marker.end-1,'__esModulf');}
  }
  if(lowered) {
    const p=proof.site.actual;
    mapping('erase raw head endpoint',head.end,null);
    mapping('erase raw tail endpoint',tail.end,null);
    mapping('retarget site cache interior',start(p.cache.left)+1,[0,0,0]);
    mapping('erase substitution closing',p.operand.end,null);
    code('invert site cache guard',start(p.cache.operatorToken),p.cache.operatorToken.end,'&&');
    const name=p.capture.left.getText(),wrong=name.slice(0,-1)+'9';assert(!item.slots.has(wrong));code('write the wrong cache slot',start(p.capture.left),p.capture.left.end,wrong);
    const cooked=p.arrays[0].elements[0];code('replace cooked head text',start(cooked)+1,start(cooked)+2,'b');
    const declaration=item.generated.find(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.some(d=>d.name.getText()===p.site));assert(declaration);
    mapping('grant site declaration source',start(declaration)+1,[0,0,0]);
    const constructor=[...item.helpers].find(([,kind])=>kind==='template');
    if(constructor){const helper=item.generated.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text===constructor[0]);assert(helper);mapping('grant constructor definition source',start(helper.name)+1,[0,0,0]);}
    else{mapping('retarget external constructor transport',start(p.construct.expression)+1,[0,0,0]);}
  }
  if(item.dependency.length) {
    const source=actual.tokens(actual.source).find(token=>/^['"]tslib['"]$/.test(token.text));assert(source);code('replace helper dependency',source.start+1,source.end-1,'wrong');mapping('grant helper import source',source.start+1,[0,0,0]);
  }
  assert.equal(controls.length,20+(item.format==='system'?3:item.format==='wrapped'||strictPlain?2:0)+(lowered?9:0)+(item.dependency.length?2:0));
  return controls;
}
module.exports={run};
