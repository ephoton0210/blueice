// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Closed signature capabilities cannot designate arbitrary callbacks or ranges.
const assert=require('node:assert/strict'),signature=require('./ge5_relocated_callable_signature.cjs');
function run(ts,native,nm,actual,am,cap){const controls=[];function reject(label,value,left=native,right=actual){if(process.env.BLUEICE_GE5_ROLE_PROGRESS)console.error(label);assert.throws(()=>signature.verify(ts,left,nm,right,am,value),undefined,label);controls.push({label,rejected:true});}
  signature.verify(ts,native,nm,actual,am,cap);
  reject('unreviewed signature source kind',{...cap,sourceKind:'any-callable'});
  reject('method kind on arrow ancestry',{...cap,sourceKind:'async-object-method'});
  reject('wrong exact native callback',{...cap,nativeCallback:cap.nativeOuter});
  reject('wrong exact actual callback',{...cap,actualCallback:cap.actualOuter});
  reject('incomplete signature header',{...cap,ranges:[[cap.ranges[0][0]+1,cap.ranges[0][1]],cap.ranges[1]]});
  reject('formal pairing to generated outer slot',{...cap,formalPairs:[{...cap.formalPairs[0],actual:cap.actualOuter.parameters[0]}]});
  reject('formal pairing to a different original node',{...cap,formalPairs:[{...cap.formalPairs[0],source:cap.source} ]});
  const unproved=Object.create(actual);unproved.covered=offset=>offset===cap.ranges[1][0]+1?false:actual.covered(offset);
  const alternateMachine={...am,view:unproved};assert.throws(()=>signature.verify(ts,native,nm,unproved,alternateMachine,cap),undefined,'unproved UTF-16 header interior');controls.push({label:'unproved UTF-16 header interior',rejected:true});
  return controls;
}
module.exports={run};
