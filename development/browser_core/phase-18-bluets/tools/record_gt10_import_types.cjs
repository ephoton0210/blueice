// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native-first G-T10 observations; BlueTS never supplies expected results.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const {pathToFileURL} = require('node:url');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt10_import_types');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw Error('Foreign G-T10 input');
const libraries = new Map(), base = path.join(root, '__gt10_import_types__');
function compile(files, options, entry = 'entry.ts') {
    const converted = ts.convertCompilerOptionsFromJson(options, root);
    if (converted.errors.length) throw Error('Invalid native options');
    const native = converted.options, sources = new Map(Object.entries(files).map(([n,t])=>[path.join(base,n),t]));
    const host = ts.createCompilerHost(native), read = host.getSourceFile;
    const exists = host.fileExists, readFile = host.readFile, dirs = host.directoryExists;
    host.fileExists = file => sources.has(file) || exists(file);
    host.readFile = file => sources.get(file) ?? readFile(file);
    host.directoryExists = file => file === base || dirs(file);
    host.getSourceFile = (file, version, ...args) => {
        if (sources.has(file)) return ts.createSourceFile(file, sources.get(file), version, true);
        if (!libraries.has(file)) libraries.set(file, read(file, version, ...args));
        return libraries.get(file);
    };
    const declarations = {}, javascript = {};
    host.writeFile = (file,text) => {
        const name = path.relative(base,file).replaceAll(path.sep,'/');
        if (name.endsWith('.d.ts')) declarations[name] = text.replaceAll('\r\n','\n');
        if (name.endsWith('.js')) javascript[name] = text;
    };
    const program = ts.createProgram([path.join(base,entry)],native,host);
    // Only this recorder's virtual project prefix is portable path metadata.
    // Keep the native message, module identity and complete UTF-16 span intact.
    const describe = d => ({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,'\n').replaceAll(base+path.sep,''),
        module:d.file?path.relative(base,d.file.fileName).replaceAll(path.sep,'/'):null,
        start:d.start??null,length:d.length??null,related:(d.relatedInformation||[]).map(describe)});
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(describe);
    if (!diagnostics.length) program.emit();
    const sorted = object => Object.fromEntries(Object.entries(object).sort(([a],[b])=>a.localeCompare(b)));
    return {diagnostics,declarations:sorted(declarations),javascript:sorted(javascript)};
}
function imports(files, module) {
    return Object.fromEntries(Object.entries(files).map(([name,text])=>{
        const source = ts.createSourceFile(name,text,ts.ScriptTarget.Latest,true), result = [];
        const walk = node => {
            if (ts.isImportDeclaration(node)) result.push({specifier:node.moduleSpecifier.text,
                bindings:node.importClause ? [node.importClause.name?.text,...(node.importClause.namedBindings ?
                    ts.isNamespaceImport(node.importClause.namedBindings)?['*']:
                    node.importClause.namedBindings.elements.map(b=>b.propertyName?.text||b.name.text):[])].filter(Boolean).sort():[]});
            if (module==='CommonJS' && ts.isCallExpression(node) && ts.isIdentifier(node.expression) &&
                node.expression.text==='require' && node.arguments.length===1 && ts.isStringLiteral(node.arguments[0]))
                result.push({specifier:node.arguments[0].text});
            ts.forEachChild(node,walk);
        };
        walk(source);
        return [name,result];
    }));
}
function execute(files,module,invoke) {
    const temporary = fs.mkdtempSync(path.join(os.tmpdir(),'bluets-gt10-native-'));
    try {
        fs.writeFileSync(path.join(temporary,'package.json'),JSON.stringify({type:module==='ESNext'?'module':'commonjs'}));
        for (const [name,text] of Object.entries(files)) {
            if (!/^[\w-]+\.js$/.test(name)) throw Error('Unconfined runtime fixture');
            fs.writeFileSync(path.join(temporary,name),text);
        }
        const entry = path.join(temporary,'entry.js');
        const source = module==='ESNext' ? `import * as entry from ${JSON.stringify(pathToFileURL(entry).href)};\n` :
            `const entry = require(${JSON.stringify(entry)});\n`;
        const run = spawnSync(process.execPath,['--input-type='+ (module==='ESNext'?'module':'commonjs'),'-e',source+invoke],{encoding:'utf8'});
        if (run.error) throw run.error;
        return {exit:run.status,stdout:run.stdout.replaceAll('\r\n','\n'),stderr:run.stderr.replaceAll('\r\n','\n')};
    } finally { fs.rmSync(temporary,{recursive:true,force:true}); }
}
function declarationInputs(row,declarations) {
    return {...Object.fromEntries(Object.entries(row.files).filter(([name])=>name.endsWith('.d.ts'))),...declarations};
}
if (process.argv.includes('--outputs')) {
    const expected = JSON.parse(fs.readFileSync(path.join(directory,'reference.json'),'utf8')).cases.filter(r=>r.accepts);
    const actual = JSON.parse(fs.readFileSync(0,'utf8'));
    const resolve = require(path.join(support,'resolve_package.cjs'));
    const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE,'acorn','dist/acorn.js'));
    if (acorn.version!=='8.15.0'||actual.length!==expected.length) throw Error('Incomplete/foreign G-T10 outputs');
    for (let i=0;i<expected.length;i++) {
        const a=actual[i],r=expected[i];
        if (a.id!==r.id || JSON.stringify(a.declarations)!==JSON.stringify(r.declarations)) throw Error(r.id+': non-exact declarations');
        if (JSON.stringify(imports(a.javascript,r.options.module))!==JSON.stringify(r.imports)) throw Error(r.id+': import retention/elision differs');
        for (const source of Object.values(a.javascript)) acorn.parse(source,{ecmaVersion:2022,sourceType:r.options.module==='ESNext'?'module':'script'});
        for (const consumer of Object.values(r.consumers)) {
            const result=compile({...declarationInputs(r,a.declarations),'consumer.ts':consumer.source},{...r.options,noEmit:true},'consumer.ts');
            if (JSON.stringify(result.diagnostics)!==JSON.stringify(consumer.diagnostics)) throw Error(r.id+': native consumer differs');
        }
        const equality="type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue.js'), typeof import('./native.js')>>; type Reverse = Assert<Equal<typeof import('./native.js'), typeof import('./blue.js')>>;";
        if (compile({...declarationInputs(r,a.declarations),'blue.d.ts':a.declarations['entry.d.ts'],'native.d.ts':r.declarations['entry.d.ts'],'consumer.ts':equality},{...r.options,noEmit:true},'consumer.ts').diagnostics.length) throw Error(r.id+': bidirectional consumer differs');
        if (r.invoke && JSON.stringify(execute(a.javascript,r.options.module,r.invoke))!==JSON.stringify(r.runtime)) throw Error(r.id+': runtime differs');
    }
    console.log(`Verified ${actual.length} exact declaration graphs, consumers, import retention/elision, syntax and executions`);
    process.exit(0);
}
const cases=[];
for (const form of [...input.forms,...input.rejections]) for (const module of form.modules||input.modules)
    for (const verdict of form.source?['reject']:['accept','reject']) {
        const options={...input.options,...form.options,module};
        const files={...form.files,'entry.ts':'// UTF-16 control: 😀\n'+(form.source||form.prefix+'\n'+form[verdict])+'\n'};
        const observed=compile(files,options),accepts=observed.diagnostics.length===0;
        if (accepts!==(verdict==='accept')) throw Error(form.id+': unexpected native '+verdict+' '+JSON.stringify(observed.diagnostics));
        const row={id:form.id+'-'+module.toLowerCase()+'-'+verdict,files,options,accepts,diagnostics:observed.diagnostics};
        if (accepts) {
            row.declarations=observed.declarations; row.imports=imports(observed.javascript,module); row.consumers={};
            for (const kind of ['accept','reject']) {
                const source="// UTF-16 consumer: 😀\nimport type {Value} from './entry.js';\n"+
                    (kind==='accept'?'export declare const accepted: Value;':'export const rejected: Value = undefined;')+'\n';
                const {diagnostics}=compile({...declarationInputs(row,observed.declarations),'consumer.ts':source},{...options,noEmit:true},'consumer.ts');
                if ((diagnostics.length===0)!==(kind==='accept')) throw Error(row.id+': unexpected consumer '+kind+' '+JSON.stringify(diagnostics));
                row.consumers[kind]={source,diagnostics};
            }
            if (form.invoke) {
                row.invoke=form.invoke; row.runtime=execute(observed.javascript,module,form.invoke);
                if (row.runtime.exit!==0) throw Error(row.id+': native runtime failed '+JSON.stringify(row.runtime));
            }
        }
        cases.push(row);
    }
const bytes=JSON.stringify({schema:1,typescript:ts.version,cases,owner_controls:input.owner_controls},null,2)+'\n';
const file=path.join(directory,'reference.json');
if (process.env.BLUEICE_WRITE_GT10_IMPORT_TYPES==='1') fs.writeFileSync(file,bytes);
else if (fs.readFileSync(file,'utf8').replaceAll('\r\n','\n')!==bytes) throw Error('Frozen native G-T10 observations changed');
console.log(`Verified ${cases.length} native G-T10 observations; ${cases.filter(r=>r.accepts).length} accepted graphs; ${cases.filter(r=>r.invoke).length} executions`);
