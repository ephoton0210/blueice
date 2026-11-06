// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const observe=require('./ge5_async_arrow_observe.cjs');
function collect(compiler,hash){return observe.collect(compiler,hash,{form:'ge2_module_helpers/standard-decorator'});}
module.exports={collect};
if(require.main===module)process.stdout.write(JSON.stringify(collect(process.argv[2],process.argv[3]))+'\n');
