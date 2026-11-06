// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const observer=require('./ge5_async_arrow_observe.cjs');
function collect(compiler,hash){return observer.collect(compiler,hash,{form:'ge2_module_helpers/async-arrow-finally'});}
module.exports={collect};
