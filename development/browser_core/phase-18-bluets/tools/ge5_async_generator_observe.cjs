// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Keep every frozen async-generator target, module and publication product.
const assert=require('node:assert/strict'),path=require('node:path'),observer=require('./ge5_async_arrow_observe.cjs');
function collect(compiler,hash){return observer.collect(compiler,hash,{form:'ge1_downlevel/async-generator-exception-template'});}
module.exports={collect};
if(require.main===module){assert(process.argv[2]);process.stdout.write(JSON.stringify(collect(path.resolve(process.argv[2]),process.argv[3]))+'\n');}
