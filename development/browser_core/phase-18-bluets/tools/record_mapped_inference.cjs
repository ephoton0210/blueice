// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native expectations and consumers are calibrated before compiler changes.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
const ts = require(implementation);
assert.equal(ts.version, '5.9.3');
const fixture = path.join(repo, 'backend/bluets/tests/fixtures/mapped_inference');
const verify = process.argv[2] === '--verify';
assert.deepEqual(process.argv.slice(2), verify ? ['--verify'] : []);
const inputBytes = fs.readFileSync(path.join(fixture, 'cases.json'));
const input = JSON.parse(inputBytes), referencePath = path.join(fixture, 'reference.json');
const frozenReference = verify ? fs.readFileSync(referencePath) : null;
if (!verify) assert(!fs.existsSync(referencePath), 'never overwrite a frozen reference');
const sha = value => createHash('sha256').update(value).digest('hex');
const parent = process.env.BLUEICE_MAPPED_INFERENCE_OBSERVATIONS;
const capture = parent ? path.join(path.resolve(parent), 'native-' + process.pid)
    : fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-mapped-inference-native-'));
fs.mkdirSync(capture, {recursive: true});
const consumer = require('./string_intrinsics/consumers.cjs')(ts, implementation);
const {writeSources} = require('./string_intrinsics/files.cjs');
const cache = new Map(), libraries = {};
function diagnostic(row, root) {
    let file = null, position = null;
    if (row.file && row.start !== undefined) {
        const at = row.file.getLineAndCharacterOfPosition(row.start);
        file = path.relative(root, row.file.fileName).replaceAll('\\', '/');
        position = {line: at.line + 1, column: at.character + 1, length: row.length};
    }
    return {code: row.code, message: ts.flattenDiagnosticMessageText(row.messageText, '\n'), file, position,
        relatedInformation: (row.relatedInformation || []).map(value => diagnostic(value, root))};
}
function resultType(declaration, kind) {
    const file = ts.createSourceFile('main.d.ts', declaration, ts.ScriptTarget.Latest, true);
    if (kind === 'function') {
        const fn = file.statements.find(row => ts.isFunctionDeclaration(row) && row.name?.text === 'result');
        assert(fn?.type, 'every accepted function exposes a typed result');
        return fn.type.getText(file);
    }
    for (const statement of file.statements) if (ts.isVariableStatement(statement)) {
        const variable = statement.declarationList.declarations.find(row => row.name.getText(file) === 'result');
        if (variable) { assert(variable.type); return variable.type.getText(file); }
    }
    throw Error('accepted case must export result');
}
const cases = [], authoringErrors = [];
let consumers = 0, executions = 0;
for (const profile of input.profiles) for (const entry of input.cases) {
    const id = profile.id + '/' + entry.id, cwd = path.join(capture, profile.id, entry.id);
    fs.mkdirSync(cwd, {recursive: true}); writeSources(cwd, entry);
    const converted = ts.convertCompilerOptionsFromJson({...input.options, ...profile.compilerOptions,
        ...entry.compilerOptions, outDir: 'dist'}, cwd);
    assert.deepEqual(converted.errors, []);
    const host = ts.createCompilerHost(converted.options), get = host.getSourceFile;
    host.getSourceFile = (file, version, ...rest) => {
        const relative = path.relative(path.dirname(implementation), file);
        if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, version, ...rest);
        const key = file + ':' + version;
        if (!cache.has(key)) cache.set(key, get(file, version, ...rest));
        libraries[path.basename(file)] = sha(fs.readFileSync(file));
        return cache.get(key);
    };
    const output = {};
    host.writeFile = (file, text) => {
        const relative = path.relative(path.join(cwd, 'dist'), file).replaceAll('\\', '/');
        assert(!relative.startsWith('../'));
        const bytes = Buffer.from(text, 'utf8'); output[relative] = bytes.toString('utf8');
        fs.mkdirSync(path.dirname(file), {recursive: true}); fs.writeFileSync(file, bytes);
    };
    const program = ts.createProgram(['main.ts', ...Object.keys(entry.files || {})].map(file => path.join(cwd, file)), converted.options, host);
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(row => diagnostic(row, cwd));
    if (!diagnostics.length) program.emit();
    const declarations = Object.fromEntries(Object.entries(output).filter(([file]) => file.endsWith('.d.ts')));
    const row = {id, form: entry.id, profile: profile.id, accepts: diagnostics.length === 0, diagnostics, declarations};
    if (row.accepts) {
        const expectedType = resultType(declarations['main.d.ts'], entry.resultKind);
        // The expected annotation comes only from the independent Native output.
        // The positive consumer therefore rejects an imprecise BlueTS producer.
        const expression = entry.resultKind === 'function' ? 'result()' : 'result';
        row.consumerSources = {
            positive: `import {result} from "./main";\nconst accepted: ${expectedType} = ${expression};\n`,
            negative: expectedType === 'never'
                ? 'import {result} from "./main";\nconst rejected: typeof result = true;\n'
                : `import {result} from "./main";\nconst rejected: never = ${expression};\n`,
        };
        const root = path.join(cwd, 'consumers'); fs.mkdirSync(root);
        for (const [name, text] of Object.entries(declarations)) {
            fs.mkdirSync(path.dirname(path.join(root, name)), {recursive: true});
            fs.writeFileSync(path.join(root, name), text);
        }
        row.consumers = {};
        for (const polarity of ['positive', 'negative']) {
            const actual = consumer(root, converted.options, row.consumerSources[polarity], declarations);
            row.consumers[polarity] = actual; consumers++;
            if (actual.accepts !== (polarity === 'positive')) authoringErrors.push({id, polarity, actual});
        }
        if (entry.runtime) {
            const esm = profile.compilerOptions.module === 'ESNext';
            const source = output['main.js'] + '\n' + (esm ? 'if(runtime!==2)throw Error("runtime");' : 'if(module.exports.runtime!==2)throw Error("runtime");') + '\n';
            const ran = spawnSync(process.execPath, ['--input-type=' + (esm ? 'module' : 'commonjs')], {input: source, encoding: 'utf8', timeout: 120000});
            if (ran.error) throw ran.error;
            row.runtime = {exit: ran.status, stdout: ran.stdout, stderr: ran.stderr}; executions++;
            if (ran.status !== 0) authoringErrors.push({id, runtime: row.runtime});
        }
    }
    fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
    cases.push(row);
}
const report = {schema: 1, typescript: ts.version, node: process.version,
    implementationSha256: sha(fs.readFileSync(implementation)), libraries, inputSha256: sha(inputBytes),
    forms: input.cases.length, profiles: input.profiles.length, comparisons: cases.length,
    accepted: cases.filter(row => row.accepts).length, rejected: cases.filter(row => !row.accepts).length,
    declarationFiles: cases.reduce((sum, row) => sum + Object.keys(row.declarations).length, 0),
    consumers, runtime: executions, authoringErrors, cases,
    conclusion: authoringErrors.length ? 'authoring-errors' : 'independently-calibrated'};
fs.writeFileSync(path.join(capture, 'report.json'), JSON.stringify(report, null, 2) + '\n');
if (authoringErrors.length) { console.error(JSON.stringify(authoringErrors, null, 2)); process.exitCode = 1; }
else {
    if (verify) {
        const {node: expectedNode, ...expected} = JSON.parse(frozenReference);
        const {node: actualNode, ...actual} = report;
        assert.deepEqual(actual, expected, 'frozen Native mapped-inference expectations changed');
        assert.deepEqual(fs.readFileSync(referencePath), frozenReference);
        fs.writeFileSync(path.join(capture, 'verification.json'), JSON.stringify({conclusion: 'success', expectedNode, actualNode,
            inputSha256: report.inputSha256, referenceSha256: sha(frozenReference)}, null, 2) + '\n', {flag: 'wx'});
    } else fs.writeFileSync(referencePath, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
    console.log(JSON.stringify({capture, forms: report.forms, comparisons: report.comparisons, accepted: report.accepted,
        rejected: report.rejected, declarations: report.declarationFiles, consumers, runtime: executions,
        inputSha256: report.inputSha256, referenceSha256: sha(fs.readFileSync(referencePath))}, null, 2));
}
