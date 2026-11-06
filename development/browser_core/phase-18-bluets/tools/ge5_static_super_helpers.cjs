// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Versioned computed-key assets are admitted by whole AST and exact binding identity.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),proofs=new WeakMap();
function create(ts,view,original){assert(ts.isSourceFile(original));const names=new Set();function sourceNames(n){if(ts.isIdentifier(n))names.add(n.text);ts.forEachChild(n,sourceNames);}sourceNames(original);
 const asset=ts.createSourceFile('computed_property_helpers.v1.js',fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/computed_property_helpers.v1.js'),'utf8'),99,true,ts.ScriptKind.JS),semantic=require('./ge5_decorator_helpers.cjs').semantic,specs=new Map(asset.statements.filter(ts.isFunctionDeclaration).map(n=>[n.name.text,semantic(ts,n)])),bindings=new Map();
 for(const node of view.source.statements.filter(ts.isFunctionDeclaration)){const match=/^(__blueice_target_(?:property_key|computed_property))_\d+$/.exec(node.name?.text||'');if(!match)continue;assert(!names.has(node.name.text),'computed helper is fresh in original AST');assert(!bindings.has(node.name.text),'unique computed helper binding');assert.deepEqual(semantic(ts,node),specs.get(match[1]),'entire versioned computed-key helper AST');view.noSource(node.getStart(view.source),node.end,'whole canonical computed-key helper');bindings.set(node.name.text,{node,kind:match[1].endsWith('computed_property')?'define-computed':'property-key'});}
 const capability={};proofs.set(capability,{ts,view,original,bindings});return capability;
}
function resolve(capability,ts,view,original,node){if(!capability)return null;const proof=proofs.get(capability);assert(proof&&proof.ts===ts&&proof.view===view&&proof.original===original,'exact helper identity capability');assert(ts.isIdentifier(node)&&node.getSourceFile()===view.source);const binding=proof.bindings.get(node.text);if(!binding)return null;
 for(let scope=node.parent;scope&&scope!==view.source;scope=scope.parent){if(ts.isFunctionLike(scope)){assert(!scope.parameters.some(p=>ts.isIdentifier(p.name)&&p.name.text===node.text),'helper call cannot resolve to a local formal');const local=[];function visit(n){if(n!==scope&&ts.isFunctionLike(n))return;if(ts.isVariableDeclaration(n)&&ts.isIdentifier(n.name)&&n.name.text===node.text)local.push(n);ts.forEachChild(n,visit);}visit(scope);assert.equal(local.length,0,'helper call cannot resolve to a shadow binding');}}
 return binding.kind;
}
module.exports={create,resolve};
