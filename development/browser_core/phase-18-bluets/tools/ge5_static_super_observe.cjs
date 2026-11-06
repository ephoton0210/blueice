// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict'),observe=require('./ge5_async_arrow_observe.cjs');
const families=new Map([['computed-reads',51],['simple-writes',43],['compound-logical',43],['prefix-postfix',43],['strict-failed-write',51],['arrows-nested-homeobjects',51]]);
function collect(compiler,hash,{family}={}){
 const selected=family?[family]:[...families.keys()];assert(selected.every(name=>families.has(name)),'closed frozen static-super families');
 const rows=selected.flatMap(name=>{const rows=observe.collect(compiler,hash,{form:'ge3_decorator_compositions/static-super-'+name});assert.equal(rows.length,families.get(name));return rows;});
 assert.equal(new Set(rows.map(row=>row.index)).size,rows.length,'disjoint complete source families');return rows;
}
module.exports={collect,families};
if(require.main===module)process.stdout.write(JSON.stringify(collect(process.argv[2],process.argv[3],{family:process.argv[4]}))+'\n');
