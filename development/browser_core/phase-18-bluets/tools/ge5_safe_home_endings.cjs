// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Enumerate all retained logical subtrees as well as the typed moved class phases.
const assert=require('node:assert/strict'),ir=require('./ge5_safe_home_ir.cjs'),pairs=require('./ge5_safe_home_pairs.cjs');
function compare(ts,n,a,proof) {
  const points=[];
  for(const fn of proof.delegated.prelude.proofs)points.push(...pairs.ends(ts,n.view,fn.native,a.view,fn.actual,'complete original top-level callable logical endings',{omitNative:[...proof.delegated.prelude.classes.flatMap(p=>p.extraNativeNodes).filter(node=>node.parent===fn.native.body),...fn.pairedExpressions.map(p=>p.native)],omitActual:fn.pairedExpressions.map(p=>p.actual)}));
  const base=n.cap.classes.find(c=>c!==n.cap.cls),pick=p=>{const list=p.view.source.statements.filter(c=>ts.isClassDeclaration(c)&&c.name.text===base.name.text);assert.equal(list.length,1);return list[0];};points.push(...pairs.ends(ts,n.view,pick(n),a.view,pick(a),'complete original Base class logical endings'));
  for(const statement of n.cap.original.statements.filter(ts.isVariableStatement))for(const d of statement.declarationList.declarations){assert(ts.isIdentifier(d.name));const name=d.name.text;const emitted=p=>{const list=p.view.source.statements.filter(s=>ts.isVariableStatement(s)&&s.declarationList.declarations.some(d=>d.name.text===name)||ts.isExpressionStatement(s)&&ts.isBinaryExpression(s.expression)&&ts.isPropertyAccessExpression(s.expression.left)&&s.expression.left.expression.getText(p.view.source)==='exports'&&s.expression.left.name.text===name&&!ts.isBinaryExpression(s.expression.right));assert.equal(list.length,1,'complete original module variable '+name);return list[0];};points.push(...pairs.ends(ts,n.view,emitted(n),a.view,emitted(a),'complete original module variable logical endings'));}
  const publication=p=>p.view.source.statements.filter(s=>ts.isExpressionStatement(s)&&ts.isBinaryExpression(s.expression)&&ts.isPropertyAccessExpression(s.expression.left)&&s.expression.left.expression.getText(p.view.source)==='exports'&&s.expression.left.name.text===n.cap.cls.name.text);
  const np=publication(n),ap=publication(a);assert.equal(np.length,ap.length);assert.equal(np.length,Number(n.cap.cls.modifiers?.some(m=>m.kind===ts.SyntaxKind.ExportKeyword)&&proof.native.module==='CommonJS'));for(let i=0;i<np.length;i++)points.push(...pairs.ends(ts,n.view,np[i],a.view,ap[i],'complete class publication logical endings'));
  for(const entry of proof.delegated.definitions)points.push(...entry.points);
  const main=proof.classes.find(p=>p.source===n.cap.cls);points.push(...main.points,...main.frames.points,...main.metadata.points);
  assert(points.length);const inventory=new Map();for(const point of points){assert(Number.isInteger(point.actual)&&Number.isInteger(point.native));const old=inventory.get(point.actual);if(old)assert.deepEqual(old.owner,point.owner,'one actual endpoint has one Native authority');else inventory.set(point.actual,point);}
  return {points,unique:[...inventory.values()],completeOriginalLogicalSubtrees:true};
}
module.exports={compare};
