// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), path = require('path'), os = require('os');
const crypto = require('crypto'), vm = require('vm'), assert = require('assert');
const repository = path.resolve(__dirname, '../../../..');
const support = path.join(repository, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(support, 'load_acorn.cjs'))();
const corpus = path.join(repository, 'backend/bluets/tests/fixtures/declaration_options');
const reference = JSON.parse(fs.readFileSync(path.join(corpus, 'reference.json')));
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-declaration-options-'));
const libraries = path.dirname(ts.getDefaultLibFilePath({})), cache = new Map();

async function record() {
    const cases = [];
    for (const item of reference.cases) {
        const directory = path.join(root, item.id);
        fs.mkdirSync(directory);
        const config = JSON.parse(fs.readFileSync(path.join(corpus, item.id, 'tsconfig.json')));
        assert.deepStrictEqual(config.compilerOptions, item.options, item.id);
        const metadata = JSON.parse(fs.readFileSync(path.join(corpus, item.id, 'metadata.json')));
        assert.deepStrictEqual(metadata, {id: item.id, form: item.form, options: item.options}, item.id);
        const source = fs.readFileSync(path.join(corpus, item.id, 'main.ts'), 'utf8');
        fs.writeFileSync(path.join(directory, 'main.ts'), source);
        const configPath = path.join(directory, 'tsconfig.json');
        fs.copyFileSync(path.join(corpus, item.id, 'tsconfig.json'), configPath);
        const parsed = ts.parseJsonSourceFileConfigFileContent(ts.readJsonConfigFile(configPath, ts.sys.readFile), ts.sys, directory);
        const host = ts.createCompilerHost(parsed.options), original = host.getSourceFile, outputs = {};
        host.getSourceFile = (file, ...args) => {
            if (path.dirname(file) !== libraries) return original(file, ...args);
            const key = file + JSON.stringify(args[0]);
            if (!cache.has(key)) cache.set(key, original(file, ...args));
            return cache.get(key);
        };
        host.writeFile = (file, text, bom) => outputs[path.relative(directory, file).replaceAll('\\', '/')] = (bom ? '\ufeff' : '') + text;
        const program = ts.createProgram(parsed.fileNames, parsed.options, host);
        const pre = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)];
        const emitted = program.emit();
        const all = [...pre];
        for (const d of emitted.diagnostics) {
            if (!all.some(old => old.code === d.code && old.start === d.start && old.file === d.file)) all.push(d);
        }
        const diagnostics = all.map(d => ({code: d.code,
            file: d.file ? path.basename(d.file.fileName) : null,
            start: d.start ?? null, length: d.length ?? null,
            message: ts.flattenDiagnosticMessageText(d.messageText, '\n')}));
        const row = {...item, sources: {'main.ts': crypto.createHash('sha256').update(source).digest('hex')},
            diagnostics, emitSkipped: emitted.emitSkipped, files: Object.keys(outputs).sort(), outputs,
            observation: null, syntaxError: null};
        if (diagnostics.length) {
            assert.strictEqual(emitted.emitSkipped, true, item.id);
            assert.deepStrictEqual(row.files, [], item.id);
        } else if (outputs['out/main.js']) {
            const javascript = outputs['out/main.js'];
            try { acorn.parse(javascript, {ecmaVersion: 2022, sourceType: item.options.module === 'CommonJS' ? 'script' : 'module'}); }
            catch (error) { row.syntaxError = String(error); }
            assert.strictEqual(row.syntaxError, null, item.id);
            let exported;
            if (item.options.module === 'CommonJS') {
                const module = {exports: {}};
                vm.runInNewContext(javascript, {module, exports: module.exports}, {filename: item.id});
                exported = module.exports;
            } else exported = await import('data:text/javascript;base64,' + Buffer.from(javascript + '\n//# sourceURL=' + item.id).toString('base64'));
            row.observation = JSON.parse(JSON.stringify(exported.result));
            assert.strictEqual(row.observation[0], 42, item.id);
        }
        cases.push(row);
    }
    const matrix = cases.map(c => `${c.id}/main.ts\t${c.diagnostics.length ? 'reject' : 'accept'}\n`).join('');
    if (process.env.BLUEICE_WRITE_DECLARATION_OPTIONS_MATRIX === '1') {
        fs.writeFileSync(path.join(corpus, 'reference.json'), JSON.stringify({typescript: ts.version, cases}, null, 2) + '\n');
        fs.writeFileSync(path.join(corpus, 'declaration-options-checker-matrix.tsv'), matrix);
    } else {
        assert.deepStrictEqual({typescript: ts.version, cases}, reference);
        assert.strictEqual(fs.readFileSync(path.join(corpus, 'declaration-options-checker-matrix.tsv'), 'utf8').replaceAll('\r\n', '\n'), matrix);
    }
    console.log(JSON.stringify({typescript: ts.version, cases: cases.length, accepted: cases.filter(c => !c.diagnostics.length).length,
        actualNodeExecutions: cases.filter(c => c.observation).length}));
}
record().catch(error => {console.error(error); process.exitCode = 1;}).finally(() => fs.rmSync(root, {recursive: true, force: true}));
