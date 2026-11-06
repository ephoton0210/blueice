// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Publication-coherent controls independently challenge roles, protocol and flow.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate,encode}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_async_arrow_guard_mutations.cjs'),execution=require('./ge5_async_generator_execution.cjs');
function run(ts,native,actual,options,original,proof,compare){
  const controls=[],start=node=>node.getStart(actual.source);
  const fresh=view=>compare(ts,stream(ts,native.code,native.raw),coherent(ts,stream(ts,view.code,view.raw)),options,original);
  fresh(actual);
  const parts=execution.fixture(ts,actual),check=view=>{fresh(actual);return fresh(view);};
  function mapping(label,offset,generated=false){assert.equal(actual.owner(offset)===null,generated,label+' precondition');assert.throws(()=>check(mutate(ts,actual,offset,generated?[0,0,0]:null)),undefined,label);controls.push({label,rejected:true,publicationCoherent:true});}
  function changed(edits){let code=actual.code;for(const[from,to,text]of edits.sort((a,b)=>b[0]-a[0])){assert(text.length<=to-from);code=code.slice(0,from)+text.padEnd(to-from,' ')+code.slice(to);}return stream(ts,code,actual.raw);}
  function code(label,node,text,reason){const candidate=changed([[start(node),node.end,text]]);assert.throws(()=>check(candidate),reason,label);controls.push({label,rejected:true,publicationCoherent:true});}
  function flip(label,call){const flag=call.arguments[0],payload=call.arguments[1],text=flag.kind===ts.SyntaxKind.TrueKeyword?'false,':'true,';assert.throws(()=>check(changed([[start(flag),start(payload),text]])),undefined,label);controls.push({label,rejected:true,publicationCoherent:true});}
  for(const kind of ['sequence','observe']){const node=proof.parts.actual[kind];mapping('erase original '+kind+' exact name ending',node.name.end,actual.owner(node.name.end)===null);}
  for(const[index,call]of parts.calls.entries())for(const[at,token]of actual.tokens(call).entries()){mapping('erase awaited call '+index+' token '+at,token.start);if(token.end-token.start>1)mapping('erase awaited call '+index+' UTF-16 interior '+at,token.start+1);}
  const effects=[];function effect(node){if(ts.isCallExpression(node)&&node.expression.getText(actual.source)==='log.push')effects.push(node);ts.forEachChild(node,effect);}effect(parts.sequence);assert.equal(effects.length,1);for(const[index,token]of actual.tokens(effects[0]).entries()){mapping('erase finally effect token '+index,token.start);if(token.end-token.start>1)mapping('erase finally effect UTF-16 interior '+index,token.start+1);}
  mapping('erase async-generator binding interior',start(parts.sequence.name)+1);mapping('grant map publication comment source',actual.code.indexOf('//# sourceMappingURL=')+4,true);code('replace first awaited operation',parts.calls[0].expression.name,'reject');code('replace first awaited operand',parts.calls[0].arguments[0],'9');code('replace finally effect operand',effects[0].arguments[0],'"changed"');code('replace finally effect operation',effects[0].expression.name,'pop');
  if(proof.sequence.retained){const fn=parts.sequence,originals=[];function yields(node){if(ts.isYieldExpression(node))originals.push(node);ts.forEachChild(node,yields);}yields(fn);assert.equal(originals.length,2);for(const[index,node]of originals.entries())mapping('erase retained public yield '+index,start(node)+1);}
  else{
    const seq=proof.sequence,cb=seq.actual.callback,driver=seq.actual.call;mapping('erase queued driver entry',start(driver));mapping('erase callback header interior',start(cb)+1);
    const helpers=proof.helpers.get(actual),queued=helpers.find(node=>node.name?.text===driver.expression.getText(actual.source));assert(queued);mapping('grant queued helper binding source',start(queued.name)+1,true);const texts=[];function helperLiterals(n){if(ts.isStringLiteral(n)&&n.text==='next')texts.push(n);ts.forEachChild(n,helperLiterals);}helperLiterals(queued);assert(texts.length);code('replace queued helper resume operation',texts[0],'"noop"');
    if(options.target==='ES5'){
      const trace=seq.core.actual,m=trace.machine,publicRequest=trace.physical.find(p=>p.kind==='yield'),firstAwait=trace.physical.find(p=>p.kind==='await');assert(publicRequest&&firstAwait);mapping('grant marker flag source',start(publicRequest.record.node.arguments[0]),true);code('turn public yield into private await',publicRequest.record.node.arguments[0],'true');flip('turn private await into public yield',firstAwait.record.node);code('skip generator entry',m.call.arguments[1],'0');
      for(const[index,event]of trace.events.entries())if(['yield','return'].includes(event.kind)){mapping('erase logical completion prefix '+index,start(event.statement)+1);if(actual.owner(event.statement.end)!==null)mapping('erase logical completion statement end '+index,event.statement.end);}
      const sent=trace.values.find(v=>v.kind==='sent'&&v.id===0);assert(sent);code('replace exact resume operation',sent.node.expression.name,'noop');
      const declaration=m.node.body.statements.flatMap(n=>ts.isVariableStatement(n)?[...n.declarationList.declarations]:[]).find(n=>!n.initializer&&ts.isIdentifier(n.name));assert(declaration);code('generated slot cannot alias JSON after template',declaration.name,'JSON',/fresh generated slot or original user binding/);
      const resumeStatement=trace.definitions.find(d=>d.value===sent)?.statement;assert(resumeStatement);const clause=m.dispatcher.caseBlock.clauses.find(c=>c.statements.includes(resumeStatement));assert(clause);code('erase mandatory resume state',clause,'');
      // All states and values remain present while only the concat getter moves
      // across await. The evaluation-phase proof must reject this exact change.
      const getter=trace.evaluations.find(e=>e.value.kind==='property'&&e.value.name==='concat'&&e.phase==='body').value,propertyStatement=trace.definitions.find(d=>d.value===getter).statement,literalStatement=trace.definitions.find(d=>d.value===getter.object).statement;
      function edge(statement){const state=[...m.states.values()].find(nodes=>nodes.includes(statement));assert(state);const node=state.find(n=>ts.isExpressionStatement(n)&&ts.isBinaryExpression(n.expression)&&n.expression.left.getText(actual.source)===m.context+'.label');assert(node&&ts.isNumericLiteral(node.expression.right));return node.expression.right;}
      const propertyEdge=edge(propertyStatement),literalEdge=edge(literalStatement),resumeEdge=edge(resumeStatement),propertyState=[...m.states].find(([,nodes])=>nodes.includes(propertyStatement))[0];
      assert.throws(()=>check(changed([[start(literalEdge),literalEdge.end,propertyEdge.getText(actual.source)],[start(propertyEdge),propertyEdge.end,resumeEdge.getText(actual.source)],[start(resumeEdge),resumeEdge.end,String(propertyState)]])),/same ordered getter\/call evaluation|getter\/call stays before/,'delay saved concat getter until after await');controls.push({label:'delay saved concat getter until after await',rejected:true,publicationCoherent:true,reason:'evaluation order across suspension'});
      controls.push(...require('./ge5_async_generator_capability_mutations.cjs').run(ts,native,actual,proof));
    }else{for(const[index,transport]of seq.body.transports.entries()){mapping('erase marker operation '+index,start(transport.actual.expression)+1);flip('invert exact marker awaiting bit '+index,transport.actual.expression.expression);}}
  }
  // A conflicting entry hidden at the exact same generated position cannot
  // acquire source merely because the final visible token owner still agrees.
  const segments=actual.decoded.segments.map(s=>[...s]),index=segments.findIndex(s=>s.length>2),existing=segments[index];assert(existing);const inserted=[existing[0],existing[1],0,0,0];if(JSON.stringify(existing.slice(2))===JSON.stringify(inserted.slice(2)))inserted[4]=1;segments.splice(index,0,inserted);assert.throws(()=>check(stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),mappings:encode(segments)}))),/conflicting same-position source transition/,'hidden in-bounds source grant');controls.push({label:'hidden in-bounds source grant',rejected:true,publicationCoherent:true});
  controls.push(...require('./ge5_async_arrow_publication_mutations.cjs').run(ts,native,actual,options,original,check));return controls;
}
module.exports={run};
