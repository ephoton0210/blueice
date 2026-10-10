// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), path = require('path'), os = require('os');
const crypto = require('crypto'), vm = require('vm'), assert = require('assert');
const repository = path.resolve(__dirname, '../../../..');
const support = path.join(repository, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(support, 'load_acorn.cjs'))();
const corpus = path.join(repository, 'backend/bluets/tests/fixtures/decorator_residuals');
const reference = JSON.parse(fs.readFileSync(path.join(corpus, 'reference.json')));
const libraries = path.dirname(ts.getDefaultLibFilePath({})), cache = new Map();
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-decorator-residuals-'));

async function execute(javascript, item) {
    let exported;
    if (item.options.module === 'CommonJS') {
        const module = {exports: {}};
        vm.runInNewContext(javascript, {module, exports: module.exports}, {filename: item.id});
        exported = module.exports;
    } else {
        exported = await import('data:text/javascript;base64,' + Buffer.from(javascript + '\n//# sourceURL=' + item.id).toString('base64'));
    }
    const observation = JSON.parse(JSON.stringify(exported.result));
    assert.strictEqual(observation[0], 42, item.id);
    return observation;
}

async function record() {
    const cases = [];
    for (const item of reference.cases) {
        const directory = path.join(root, item.id);
        fs.mkdirSync(directory);
        const source = fs.readFileSync(path.join(corpus, item.id, 'main.ts'), 'utf8');
        const config = JSON.parse(fs.readFileSync(path.join(corpus, item.id, 'tsconfig.json')));
        const metadata = JSON.parse(fs.readFileSync(path.join(corpus, item.id, 'metadata.json')));
        assert.deepStrictEqual(metadata, {form: item.form, options: item.options}, item.id);
        assert.deepStrictEqual(config.compilerOptions, item.options, item.id);
        fs.writeFileSync(path.join(directory, 'main.ts'), source);
        const parsed = ts.parseJsonConfigFileContent(config, ts.sys, directory);
        const host = ts.createCompilerHost(parsed.options), original = host.getSourceFile, outputs = {};
        host.getSourceFile = (file, ...args) => {
            if (path.dirname(file) !== libraries) return original(file, ...args);
            const key = file + JSON.stringify(args[0]);
            if (!cache.has(key)) cache.set(key, original(file, ...args));
            return cache.get(key);
        };
        host.writeFile = (file, text, bom) => outputs[path.basename(file)] = (bom ? '\ufeff' : '') + text;
        const program = ts.createProgram(parsed.fileNames, parsed.options, host);
        const diagnostics = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)].map(d => ({
            code: d.code, file: d.file ? path.basename(d.file.fileName) : null,
            start: d.start, length: d.length, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'),
        }));
        const emitted = program.emit();
        const row = {id: item.id, form: item.form, options: item.options,
            sources: {'main.ts': crypto.createHash('sha256').update(source).digest('hex')},
            diagnostics, emitSkipped: emitted.emitSkipped, files: Object.keys(outputs).sort(),
            declaration: outputs['main.d.ts'] || null, observation: null, runtimeError: null,
            syntaxError: null, javascript: outputs['main.js'] || null, typescriptParseErrors: []};
        if (diagnostics.length) {
            assert.strictEqual(emitted.emitSkipped, true, item.id);
            assert.deepStrictEqual(row.files, [], item.id);
        } else {
            const edition = item.options.target === 'ESNext' ? 'latest' : item.options.target === 'ES5' ? 5 : Number(item.options.target.slice(2));
            try { acorn.parse(row.javascript, {ecmaVersion: edition, sourceType: item.options.module === 'CommonJS' ? 'script' : 'module'}); }
            catch (error) { row.syntaxError = String(error); }
            try { row.observation = await execute(row.javascript, item); }
            catch (error) { row.runtimeError = String(error); }
            const parsedJS = ts.createSourceFile('main.js', row.javascript, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
            row.typescriptParseErrors = parsedJS.parseDiagnostics.map(d => ({code: d.code, start: d.start, length: d.length, message: ts.flattenDiagnosticMessageText(d.messageText, '\n')}));
            assert.deepStrictEqual(row.typescriptParseErrors, [], item.id);
            const features = {decorators: 0, autoAccessors: 0};
            function visit(node) {
                if (ts.canHaveDecorators(node)) features.decorators += (ts.getDecorators(node) || []).length;
                if (ts.isPropertyDeclaration(node) && node.modifiers?.some(m => m.kind === ts.SyntaxKind.AccessorKeyword)) features.autoAccessors++;
                ts.forEachChild(node, visit);
            }
            visit(parsedJS);
            row.proposalFeatures = features;
            if (row.runtimeError) {
                assert.strictEqual(item.options.target, 'ESNext', item.id);
                assert(features.decorators || features.autoAccessors, item.id);
                const lowered = ts.transpileModule(row.javascript, {fileName: 'main.js', reportDiagnostics: true,
                    compilerOptions: {target: ts.ScriptTarget.ES2023, module: item.options.module === 'CommonJS' ? ts.ModuleKind.CommonJS : ts.ModuleKind.ESNext, newLine: ts.NewLineKind.LineFeed, experimentalDecorators: false}});
                assert.deepStrictEqual(lowered.diagnostics, [], item.id);
                row.proposalExecution = {adapter: 'typescript@5.9.3 emitted-JavaScript-only ES2023 lowering', observation: await execute(lowered.outputText, item)};
            }
        }
        cases.push(row);
    }
    const matrix = cases.map(item => item.id + '/main.ts\t' + (item.diagnostics.length ? 'reject' : 'accept') + '\n').join('');
    if (process.env.BLUEICE_WRITE_DECORATOR_RESIDUALS_MATRIX === '1') {
        fs.writeFileSync(path.join(corpus, 'reference.json'), JSON.stringify({typescript: ts.version, cases}, null, 2) + '\n');
        fs.writeFileSync(path.join(corpus, 'decorator-residuals-checker-matrix.tsv'), matrix);
    } else {
        // Native Node error text can contain engine-version-specific columns; retain its category.
        const stable = rows => rows.map(row => ({...row, runtimeError: row.runtimeError?.split(':')[0] || null}));
        assert.strictEqual(reference.typescript, ts.version);
        assert.deepStrictEqual(stable(cases), stable(reference.cases));
        assert.strictEqual(fs.readFileSync(path.join(corpus, 'decorator-residuals-checker-matrix.tsv'), 'utf8').replaceAll('\r\n', '\n'), matrix);
    }
    console.log(JSON.stringify({typescript: ts.version, cases: cases.length, accepted: cases.filter(x => !x.diagnostics.length).length,
        directNode: cases.filter(x => x.observation).length, proposalOutputNode: cases.filter(x => x.proposalExecution).length}));
}
record().catch(error => {console.error(error); process.exitCode = 1;}).finally(() => fs.rmSync(root, {recursive: true, force: true}));
