// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs=require('fs'),cp=require('child_process');
const acorn=require('../oracle_support/load_acorn.cjs')(process.env.BLUEICE_ACORN_ORACLE || 'acorn');
const [file,target,moduleKind,id]=process.argv.slice(2);let syntaxAccepted=true;
try{acorn.parse(fs.readFileSync(file,'utf8'),{ecmaVersion:'latest',sourceType:moduleKind==='CommonJS'?'script':'module'});}catch{syntaxAccepted=false;}
const code=moduleKind==='CommonJS'?'process.stdout.write(JSON.stringify(require(process.argv[1]).result))':'import(require("url").pathToFileURL(process.argv[1]).href).then(m=>process.stdout.write(JSON.stringify(m.result)))';
const r=cp.spawnSync('node',['-e',code,file],{encoding:'utf8'});
console.log(JSON.stringify({syntaxAccepted,observation:r.status===0?JSON.parse(r.stdout):null,runtimeErrorCategory:r.status===0?null:r.stderr}));
