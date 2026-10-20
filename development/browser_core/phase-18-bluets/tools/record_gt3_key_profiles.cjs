// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Observed type-only key sets and ordering, not copied native compiler code.
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const root = path.resolve(__dirname,'../../../..');
const ts = require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const targets = ['ES5','ES2015','ES2016','ES2017','ES2018','ES2019','ES2020','ES2021','ES2022','ES2023','ESNext'];
const expressions = {String:'string',Number:'number',Boolean:'boolean',BigInt:'bigint',Symbol:'symbol',Array:'any[]',ReadonlyArray:'readonly any[]'};
const hash = data => crypto.createHash('sha256').update(data).digest('hex');
const cache = new Map();
function program(target,mode,source) {
    const options = {strict:true,target:ts.ScriptTarget[target],module:ts.ModuleKind.ESNext,types:[]};
    if (mode === 'explicit') options.lib = ['lib.'+target.toLowerCase()+'.d.ts'];
    const name = path.join(root,'__gt3_keys__','entry.ts');
    const host = ts.createCompilerHost(options), read = host.getSourceFile;
    host.getSourceFile = (file,version,...args) => {
        if(file===name) return ts.createSourceFile(file,source,version,true);
        const key = target+':'+mode+':'+file;
        if(!cache.has(key)) cache.set(key,read(file,version,...args));
        return cache.get(key);
    };
    const p = ts.createProgram([name],options,host);
    const diagnostics = ts.getPreEmitDiagnostics(p);
    if(diagnostics.length) throw Error(ts.formatDiagnosticsWithColorAndContext(diagnostics,host));
    return {p,c:p.getTypeChecker(),entry:p.getSourceFile(name)};
}
const profiles = [];
for(const target of targets) for(const mode of ['default','explicit']) {
    const seed = program(target,mode,'export {};');
    const cutoff = seed.c.getTypeCount(), strings = new Map();
    const remember = type => {
        type = type.regularType || type;
        if(type.id<=cutoff && type.flags&ts.TypeFlags.StringLiteral) strings.set(type.value,type.id);
    };
    for(const value of ['','string','number','bigint','boolean','symbol','undefined','object','function']) remember(seed.c.getStringLiteralType(value));
    for(const file of seed.p.getSourceFiles().filter(file=>file.isDeclarationFile)) {
        const visit = node => {if(ts.isLiteralTypeNode(node)) remember(seed.c.getTypeAtLocation(node));ts.forEachChild(node,visit);};
        visit(file);
    }
    const declarations = Object.entries(expressions).map(([name,type])=>`type ${name}Keys = keyof ${type};`).join('\n');
    const {p,c,entry} = program(target,mode,declarations+'\nexport {};');
    const keysets = [];
    for(const node of entry.statements.filter(ts.isTypeAliasDeclaration)) {
        const type = c.getTypeAtLocation(node.type);
        const parts = type.flags&ts.TypeFlags.Never ? [] : type.isUnion() ? type.types : [type];
        const keys = parts.map(part=>{
            if(part.flags&ts.TypeFlags.Number) return {kind:'number'};
            if(part.flags&ts.TypeFlags.StringLiteral) return {kind:'text',value:part.value};
            if(part.flags&ts.TypeFlags.UniqueESSymbol) return {kind:'symbol',value:part.symbol.getName()};
            throw Error(`Unaccounted key type ${c.typeToString(part)}`);
        });
        keysets.push({name:node.name.text.slice(0,-4),keys,display:c.typeToString(type,undefined,ts.TypeFormatFlags.InTypeAlias|ts.TypeFormatFlags.AllowUniqueESSymbolType)});
    }
    const libraries = p.getSourceFiles().filter(file=>file.isDeclarationFile).map(file=>({name:path.basename(file.fileName),sha256:hash(file.text.replaceAll('\r\n','\n'))})).sort((a,b)=>a.name.localeCompare(b.name,'en'));
    profiles.push({target:target.toLowerCase(),mode,libraries,keysets,strings:[...strings].sort((a,b)=>a[1]-b[1]).map(([text])=>text)});
}
for(const target of targets) {
    const [a,b] = profiles.filter(profile=>profile.target===target.toLowerCase());
    if(JSON.stringify(a.keysets)!==JSON.stringify(b.keysets)) throw Error('Default/explicit ECMAScript keys differ: '+target);
}
const controls = profiles.map(profile=>({id:profile.target+'-'+profile.mode,sha256:hash(JSON.stringify(profile))}));
for(const [relative,value] of [
    ['backend/bluets/src/type_operator_profiles/native-5.9.3.json',{_license:'MPL-2.0',schema:1,typescript:ts.version,profiles}],
    ['backend/bluets/tests/fixtures/gt3_operators/key-profiles.json',{schema:1,typescript:ts.version,controls}],
]) {
    const file = path.join(root,relative), text = JSON.stringify(value,null,2)+'\n';
    if(process.env.BLUEICE_WRITE_GT3_KEY_PROFILES === '1') {fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,text);}
    else if(fs.readFileSync(file,'utf8').replaceAll('\r\n','\n')!==text) throw Error('Native key profiles changed: '+relative);
}
console.log(`Verified ${profiles.length} independently observed native key profiles`);
