// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Focused complete-cohort transport proof; this does not close class role gaps.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs');
const {stream}=require('./ge5_role_stream.cjs'),{read}=require('./ge5_module_envelope.cjs'),{mutate}=require('./ge5_role_mutations.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input);
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const observations=JSON.parse(fs.readFileSync(0,'utf8')),verified=[],failures=[],controls=[];const identity=require('./ge5_class_cases.cjs').verify(root,input,reference,observations);
for(const row of observations) {
  const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];
  for(const [name,map]of Object.entries(expected.maps)) {
    if(map.kind!=='javascript')continue;
    try {
      const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString('utf8'):row.outputs[name];
      const native=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),actual=stream(ts,code,raw);
      const file=map.output.endsWith('/base.js')?'base.ts':'main.ts',dependencies=original.options.importHelpers&&original.options.target==='ES5'?['tslib']:[];
      const contexts=[{blue:false,view:native},{blue:true,view:actual}];
      for(const context of contexts) {
        const settings={blue:context.blue,originals:[original.files[file]],dependencies};
        const proof=read(ts,context.view,original.options,settings);proof.classifyTransport();
        if(proof.wrapperTokens.length) {
          const position=proof.wrapperTokens.find(token=>token.end-token.start>1).start+1;
          assert.throws(()=>read(ts,mutate(ts,context.view,position,[0,0,0]),original.options,settings).classifyTransport(),undefined,'envelope source grant must fail');
          controls.push({index:row.index,map:name,blue:context.blue,kind:'generated envelope source grant',rejected:true});
          const token=proof.wrapperTokens.find(token=>token.text==='register'||token.text==='define'||token.text==='"object"');assert(token);
          const replacement=token.text.startsWith('"')?'"x'+token.text.slice(2):token.text.slice(0,-1)+'x';
          const changed=stream(ts,context.view.code.slice(0,token.start)+replacement+context.view.code.slice(token.end),context.view.raw);
          assert.throws(()=>read(ts,changed,original.options,settings).classifyTransport(),undefined,'changed module registration/branch must fail');
          controls.push({index:row.index,map:name,blue:context.blue,kind:'module registration/branch code',rejected:true});
        }
      }
      verified.push({index:row.index,id:row.id,map:name,format:contexts[1].view.source.statements.length,nativeAndBlue:true});
    }catch(error){failures.push({index:row.index,id:row.id,map:name,error:error.message,stack:error.stack});}
  }
}
const report={...identity,scope:'complete class-family transport envelopes only; full class roles remain pending',configurations:observations.length,maps:verified.length+failures.length,verified,controls,failures};
assert.equal(report.configurations,661);assert.equal(report.maps,712);
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'class-envelope.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify({scope:report.scope,configurations:report.configurations,maps:report.maps,verified:verified.length,controls:controls.length,failures},null,2));if(failures.length)process.exitCode=1;
