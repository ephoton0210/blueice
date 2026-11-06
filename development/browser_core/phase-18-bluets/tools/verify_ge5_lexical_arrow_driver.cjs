// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Public construction of all 51 products precedes narrow driver boundary checks.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),authority=require('./ge5_native_authority.cjs'),{read}=require('./ge5_lexical_arrow_read.cjs'),observer=require('./ge5_lexical_arrow_observe.cjs');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),reference=JSON.parse(fs.readFileSync(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'))),rows=observer.collect(path.resolve(process.argv[2]),process.argv[3]),failures=[];
for(const row of rows){const expected=reference.cases[row.index],name=Object.keys(expected.maps).find(name=>expected.maps[name].kind==='javascript'),map=expected.maps[name],code=row.outputs[map.output],raw=name.endsWith('#inline')?Buffer.from(code.split('\n').find(line=>line.startsWith('//# sourceMappingURL=data:')).split('base64,')[1],'base64').toString():row.outputs[name],n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,code,raw),nn=read(ts,n,row.options,row.files['main.ts'],false),aa=read(ts,a,row.options,row.files['main.ts'],true);
  if(nn.retained)continue;
  const nativeOpen=n.tokens(nn.fn).find(token=>token.text==='('),actualOpen=a.tokens(aa.callback).find(token=>token.text==='(');
  const signature=row.options.target==='ES5'?[['moved ES5 parameter opening',nativeOpen.start,actualOpen.start]]:[];
  for(const[label,ni,ai]of [...signature,['driver entry',nn.driver.getStart(n.source),aa.driver.getStart(a.source)],['driver end',nn.driver.end,aa.driver.end],['callback body entry',nn.callback.body.getStart(n.source),aa.callback.body.getStart(a.source)]])try{assert.deepEqual(a.owner(ai),n.owner(ni),label);}catch(error){failures.push({id:row.id,label,native:n.owner(ni),actual:a.owner(ai)});}
}
console.log(JSON.stringify({completeFamily:false,scope:'public driver boundary regression only',configurations:rows.length,repeatBuilds:2,compilerSha256:rows[0].compilerSha256,failures},null,2));assert.equal(failures.length,0,'all original async-arrow driver boundaries');
