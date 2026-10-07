// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// K.4.5 accepted/rejected operators and exact primary evidence from TypeScript.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const corpus = path.join(fixtures, 'typescript_oracle');
const ts = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'))(
    process.env.BLUEICE_BLUETSC_ORACLE || 'tsc',
);
if (ts.version !== '5.9.3') throw new Error(`Expected TypeScript 5.9.3, received ${ts.version}`);
const options = {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022,
    strict: true, noEmit: true, allowImportingTsExtensions: true,
};
const library = path.dirname(ts.getDefaultLibFilePath(options));
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
    .filter(item => item.isDirectory() && item.name.startsWith('toper-'))
    .map(item => item.name).sort().map(name => {
        const filename = path.join(corpus,name,'main.ts');
        const host = ts.createCompilerHost(options);
        const read = host.getSourceFile;
        host.getSourceFile = (file,version,onError,createNew) => {
            if (path.dirname(file) !== library) return read(file,version,onError,createNew);
            const key = `${file}:${JSON.stringify(version)}`;
            if (!parsedLibraries.has(key)) parsedLibraries.set(key,read(file,version,onError,createNew));
            return parsedLibraries.get(key);
        };
        const program = ts.createProgram([filename],options,host);
        const diagnostics = ts.getPreEmitDiagnostics(program).filter(item=>item.category===ts.DiagnosticCategory.Error);
        return {entry:`${name}/main.ts`,accepts:diagnostics.length===0,
            runtime:name.startsWith('toper-runtime-'),first:diagnostics.length?diagnostic(diagnostics[0]):null};
    });
const record = JSON.stringify({version:ts.version,cases},null,2)+'\n';
const matrix = cases.map(item=>`${item.entry}\t${item.accepts?'accept':'reject'}\n`).join('');
const destination = path.join(corpus,'operators-reference.json');
const matrixPath = path.join(corpus,'operators-checker-matrix.tsv');
if (process.env.BLUEICE_WRITE_OPERATORS_MATRIX === '1') {
    fs.writeFileSync(destination,record);fs.writeFileSync(matrixPath,matrix);
} else if (fs.readFileSync(destination,'utf8').replaceAll('\r\n','\n')!==record
    || fs.readFileSync(matrixPath,'utf8').replaceAll('\r\n','\n')!==matrix) {
    throw new Error('Operators observations differ from pinned TypeScript');
}
if (cases.filter(item=>item.runtime).some(item=>!item.accepts)) {
    throw new Error('A runtime witness is rejected by TypeScript');
}
process.stdout.write(JSON.stringify({cases:cases.length,accept:cases.filter(item=>item.accepts).length,
    reject:cases.filter(item=>!item.accepts).length,runtime:cases.filter(item=>item.runtime).length})+'\n');
