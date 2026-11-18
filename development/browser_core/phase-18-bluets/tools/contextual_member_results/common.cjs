// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c=require('../higher_order_inference/common.cjs');
const os=require('node:os');
const {spawnSync}=require('node:child_process');
const {fs,path,assert,sha,repo}=c;
const fixture=path.join(repo,'backend/bluets/tests/fixtures/contextual_member_results');
const read=name=>JSON.parse(fs.readFileSync(path.join(fixture,name)));
const declarations=outputs=>Object.fromEntries(Object.entries(outputs).filter(([name])=>/\.d\.(ts|mts|cts)$/.test(name)));
function compilerArtifacts(directory,surface,accepts) {
  const raw=c.inventory(directory),outputs={...raw},name='bluetsc.manifest.json';
  let ownerManifest=null;
  if(surface==='legacy'&&accepts) {
    assert(Object.hasOwn(raw,name),'legacy publisher must retain its owner receipt');
    const text=raw[name],manifest=JSON.parse(text);delete outputs[name];
    assert.equal(manifest.runtimePolicy,'checked');assert.equal(manifest.outputOwnership.version,1);
    assert.deepEqual(Object.keys(manifest.outputOwnership.files).sort(),Object.keys(outputs).sort(),'complete owner receipt inventory');
    for(const[file,value]of Object.entries(outputs)) {
      const entry=manifest.outputOwnership.files[file];assert.equal(entry.bytes,Buffer.byteLength(value));assert.equal(entry.sha256,sha(value));
    }
    ownerManifest={name,sha256:sha(text),bytes:Buffer.byteLength(text),verifiedArtifactCount:Object.keys(outputs).length};
  }
  return{outputs,ownerManifest,rawInventory:Object.fromEntries(Object.entries(raw).map(([name,text])=>[name,{sha256:sha(text),bytes:Buffer.byteLength(text)}]))};
}
function authorities(ctx) {
  const frozen=read('FROZEN.json');
  for(const[name,digest]of Object.entries(frozen.sha256))assert.equal(sha(fs.readFileSync(path.join(fixture,name))),digest,name);
  assert.equal(sha(fs.readFileSync(ctx.implementation)),frozen.implementationSha256);
  for(const[name,digest]of Object.entries(frozen.libraries))assert.equal(sha(fs.readFileSync(path.join(path.dirname(ctx.implementation),name))),digest,name);
  return frozen;
}
function capture() {
  const parent=process.env.BLUEICE_CONTEXTUAL_MEMBER_OBSERVATIONS||process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY||os.tmpdir();
  fs.mkdirSync(parent,{recursive:true});return fs.mkdtempSync(path.join(path.resolve(parent),'bluets-contextual-members-'));
}
function syntax(ctx,source,profile) {
  const esm=profile.compilerOptions.module==='ESNext',es5=profile.compilerOptions.target==='ES5';
  const ast=ctx.acorn.parse(source,{ecmaVersion:es5?(esm?2015:5):2022,sourceType:esm?'module':'script'});
  if(es5&&esm)for(const statement of ast.body) {
    if(statement.type==='ImportDeclaration')continue;
    const body=statement.type==='ExportNamedDeclaration'?statement.declaration:statement;
    if(body)ctx.acorn.parse(source.slice(body.start,body.end),{ecmaVersion:5,sourceType:'script'});
  }
}
function runtimeProgram(row,esm) {
  return(esm?'import * as namespace from "./main.js";\n':'const namespace = require("./main.js");\n')+
    'if (!Object.prototype.hasOwnProperty.call(namespace,"result")) throw Error("missing export");\n'+
    (row.form==='object-property-member-context'?'if (!namespace.result || !Object.prototype.hasOwnProperty.call(namespace.result,"value") || namespace.result.value !== undefined) throw Error("wrong record result");\n':
      'if (namespace.result !== undefined) throw Error("wrong result");\n')+'process.stdout.write("ok\\n");\n';
}
function runtime(directory,row,profile,outputs,expected) {
  fs.mkdirSync(directory,{recursive:true});
  for(const[name,source]of Object.entries(outputs).filter(([name])=>name.endsWith('.js')))fs.writeFileSync(path.join(directory,name),source);
  const esm=profile.compilerOptions.module==='ESNext';fs.writeFileSync(path.join(directory,'package.json'),JSON.stringify({type:esm?'module':'commonjs'})+'\n');
  const program=runtimeProgram(row,esm);assert.equal(sha(program),expected.runnerSha256,'unchanged authored execution runner');
  const runner=path.join(directory,esm?'check.mjs':'check.cjs');fs.writeFileSync(runner,program);
  const result=spawnSync(process.execPath,[runner],{cwd:directory,encoding:'utf8',timeout:30000});if(result.error)throw result.error;
  return{exit:result.status,stdout:result.stdout,stderr:result.stderr,runnerSha256:sha(program)};
}
function consumerDirectory(directory,options,source,graph) {
  fs.mkdirSync(directory,{recursive:true});
  for(const[name,text]of Object.entries(graph))fs.writeFileSync(path.join(directory,name),text);
  fs.writeFileSync(path.join(directory,'consumer.ts'),source);
  const config={compilerOptions:{...options,noEmit:true},files:['consumer.ts',...Object.keys(graph)].sort()};
  fs.writeFileSync(path.join(directory,'tsconfig.json'),JSON.stringify(config,null,2)+'\n');
  return config;
}
module.exports={...c,fixture,read,declarations,compilerArtifacts,authorities,capture,syntax,runtime,consumerDirectory};
