// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Closed lexical-arrow expressions compose with the existing full class/module ABI.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),reader=require('./ge5_lexical_arrow_read.cjs'),frames=require('./ge5_lexical_arrow_frames.cjs'),classModule=require('./ge5_class_module.cjs');
function compare(ts,native,actual,options,original){assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);const lines=original.split('\n');for(const view of[native,actual])for(const segment of view.decoded.segments)if(segment.length>2)assert(segment[2]===0&&segment[3]<lines.length&&segment[4]<=lines[segment[3]].length,'every callable source coordinate is within the original content');
  if(options.inlineSourceMap){for(const view of[native,actual]){const match=/(?:^|\n)\/\/# sourceMappingURL=data:application\/json;base64,([^\r\n]+)\r?\n?$/.exec(view.code);assert(match);assert.equal(Buffer.from(match[1],'base64').toString(),view.raw);}}else{const url=view=>/(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(view.code)?.[1];assert.equal(url(actual),url(native),'external lexical map publication');}
  let expressionProof,items;
  const proof=classModule.compare(ts,native,actual,options,original,{
    prepare({n,a,original:source}){const nn=reader.read(ts,native,options,original,false),aa=reader.read(ts,actual,options,original,true),sourceNames=new Set();function names(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,names);}names(source);
      for(const[item,view,blue]of[[n,native,false],[a,actual,true]]){const helpers=item.sourceNodes.filter(node=>blue?ts.isFunctionDeclaration(node)&&/^__blueice_target_(?:generator|delegate_iterator|delegate_step|async_driver|async)_\d+$/.test(node.name?.text):ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&['__awaiter','__generator'].includes(node.declarationList.declarations[0].name.getText(view.source)));
        const expected=options.target==='ES5'?blue?4:2:['ES2015','ES2016'].includes(options.target)?1:0;assert.equal(helpers.length,expected,'complete lexical promise helper inventory');
        if(blue&&helpers.length){const generator=helpers.filter(node=>/^__blueice_target_(?:generator|delegate_iterator|delegate_step)_\d+$/.test(node.name.text));if(options.target==='ES5')require('./ge5_binding_helpers.cjs').validate(ts,view,generator,sourceNames);else assert.equal(generator.length,0);const driver=helpers.filter(node=>!generator.includes(node));assert.equal(driver.length,1);const name=driver[0].name.text;item.exact(driver[0],fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/async_helpers.v1.js'),'utf8').replaceAll('__blueice_target_async',name));assert.equal(aa.driver.expression.getText(actual.source),name);}
        for(const node of helpers)view.noSource(node.getStart(view.source),node.end,'complete lexical promise/generator helper');
      }
      expressionProof=frames.compare(ts,native,nn,actual,aa,original,options);items={native:nn,actual:aa};return expressionProof;
    },
    classRoles(source){const field=source.members.find(member=>ts.isPropertyDeclaration(member)&&member.name.getText(source.getSourceFile())==='read');assert(field?.initializer);return {pairedExpressions:[{source:field.initializer,native:items.native.expression,actual:items.actual.expression}],...(expressionProof.capture?{nativeConstructorReceiverCapture:{...expressionProof.capture,source:field.initializer}}:{})};},
  });
  return {...proof,expression:expressionProof,items};
}
module.exports={compare};
