// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// K.5.2 generic/dynamic class verdicts and diagnostic origins from the pinned compiler.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const corpus = path.join(fixtures, 'typescript_oracle');
const ts = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'))(
    process.env.BLUEICE_BLUETSC_ORACLE || 'tsc',
);
const defaults = {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022,
    strict: true, noEmit: true, allowImportingTsExtensions: true,
};
const library = path.dirname(ts.getDefaultLibFilePath(defaults));
const parsedLibraries = new Map();
function diagnostic(item) {
    const record = {code:item.code, message:ts.flattenDiagnosticMessageText(item.messageText,'\n')};
    if (item.file && item.start !== undefined) {
        const position = item.file.getLineAndCharacterOfPosition(item.start);
        record.module = path.dirname(item.file.fileName) === library
            ? `<typescript-lib>/${path.basename(item.file.fileName)}` : path.basename(item.file.fileName);
        record.position = {line:position.line+1,column:position.character+1,length:item.length || 0};
    }
    record.related = (item.relatedInformation || []).map(diagnostic);
    return record;
}
const cases = fs.readdirSync(corpus,{withFileTypes:true})
    .filter(item => item.isDirectory() && item.name.startsWith('dynamic-'))
    .map(item => item.name).sort().map(name => {
        const directory = path.join(corpus,name);
        const flagFile = path.join(directory,'flags.txt');
        const flags = fs.existsSync(flagFile) ? fs.readFileSync(flagFile,'utf8').trim().split(/\s+/) : [];
        const parsed = ts.parseCommandLine(flags);
        if (parsed.errors.length) throw new Error(JSON.stringify(parsed.errors.map(diagnostic)));
        const options = {...defaults,...parsed.options};
        const host = ts.createCompilerHost(options);
        const read = host.getSourceFile;
        host.getSourceFile = (file,version,onError,createNew) => {
            if (path.dirname(file) !== library) return read(file,version,onError,createNew);
            const key = `${file}:${JSON.stringify(version)}`;
            if (!parsedLibraries.has(key)) parsedLibraries.set(key,read(file,version,onError,createNew));
            return parsedLibraries.get(key);
        };
        const program = ts.createProgram([path.join(directory,'main.ts')],options,host);
        const diagnostics = ts.getPreEmitDiagnostics(program).filter(item=>item.category===ts.DiagnosticCategory.Error);
        return {entry:`${name}/main.ts`,accepts:diagnostics.length===0,flags,
            runtime:name.startsWith('dynamic-runtime-'),
            declaration:name.startsWith('dynamic-runtime-') || name.startsWith('dynamic-declarations-'),
            first:diagnostics.length?diagnostic(diagnostics[0]):null};
    });
const record = JSON.stringify({version:ts.version,cases},null,2)+'\n';
const matrix = cases.map(item=>`${item.entry}\t${item.accepts?'accept':'reject'}\n`).join('');
const destination = path.join(corpus,'class-dynamic-reference.json');
const matrixPath = path.join(corpus,'class-dynamic-checker-matrix.tsv');
if (process.env.BLUEICE_WRITE_CLASS_DYNAMIC_MATRIX === '1') {
    fs.writeFileSync(destination,record);fs.writeFileSync(matrixPath,matrix);
} else if (fs.readFileSync(destination,'utf8').replaceAll('\r\n','\n')!==record
    || fs.readFileSync(matrixPath,'utf8').replaceAll('\r\n','\n')!==matrix) {
    throw new Error('Generic/dynamic class observations differ from pinned TypeScript');
}
if (cases.filter(item=>item.runtime || item.declaration).some(item=>!item.accepts)) {
    throw new Error('A runtime/declaration witness is rejected by TypeScript');
}
process.stdout.write(JSON.stringify({cases:cases.length,accept:cases.filter(item=>item.accepts).length,
    reject:cases.filter(item=>!item.accepts).length,runtime:cases.filter(item=>item.runtime).length,
    declarations:cases.filter(item=>item.declaration).length})+'\n');
