// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independently observed native allocation facts; no TypeScript source is copied.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt1_declaration_order');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw Error('Foreign declaration matrix');
const globals = 'interface Array<T> {} interface Boolean {} interface Function {} interface CallableFunction extends Function {} interface NewableFunction extends Function {} interface IArguments {} interface Number {} interface Object {} interface RegExp {} interface String {}';
const targets = ['ES5','ES2015','ES2016','ES2017','ES2018','ES2019','ES2020','ES2021','ES2022','ES2023','ESNext'];
const cache = new Map();
const hash = value => crypto.createHash('sha256').update(value).digest('hex');

function program(target, mode, source, sources = new Map()) {
    const options = {strict:true,target:ts.ScriptTarget[target],module:ts.ModuleKind.ESNext,newLine:ts.NewLineKind.LineFeed,declaration:true,types:[]};
    if (mode === 'explicit') options.lib = ['lib.'+target.toLowerCase()+'.d.ts'];
    if (mode === 'none') options.lib = [];
    const base = path.join(root, '__gt1_order__');
    const name = path.join(base, 'entry.ts'), owned = path.join(base, 'owned-globals.d.ts');
    sources = new Map(sources);
    sources.set(name, source); sources.set(owned, globals);
    const host = ts.createCompilerHost(options), read = host.getSourceFile;
    const exists = host.fileExists, readFile = host.readFile, directoryExists = host.directoryExists;
    host.fileExists = file => sources.has(file) || exists(file);
    host.readFile = file => sources.get(file) ?? readFile(file);
    host.directoryExists = file => file === base || directoryExists(file);
    host.getSourceFile = (file, version, ...args) => {
        if (sources.has(file)) return ts.createSourceFile(file, sources.get(file), version, true);
        const key = target+':'+mode+':'+file;
        if (!cache.has(key)) cache.set(key, read(file, version, ...args));
        return cache.get(key);
    };
    let declaration;
    host.writeFile = (file, text) => { if (path.basename(file) === 'entry.d.ts') declaration = text.replaceAll('\r\n','\n'); };
    const p = ts.createProgram(mode === 'none' ? [owned,name] : [name], options, host);
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(p));
    const describe = d => ({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),module:d.file?path.basename(d.file.fileName):null,start:d.start??null,length:d.length??null,related:(d.relatedInformation||[]).map(describe)});
    return {p,diagnostics:diagnostics.map(describe),c:p.getTypeChecker(),emit:()=>{p.emit();return declaration;}};
}

if (process.argv.includes('--outputs')) {
    const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'), 'utf8'));
    const outputs = JSON.parse(fs.readFileSync(0, 'utf8'));
    if (outputs.length !== reference.cases.length) throw Error('Incomplete declaration consumers');
    for (let i = 0; i < outputs.length; i++) {
        const actual = outputs[i], expected = reference.cases[i];
        if (actual.id !== expected.id || actual.declaration !== expected.declaration) throw Error('Non-exact declaration consumer');
        const base = path.join(root, '__gt1_order__');
        const sources = new Map([[path.join(base,'blue.d.ts'),actual.declaration],[path.join(base,'native.d.ts'),expected.declaration]]);
        const consumer = "type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue'), typeof import('./native')>>; type Reverse = Assert<Equal<typeof import('./native'), typeof import('./blue')>>;";
        const {diagnostics} = program(expected.target, expected.library, consumer, sources);
        if (diagnostics.length) throw Error(`${expected.id}: ${JSON.stringify(diagnostics)}`);
    }
    console.log(`Verified ${outputs.length} exact native declaration consumers`);
    process.exit(0);
}

function seed(target, mode) {
    const {p,c,diagnostics} = program(target, mode, 'export {};');
    if (diagnostics.length) throw Error(JSON.stringify(diagnostics));
    // Freeze the cutoff before inspection can instantiate otherwise unused types.
    const cutoff = c.getTypeCount(), values = new Map();
    const record = value => {
        value = (value.regularType || value).freshType;
        if (!value || value.id > cutoff || !(value.flags & (ts.TypeFlags.StringLiteral|ts.TypeFlags.NumberLiteral))) return;
        values.set(typeof value.value+':'+JSON.stringify(value.value), {id:value.id,value:value.value});
    };
    for (const text of ['','string','number','bigint','boolean','symbol','undefined','object','function']) record(c.getStringLiteralType(text));
    record(c.getNumberLiteralType(0));
    for (const file of p.getSourceFiles().filter(file => file.isDeclarationFile)) {
        const visit = node => {if (ts.isLiteralTypeNode(node)) record(c.getTypeAtLocation(node)); ts.forEachChild(node,visit);};
        visit(file);
    }
    const libraries = p.getSourceFiles().filter(file => file.isDeclarationFile)
        .map(file => ({name:path.basename(file.fileName),sha256:hash(file.text.replaceAll('\r\n','\n'))}))
        .sort((a,b) => a.name.localeCompare(b.name,'en'));
    return {target:target.toLowerCase(),mode,libraries,values:[...values.values()].sort((a,b) => a.id-b.id).map(value => value.value)};
}
const profiles = [];
for (const target of targets) for (const mode of ['default','explicit','none']) profiles.push(seed(target,mode));
const catalog = {_license:'MPL-2.0',schema:1,typescript:ts.version,kind:'fresh-literal-allocation-precedence',options:{strict:true,module:'ESNext',types:[]},profiles};
const cases = [];
for (const target of input.targets) for (const mode of input.libraries) for (const form of input.forms) {
    const {diagnostics,emit} = program(target,mode,form.source);
    const row = {id:form.id+'-'+target.toLowerCase()+'-'+mode,form:form.id,target,library:mode,source:form.source,accepts:diagnostics.length===0,diagnostics};
    if (row.accepts) { row.declaration = emit(); if (typeof row.declaration !== 'string') throw Error('Missing native declaration'); }
    cases.push(row);
}
const profile_controls = profiles.map(profile => ({id:profile.target+'-'+profile.mode,sha256:hash(JSON.stringify(profile))}));
const reference = {schema:1,typescript:ts.version,options:{strict:true,module:'ESNext',newLine:'LF'},owned_globals:globals,cases,profile_controls};
for (const [output,value] of [
    [path.join(root,'backend/bluets/src/emitter/literal_order/native-5.9.3.json'),catalog],
    [path.join(directory,'reference.json'),reference],
]) {
    const text = JSON.stringify(value,null,2)+'\n';
    if (process.env.BLUEICE_WRITE_GT1_DECLARATION_ORDER === '1') {
        fs.mkdirSync(path.dirname(output),{recursive:true}); fs.writeFileSync(output,text);
    } else if (fs.readFileSync(output,'utf8').replaceAll('\r\n','\n') !== text) throw Error('Native declaration facts changed: '+output);
}
console.log(`Verified ${profiles.length} native allocation profiles and ${cases.length} declaration observations`);
