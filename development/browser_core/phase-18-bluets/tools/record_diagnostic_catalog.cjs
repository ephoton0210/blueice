// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Pinned diagnostic metadata, without copying TypeScript checking algorithms.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
if (ts.version !== '5.9.3') throw new Error(`Expected TypeScript 5.9.3, received ${ts.version}`);
const templates = Object.fromEntries(Object.values(ts.Diagnostics)
    .sort((left, right) => left.code - right.code)
    .map(item => [item.code, item.message]));
const filename = path.join(root, '__diagnostic_catalog__.ts');
const options = { target: ts.ScriptTarget.ES2022, lib: ['lib.es2022.d.ts'], strict: true, noEmit: true };
const host = ts.createCompilerHost(options);
const read = host.getSourceFile;
host.getSourceFile = (name, version, onError, createNew) => name === filename
    ? ts.createSourceFile(name, '', version)
    : read(name, version, onError, createNew);
const program = ts.createProgram([filename], options, host);
const checker = program.getTypeChecker();
const source = program.getSourceFile(filename);
const constructors = {};
const members = {};
const libraryDeclarations = {};
const librarySignatures = {};
function recordSignatures(key,signatures,activeChecker=checker,activeSource=source,activeProgram=program) {
    const files=activeProgram.getSourceFiles();
    signatures=[...signatures].sort((a,b)=>files.indexOf(b.declaration?.getSourceFile())-files.indexOf(a.declaration?.getSourceFile()));
    librarySignatures[key]=signatures.map(signature=>({
        parameters:signature.parameters.map(parameter=>({name:parameter.name,type:activeChecker.typeToString(activeChecker.getTypeOfSymbolAtLocation(parameter,activeSource),activeSource,ts.TypeFormatFlags.NoTruncation),optional:!!(parameter.valueDeclaration?.questionToken || parameter.valueDeclaration?.initializer),rest:!!parameter.valueDeclaration?.dotDotDotToken})),
        result:activeChecker.typeToString(activeChecker.getReturnTypeOfSignature(signature),activeSource,ts.TypeFormatFlags.NoTruncation),
        minimum:signature.minArgumentCount,
    }));
}
function recordDeclaration(key,symbol) {
    const candidates=symbol.declarations?.filter(item=>path.dirname(item.getSourceFile().fileName)===path.dirname(ts.getDefaultLibFilePath(options))) || [];
    const declaration=candidates.find(ts.isVariableDeclaration) || candidates[0];
    if (!declaration) return;
    const source=declaration.getSourceFile();
    const node=ts.isVariableDeclaration(declaration)?declaration.name:declaration;
    const start=node.getStart(source),end=node.end;
    const position=source.getLineAndCharacterOfPosition(start);
    libraryDeclarations[key]={file:`<typescript-lib>/${path.basename(source.fileName)}`,
        start:Buffer.byteLength(source.text.slice(0,start)),end:Buffer.byteLength(source.text.slice(0,end)),
        line:position.line+1,column:position.character+1,length:end-start,
        sourceLine:source.text.split(/\r\n|\r|\n/)[position.line]};
}
for (const name of ['Object', 'Array', 'String', 'Number', 'Boolean', 'Date', 'RegExp',
    'Error', 'EvalError', 'RangeError', 'ReferenceError', 'SyntaxError', 'TypeError', 'URIError', 'JSON', 'Promise']) {
    const symbol = checker.resolveName(name, source, ts.SymbolFlags.Value, false);
    if (!symbol) throw new Error(`Missing library value ${name}`);
    const value = checker.getTypeOfSymbolAtLocation(symbol, source);
    recordDeclaration(name,symbol);
    constructors[name] = value.getConstructSignatures().length;
    recordSignatures(`es2022/new ${name}`,value.getConstructSignatures());
    for (const member of value.getProperties()) recordSignatures(`es2022/${name}Constructor.${member.name}`,checker.getTypeOfSymbolAtLocation(member,source).getCallSignatures());
    members[`${name}Constructor`] = Object.fromEntries(value.getProperties().map(member => [
        member.name, checker.getTypeOfSymbolAtLocation(member, source).getCallSignatures().length,
    ]).sort(([left], [right]) => left.localeCompare(right, 'en')));
}
for (const name of ['Array', 'String', 'Number', 'Boolean']) {
    const symbol = checker.resolveName(name, source, ts.SymbolFlags.Type, false);
    const value = checker.getDeclaredTypeOfSymbol(symbol);
    for (const member of value.getProperties()) recordDeclaration(`${name}.${member.name}`,member);
    for (const member of value.getProperties()) recordSignatures(`es2022/${name}.${member.name}`,checker.getTypeOfSymbolAtLocation(member,source).getCallSignatures());
    members[name] = Object.fromEntries(value.getProperties().map(member => [
        member.name, checker.getTypeOfSymbolAtLocation(member, source).getCallSignatures().length,
    ]).sort(([left], [right]) => left.localeCompare(right, 'en')));
}
const compilerOptionNames = ts.optionDeclarations.map(option => option.name).sort();
const baseProgram=ts.createProgram([filename],{...options,lib:['lib.es2020.d.ts']},host);
const baseChecker=baseProgram.getTypeChecker(),baseSource=baseProgram.getSourceFile(filename);
for (const name of ['Object','Array','String','Number','Boolean','Date','RegExp','Error','EvalError','RangeError','ReferenceError','SyntaxError','TypeError','URIError','JSON','Promise']) {
    const symbol=baseChecker.resolveName(name,baseSource,ts.SymbolFlags.Value,false);
    const value=baseChecker.getTypeOfSymbolAtLocation(symbol,baseSource);
    recordSignatures(`es2020/new ${name}`,value.getConstructSignatures(),baseChecker,baseSource,baseProgram);
    for (const member of value.getProperties()) recordSignatures(`es2020/${name}Constructor.${member.name}`,baseChecker.getTypeOfSymbolAtLocation(member,baseSource).getCallSignatures(),baseChecker,baseSource,baseProgram);
}
for (const name of ['Array','String','Number','Boolean']) {
    const value=baseChecker.getDeclaredTypeOfSymbol(baseChecker.resolveName(name,baseSource,ts.SymbolFlags.Type,false));
    for (const member of value.getProperties()) recordSignatures(`es2020/${name}.${member.name}`,baseChecker.getTypeOfSymbolAtLocation(member,baseSource).getCallSignatures(),baseChecker,baseSource,baseProgram);
}
for (const name of ['Promise','Iterator','Generator','IterableIterator']) {
    for (const [profile,activeProgram,activeChecker,activeSource] of [['es2020',baseProgram,baseChecker,baseSource],['es2022',program,checker,source]]) {
        const value=activeChecker.getDeclaredTypeOfSymbol(activeChecker.resolveName(name,activeSource,ts.SymbolFlags.Type,false));
        for (const member of value.getProperties()) recordSignatures(`${profile}/${name}.${member.name}`,activeChecker.getTypeOfSymbolAtLocation(member,activeSource).getCallSignatures(),activeChecker,activeSource,activeProgram);
    }
}
const catalog = { version: ts.version, templates, constructors, members, compilerOptionNames,libraryDeclarations,librarySignatures };
const text = JSON.stringify(catalog, null, 2) + '\n';
const destination = path.join(root, 'backend/bluets/src/diagnostic/typescript-5.9.3.json');
if (process.env.BLUEICE_WRITE_DIAGNOSTICS_MATRIX === '1') fs.writeFileSync(destination, text);
else if (fs.readFileSync(destination, 'utf8') !== text) throw new Error('Pinned diagnostic catalog changed');
process.stdout.write(`Recorded ${Object.keys(templates).length} templates and ${Object.keys(members).length} library member catalogs\n`);
