// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), path = require('path'), os = require('os');
const crypto = require('crypto'), vm = require('vm'), assert = require('assert');
const repository = path.resolve(__dirname, '../../../..');
const corpus = path.join(repository, 'backend/bluets/tests/fixtures/output_options');
const ts = require(path.join(repository, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
assert.strictEqual(ts.version, '5.9.3');
const reference = JSON.parse(fs.readFileSync(path.join(corpus, 'reference.json'), 'utf8'));
const inputs = reference.cases.map(item => ({...item, config: JSON.parse(fs.readFileSync(path.join(corpus, item.id, 'tsconfig.json'), 'utf8')), sources: {'main.ts': fs.readFileSync(path.join(corpus, item.id, 'main.ts'), 'utf8')}}));
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-output-draft-'));
const libraries = path.dirname(ts.getDefaultLibFilePath({})), parsedLibraries = new Map();
async function record() {
    const cases = [];
    for (const item of inputs) {
        const directory = path.join(root, item.id);
        fs.mkdirSync(directory);
        for (const [name, source] of Object.entries(item.sources)) fs.writeFileSync(path.join(directory, name), source);
        const config = path.join(directory, 'tsconfig.json');
        fs.writeFileSync(config, JSON.stringify(item.config, null, 2) + '\n');
        const configSource = ts.readJsonConfigFile(config, ts.sys.readFile);
        const parsed = ts.parseJsonSourceFileConfigFileContent(configSource, ts.sys, directory);
        assert.deepStrictEqual(parsed.errors, [], item.id);
        const host = ts.createCompilerHost(parsed.options), original = host.getSourceFile, outputs = {};
        host.getSourceFile = (file, ...args) => {
            if (path.dirname(file) !== libraries) return original(file, ...args);
            const key = file + ':' + JSON.stringify(args[0]);
            if (!parsedLibraries.has(key)) parsedLibraries.set(key, original(file, ...args));
            return parsedLibraries.get(key);
        };
        host.writeFile = (file, text, bom) => outputs[path.basename(file)] = (bom ? '\ufeff' : '') + text;
        const program = ts.createProgram(parsed.fileNames, parsed.options, host);
        const diagnostics = ts.getPreEmitDiagnostics(program).map(d => ({
            code: d.code, file: d.file ? path.basename(d.file.fileName) : null,
            start: d.start, length: d.length, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'),
        }));
        const emitted = program.emit();
        const reference = {sources: Object.fromEntries(Object.entries(item.sources).map(([n, s]) => [n, crypto.createHash('sha256').update(s).digest('hex')])), diagnostics, emitSkipped: emitted.emitSkipped, files: Object.keys(outputs).sort()};
        if (diagnostics.length) {
            assert.strictEqual(diagnostics.length, 1, item.id);
            assert.strictEqual(diagnostics[0].code, 5102, item.id);
            assert.strictEqual(emitted.emitSkipped, true, item.id);
            assert.strictEqual(Object.keys(outputs).length, 0, item.id);
        } else {
            const javascript = outputs['main.js'], map = JSON.parse(outputs['main.js.map']);
            let exported;
            if (item.config.compilerOptions.module === 'CommonJS') {
                const module = {exports: {}};
                vm.runInNewContext(javascript, {module, exports: module.exports}, {filename: item.id + '/main.js'});
                exported = module.exports;
            } else {
                exported = await import('data:text/javascript;base64,' + Buffer.from(javascript).toString('base64'));
            }
            reference.observation = {result: exported.result, text: exported.text};
            assert.deepStrictEqual(reference.observation, {result: 42, text: '// literal /* retained */'}, item.id);
            reference.javascript = {bom: javascript.startsWith('\ufeff'), crlf: (javascript.match(/\r\n/g) || []).length, bareLF: (javascript.replaceAll('\r\n', '').match(/\n/g) || []).length, ordinaryComment: javascript.includes('retained ordinary comment'), publicComment: javascript.includes('Public answer.'), internalComment: javascript.includes('@internal'), literalRetained: javascript.includes('// literal /* retained */'), sourceMappingURL: javascript.split(/\r?\n/).find(x => x.includes('sourceMappingURL'))};
            reference.declaration = outputs['main.d.ts'];
            const casePath = directory.replaceAll('\\', '/');
            reference.sourceMap = {file: map.file, sourceRoot: map.sourceRoot, sources: map.sources.map(source => source.replace(casePath, '<case>').replace(casePath.replace(/^\//, ''), '<case>'))};
            if (map.sourcesContent !== undefined) reference.sourceMap.sourcesContent = map.sourcesContent;
        }
        cases.push({id: item.id, form: item.form, options: item.options, reference});
    }
    const matrix = cases.map(item => item.id + '/main.ts\t' + (item.reference.diagnostics.length ? 'reject' : 'accept') + '\n').join('');
    if (process.env.BLUEICE_WRITE_OUTPUT_OPTIONS_MATRIX === '1') {
        fs.writeFileSync(path.join(corpus, 'reference.json'), JSON.stringify({typescript: ts.version, cases}, null, 2) + '\n');
        fs.writeFileSync(path.join(corpus, 'output-options-checker-matrix.tsv'), matrix);
    } else {
        assert.deepStrictEqual({typescript: ts.version, cases}, reference);
        assert.strictEqual(fs.readFileSync(path.join(corpus, 'output-options-checker-matrix.tsv'), 'utf8').replaceAll('\r\n', '\n'), matrix);
    }
    console.log(JSON.stringify({typescript: ts.version, cases: cases.length, accepts: cases.filter(x => !x.reference.diagnostics.length).length, rejects: cases.filter(x => x.reference.diagnostics.length).length, executed: cases.filter(x => x.reference.observation).length}));
}
record().catch(error => {console.error(error); process.exitCode = 1;}).finally(() => fs.rmSync(root, {recursive: true, force: true}));
