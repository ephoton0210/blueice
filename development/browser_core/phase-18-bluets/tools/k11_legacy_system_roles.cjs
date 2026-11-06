// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exact original K.11 legacy2/System1 domains; frozen G5 readers stay unchanged.
const assert=require('node:assert/strict'),crypto=require('node:crypto');
const legacy=require('./k11_legacy_fixed_functions.cjs'),system=require('./k11_system_source_roles.cjs');
const identities=new Map([["6b5ea6445580bfbe55cb491dde83154b7689ae38c71fdda2753fa8acb819aba4", {"case": "k11-legacy-class-member", "metadata": false, "module": "ES2022"}], ["84d4cad4d110aa257b3c00b90344e57390c4ddff460cf2997763b86cdea82d3f", {"case": "k11-legacy-parameter-metadata", "metadata": true, "module": "ES2022"}], ["f5be563ed189d1c9ba0f5e231f097af4660533868a7bcd7f1030a7fd68d42b3f", {"case": "k11-system-wrapper", "metadata": false, "module": "System"}]]);
const digest=text=>crypto.createHash('sha256').update(text).digest('hex');
function callableEnds(ts,proof,native,actual){const points=[];for(const fn of proof.delegated?.functions??[]){function visit(nn,an){assert.equal(an.kind,nn.kind,'complete fixed source callable AST kind');if(ts.isIdentifier(nn)||ts.isStringLiteral(nn)||ts.isNumericLiteral(nn))assert.equal(an.text,nn.text);assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'complete original callable logical endpoint');points.push({label:ts.SyntaxKind[nn.kind],native:nn.end,actual:an.end,owner:native.owner(nn.end)});const n=[],a=[];ts.forEachChild(nn,x=>{n.push(x);});ts.forEachChild(an,x=>{a.push(x);});assert.equal(a.length,n.length);for(let i=0;i<n.length;i++)visit(n[i],a[i]);}visit(fn.native,fn.actual);}return points;}
function compare(ts,native,actual,options,originalText){
  assert.equal(ts.version,'5.9.3');const identity=identities.get(digest(originalText));assert(identity,'exact unchanged K11 legacy/System source identity');
  assert.equal(options.target,'ES2022');assert.equal(options.module,identity.module);assert.equal(options.strict,true);assert.equal(options.experimentalDecorators,identity.module!=='System');assert.equal(options.emitDecoratorMetadata,identity.metadata);assert(options.useDefineForClassFields===undefined||options.useDefineForClassFields===true);assert.equal(typeof options.inlineSources,'boolean');
  for(const view of[native,actual]){const map=JSON.parse(view.raw);if(options.inlineSources)assert.deepEqual(map.sourcesContent,[originalText],'exact original inline content');else assert(!Object.hasOwn(map,'sourcesContent'),'original inlineSources=false profile');}
  const proof=(identity.module==='System'?system:legacy).compare(ts,native,actual,options,originalText),ends=callableEnds(ts,proof,native,actual);
  const scanner=ts.createScanner(ts.ScriptTarget.Latest,false,ts.LanguageVariant.Standard,originalText),comments=[];for(let kind=scanner.scan();kind!==ts.SyntaxKind.EndOfFileToken;kind=scanner.scan())if(kind===ts.SyntaxKind.SingleLineCommentTrivia||kind===ts.SyntaxKind.MultiLineCommentTrivia)comments.push(scanner.getTokenText());
  const sourceCommentEnds=[],nc=native.comments().filter(c=>comments.includes(c.text)),ac=actual.comments().filter(c=>comments.includes(c.text)&&!(proof.generatedComments??[]).some(g=>g.start===c.start&&g.end===c.end));assert.deepEqual(nc.map(c=>c.text),comments);assert.deepEqual(ac.map(c=>c.text),comments);
  for(let i=0;i<nc.length;i++){assert.deepEqual(actual.owner(ac[i].end),native.owner(nc[i].end),'every original comment logical endpoint');sourceCommentEnds.push({label:'original comment '+i,native:nc[i].end,actual:ac[i].end,owner:native.owner(nc[i].end)});}
  for(const view of[native,actual])for(let offset=0;offset<view.code.length;offset++)assert(view.covered(offset),'complete module UTF16 character classification');
  return {...proof,case:identity.case,callableEnds:ends,sourceCommentEnds,completeModuleRoles:true,completeUTF16Characters:{native:native.code.length,actual:actual.code.length}};
}
function runControls(ts,native,actual,options,originalText,proof){return require('./k11_legacy_system_controls.cjs').run(ts,native,actual,options,originalText,proof,compare);}
module.exports={compare,runControls};
