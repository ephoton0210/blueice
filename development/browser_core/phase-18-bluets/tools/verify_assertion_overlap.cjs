// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c=require('./contextual_member_results/common.cjs');
const {spawnSync}=require('node:child_process'),{isDeepStrictEqual}=require('node:util');
const {fs,path,assert,sha}=c;
const authorityPath=path.join(c.fixture,'ASSERTION_OVERLAP_FROZEN.json');
if(process.argv[3])assert.equal(process.argv[3],fs.readFileSync(authorityPath,'utf8'),'compiled public-boundary authority');
const frozen=JSON.parse(fs.readFileSync(authorityPath));
for(const[name,digest]of Object.entries(frozen.sha256))assert.equal(sha(fs.readFileSync(path.join(c.fixture,name))),digest,name);
assert.equal(sha(fs.readFileSync(path.join(c.fixture,'FROZEN.json'))),frozen.primaryFixtureUnchangedSha256);
const aliasAuthorityPath=path.join(c.fixture,'ASSERTION_ALIAS_FROZEN.json');
if(process.argv[4])assert.equal(process.argv[4],fs.readFileSync(aliasAuthorityPath,'utf8'),'compiled alias-regression authority');
const aliasFrozen=JSON.parse(fs.readFileSync(aliasAuthorityPath));
for(const[name,digest]of Object.entries(aliasFrozen.sha256))assert.equal(sha(fs.readFileSync(path.join(c.fixture,name))),digest,name);
assert.equal(aliasFrozen.originalAssertionSupplementSha256,sha(fs.readFileSync(authorityPath)));
assert.equal(aliasFrozen.primaryFixtureUnchangedSha256,frozen.primaryFixtureUnchangedSha256);
const suites=[{input:c.read('assertion-overlap-cases.json'),reference:c.read('assertion-overlap-reference.json')},{input:c.read('assertion-alias-cases.json'),reference:c.read('assertion-alias-reference.json')}];
const compiler=fs.realpathSync(process.argv[2]),compilerSha256=sha(fs.readFileSync(compiler)),root=c.capture();
const report={schema:1,conclusion:'in_progress',root,compiler,compilerSha256,authoritySha256:sha(fs.readFileSync(authorityPath)),aliasAuthoritySha256:sha(fs.readFileSync(aliasAuthorityPath)),inventoryCredit:0,cases:[],differences:[]};
const save=()=>{const p=path.join(root,'assertion-overlap-report.tmp');fs.writeFileSync(p,JSON.stringify(report,null,2)+'\n');fs.renameSync(p,path.join(root,'assertion-overlap-report.json'));};
try {
 save();
 for(const {input,reference}of suites)for(const surface of ['native','legacy'])for(const wanted of reference.cases){
  const profile=input.profiles.find(p=>p.id===wanted.profile),entry=input.cases.find(e=>e.id===wanted.form),prepared=c.prepare(path.join(root,surface),input,profile,entry);
  const owner=path.join(root,'owner-state',surface,wanted.id);fs.mkdirSync(owner,{recursive:true,mode:0o700});
  const args=surface==='native'?['--project','tsconfig.json','--diagnostics-json']:['build','--config','tsconfig.json','--diagnostics-json'];
  const result=spawnSync(compiler,args,{cwd:prepared.cwd,encoding:'utf8',timeout:30000,env:{...process.env,BLUETSC_OWNER_STATE_DIR:owner}});if(result.error)throw result.error;
  fs.writeFileSync(path.join(prepared.cwd,'build.stdout'),result.stdout||'');fs.writeFileSync(path.join(prepared.cwd,'build.stderr'),result.stderr||'');
  const diagnostics=(result.stderr||'').split(/\r?\n/).filter(Boolean).map(s=>{const row=JSON.parse(s);try{return c.blueDiagnostic(row);}catch{return{unmatchedBlueDiagnostic:row};}});
  const accepts=result.status===0,artifacts=c.compilerArtifacts(path.join(prepared.cwd,'dist'),surface,accepts),declarations=c.declarations(artifacts.outputs);
  const row={id:wanted.id,surface,accepts,exitCode:result.status,diagnostics,declarations,rawInventory:artifacts.rawInventory,ownerManifest:artifacts.ownerManifest,differences:[]};
  if(accepts!==wanted.accepts)row.differences.push('verdict');if(!isDeepStrictEqual(diagnostics,wanted.diagnostics))row.differences.push('complete-diagnostics');if(!isDeepStrictEqual(declarations,wanted.declarations))row.differences.push('exact-declarations');if(!wanted.accepts&&Object.keys(artifacts.outputs).length)row.differences.push('rejected-artifacts');
  fs.writeFileSync(path.join(prepared.cwd,'observation.json'),JSON.stringify(row,null,2)+'\n');report.cases.push(row);if(row.differences.length)report.differences.push({id:row.id,surface,differences:row.differences});if(report.cases.length%14===0)save();
 }
 assert.equal(report.cases.length,192);assert.equal(report.differences.length,0,'all constrained-assertion regression verdicts, diagnostics and declarations');assert.equal(sha(fs.readFileSync(compiler)),compilerSha256);report.conclusion='complete-assertion-overlap-regressions-passed';save();console.log(JSON.stringify({root,compilerSha256,conclusion:report.conclusion,observations:192,inventoryCredit:0}));
}catch(error){report.conclusion='failure';report.error=String(error.stack||error);save();console.error(JSON.stringify({root,conclusion:report.conclusion,observations:report.cases.length,differences:report.differences.length,first:report.differences.slice(0,8),error:report.error}));process.exitCode=1;}
