// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Execute the actual emitted module in a context that supplies only its loader.
const vm=require('node:vm'),assert=require('node:assert/strict');
async function execute(ts,view,profile,branch,expectedResult,environment={}) {
  const exports={},module={exports},publications=[];let registered=null,defined=0;
  const reject=name=>assert.fail('unexpected module dependency: '+name);
  const dependency=profile.dependency||[];
  const require=name=>{assert(dependency.includes(name),'reviewed module dependency');assert.equal(name,'tslib');return environment.tslib;};
  const define=(dependencies,factory)=>{
    assert.deepEqual(Array.from(dependencies),['require','exports',...dependency]);defined++;
    const result=factory(require,exports,...dependency.map(require));if(result!==undefined)module.exports=result;
  };
  define.amd={};
  const sandbox={...environment.globals};
  if(profile.format==='system')sandbox.System={register(dependencies,factory){
    assert.deepEqual(Array.from(dependencies),dependency);assert.equal(registered,null);
    registered=factory((name,value)=>{publications.push([name,value]);exports[name]=value;},{id:'main'});
  }};
  else if(profile.format==='wrapped'&&(profile.module==='AMD'||branch==='amd'))sandbox.define=define;
  else if(profile.format!=='plain'||!profile.esm)Object.assign(sandbox,{exports,module,require});
  const context=vm.createContext(sandbox);
  let result;
  if(profile.esm) {
    assert.equal(typeof vm.SourceTextModule,'function','module execution requires --experimental-vm-modules');
    const instance=new vm.SourceTextModule(view.code,{context,identifier:'memory:///main.js'});
    await instance.link(name=>{const library=require(name);const members=environment.members||['__spreadArray'];return new vm.SyntheticModule(members,function(){for(const member of members)this.setExport(member,library[member]);},{context});});await instance.evaluate();assert.deepEqual(Object.keys(instance.namespace),['result']);result=instance.namespace.result;
  } else {
    vm.runInContext(view.code,context,{timeout:1000});
    if(profile.format==='system') {
      assert(registered&&Array.isArray(registered.setters)&&typeof registered.execute==='function');assert.equal(registered.setters.length,dependency.length);
      registered.setters.forEach((setter,index)=>setter(require(dependency[index])));
      registered.execute();assert.deepEqual(publications,[['result',expectedResult]]);result=exports.result;
    } else {if(profile.format==='wrapped'&&(profile.module==='AMD'||branch==='amd'))assert.equal(defined,1);result=module.exports.result;}
  }
  assert.equal(result,expectedResult,profile.module+' '+branch+' actual execution');
  return result;
}
async function run(ts,native,actual,options,proof) {
  const esm=proof.native.format==='plain'&&native.source.statements.some(statement=>statement.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));
  const profile={format:proof.native.format,module:options.module,esm};
  const branches=options.module==='UMD'?['commonjs','amd']:['default'];
  const variants=[];
  function fixture(view,item,mode) {
    if(mode==='original')return view;
    const statement=item.user[1];
    const initializer=ts.isVariableStatement(statement)?statement.declarationList.declarations[0].initializer:statement.expression.right;
    assert(ts.isObjectLiteralExpression(initializer));
    return {code:view.code.slice(0,initializer.getStart(view.source))+(mode==='null'?'null':'void 0')+view.code.slice(initializer.end)};
  }
  for(const mode of ['original','null','undefined']) {
    const expectedResult=mode==='original'?42:0;let executions=0;
    for(const branch of branches) {
      const expected=await execute(ts,fixture(native,proof.native,mode),profile,branch,expectedResult);
      const observed=await execute(ts,fixture(actual,proof.actual,mode),profile,branch,expectedResult);
      assert.equal(observed,expected);executions+=2;
    }
    variants.push({mode,executions,result:expectedResult});
  }
  return {executions:variants[0].executions,branches,actualResult:42,nativeResult:42,
    additionalFixtureExecutions:variants.slice(1).reduce((sum,row)=>sum+row.executions,0),variants};
}
module.exports={run,execute};
