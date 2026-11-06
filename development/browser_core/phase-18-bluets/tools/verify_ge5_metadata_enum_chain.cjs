// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Read saved artifacts directly: this audit does not invoke or infer a compiler.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs'),{stream}=require('./ge5_role_stream.cjs'),enums=require('./ge5_metadata_enums.cjs'),envelope=require('./ge5_module_envelope.cjs');
const input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json')),parents=graph.parents(root,input),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const args=process.argv.slice(2);assert.equal(args.shift(),'--report');const output=args.shift();assert(output&&args.length,'usage: --report report.json observations.json [...]');
const points=[],errors=[],products=[],configurations=[];
for(const file of args)for(const row of JSON.parse(fs.readFileSync(file,'utf8'))) {
  const original=graph.resolve(input,input.cases[row.index],parents),expected=reference.cases[row.index];configurations.push({file,index:row.index,id:row.id,compilerSha256:row.compilerSha256});
  for(const[name,map]of Object.entries(expected.maps)) {
    if(map.kind!=='javascript')continue;const filename=path.posix.relative(original.options.outDir,map.output).replace(/\.js$/,'.ts');if(filename===original.entry)continue;
    try {
      const source=ts.createSourceFile(filename,original.files[filename],ts.ScriptTarget.Latest,true),declarations=source.statements.filter(ts.isEnumDeclaration);if(!declarations.length)continue;
      const code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name],n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,code,raw),options={originals:[source],dependencies:original.options.importHelpers&&original.options.target==='ES5'?['tslib']:[]},nn=envelope.read(ts,n,original.options,options),an=envelope.read(ts,a,original.options,{...options,blue:true});
      products.push({file,index:row.index,id:row.id,map:name,compilerSha256:row.compilerSha256,nativeJavaScriptSha256:expected.outputs[map.output],nativeMapSha256:graph.sha256(n.raw),actualJavaScriptSha256:graph.sha256(code),actualMapSha256:graph.sha256(raw)});
      for(const declaration of declarations)for(const pair of enums.audit(ts,n,a,nn,an,enums.source(ts,declaration),original.options).pairs)points.push({file,index:row.index,id:row.id,map:name,enum:declaration.name.text,label:pair.label,nativeKind:ts.SyntaxKind[pair.native.kind],actualKind:ts.SyntaxKind[pair.actual.kind],nativeEnd:pair.native.end,actualEnd:pair.actual.end,nativeOwner:pair.nativeOwner,actualOwner:pair.actualOwner,equal:JSON.stringify(pair.nativeOwner)===JSON.stringify(pair.actualOwner)});
    }catch(error){errors.push({file,index:row.index,id:row.id,map:name,error:error.stack});}
  }
}
const failures=points.filter(point=>!point.equal),report={scope:'read-only original provider enum complete generated AST endpoints; saved artifacts, no new CLI evidence',configurations:configurations.length,maps:products.length,points:points.length,affectedConfigurations:new Set(failures.map(row=>row.index)).size,affectedMaps:new Set(failures.map(row=>row.index+'\0'+row.map)).size,failedPoints:failures.length,errors,configurationsData:configurations,products,failures,result:points};
fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({...report,configurationsData:undefined,products:undefined,result:undefined,failures:failures.slice(0,8)},null,2));if(errors.length||failures.length)process.exitCode=1;
