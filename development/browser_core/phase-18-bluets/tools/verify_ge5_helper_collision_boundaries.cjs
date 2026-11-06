// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Public boundary regression; complete role closure is verified separately.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream,compareTokens}=require('./ge5_role_stream.cjs');
const root=path.resolve(__dirname,'../../../..'),fixture=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes=fs.readFileSync(path.join(fixture,'cases.json')),referenceBytes=fs.readFileSync(path.join(fixture,'reference.json'));
assert.equal(graph.sha256(inputBytes),'5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(graph.sha256(referenceBytes),'166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
const input=JSON.parse(inputBytes),reference=JSON.parse(referenceBytes),parents=graph.parents(root,input);
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const form='ge2_module_helpers/helper-name-collision',rows=process.argv[2]?require('./ge5_async_arrow_observe.cjs').collect(path.resolve(process.argv[2]),process.argv[3],{form}):JSON.parse(fs.readFileSync(0,'utf8'));
assert.deepEqual(rows.map(row=>row.index),input.cases.flatMap((row,index)=>row.form===form?[index]:[]));assert.equal(rows.length,363);
if(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY,'helper-name-collision.json'),JSON.stringify(rows)+'\n');}
const failures=[];let headers=0,operands=0,arrows=0;
function find(node,predicate){const matches=[];function visit(n){if(predicate(n))matches.push(n);ts.forEachChild(n,visit);}visit(node);return matches;}
for(const row of rows){const expected=reference.cases[row.index],original=graph.resolve(input,input.cases[row.index],parents);assert.equal(row.id,expected.id);assert.deepEqual(row.files,original.files);assert.deepEqual(row.options,original.options);assert.equal(row.repeatBuilds,2);
 for(const[name,map]of Object.entries(expected.maps))if(map.kind==='javascript'){
  const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name];
  const n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,code,raw);assert.deepEqual(a.decoded.metadata,n.decoded.metadata);
  const functions=[n,a].map(v=>{const nodes=find(v.source,node=>ts.isFunctionDeclaration(node)&&node.name?.text==='compute');assert.equal(nodes.length,1,'exact original compute declaration');return nodes[0];});
  const calls=[n,a].map(v=>{const nodes=find(v.source,node=>ts.isCallExpression(node)&&ts.isPropertyAccessExpression(node.expression)&&node.expression.expression.getText(v.source)==='Promise'&&node.expression.name.text==='resolve'&&node.arguments.length===1&&node.arguments[0].getText(v.source)==='34');assert.equal(nodes.length,1,'exact original awaited operand');return nodes[0];});
  try{const nh=n.tokens(functions[0]).filter(t=>t.start<functions[0].body.getStart(n.source)+1),ah=a.tokens(functions[1]).filter(t=>t.start<functions[1].body.getStart(a.source)+1);
   if(['ES5','ES2015','ES2016'].includes(row.options.target)){const returned=functions[1].body.statements.at(-1);assert(ts.isReturnStatement(returned)&&ts.isCallExpression(returned.expression));const callback=returned.expression.arguments.at(-1);assert(ts.isFunctionExpression(callback));const cb=a.tokens(callback).filter(t=>t.start<callback.body.getStart(a.source)+1),no=nh.findIndex(t=>t.text==='('),ao=ah.findIndex(t=>t.text==='('),co=cb.findIndex(t=>t.text==='(');compareTokens(n,nh.slice(0,no),a,ah.slice(0,ao),'original compute identity');compareTokens(n,nh.slice(no),a,cb.slice(co),'complete relocated original compute signature');compareTokens(n,[nh.at(-1)],a,[ah.at(-1)],'original compute outer body entry');}
   else compareTokens(n,nh,a,ah,'complete retained original compute signature');headers++;
  }catch(e){failures.push({id:row.id,role:'header',error:e.message});}
  try{compareTokens(n,n.tokens(calls[0]),a,a.tokens(calls[1]),'complete original await operand');assert.deepEqual(a.owner(calls[1].end),n.owner(calls[0].end),'original await operand exact endpoint');operands++;}catch(e){failures.push({id:row.id,role:'await endpoint',error:e.message});}
  try{const origin=require('./ge5_helper_collision_read.cjs').source(ts,original.files['main.ts']);require('./ge5_helper_collision_arrow.cjs').compare(ts,n,a,row.options,origin);arrows++;}catch(e){failures.push({id:row.id,role:'concise arrow',error:e.message});}
 }
}
const report={scope:'helper collision public signatures, await endpoints and concise arrows',compilerSha256:rows[0].compilerSha256,configurations:rows.length,headers,operands,arrows,failures};assert(rows.every(row=>row.compilerSha256===report.compilerSha256));console.log(JSON.stringify(report,null,2));if(failures.length)process.exitCode=1;
