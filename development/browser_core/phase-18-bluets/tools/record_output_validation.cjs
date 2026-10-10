// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), path = require('path'), os = require('os');
const assert = require('assert'), crypto = require('crypto'), vm = require('vm');
const {spawnSync} = require('child_process');
const repository = path.resolve(__dirname, '../../../..');
const corpus = path.join(repository, 'backend/bluets/tests/fixtures/output_validation');
const ts = require(path.join(repository, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
assert.strictEqual(ts.version, '5.9.3');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-output-validation-'));
const libraryDirectory = path.dirname(ts.getDefaultLibFilePath({})), libraries = new Map();
try {
    const cases = [];
    for (const id of fs.readdirSync(corpus).filter(id => fs.statSync(path.join(corpus, id)).isDirectory()).sort()) {
        const input = path.join(corpus, id), directory = path.join(root, id);
        fs.mkdirSync(directory);
        for (const name of ['main.ts', 'tsconfig.json']) fs.copyFileSync(path.join(input, name), path.join(directory, name));
        const flags = JSON.parse(fs.readFileSync(path.join(input, 'flags.json'), 'utf8'));
        const command = ts.parseCommandLine(flags);
        const config = path.join(directory, 'tsconfig.json');
        const configSource = ts.readJsonConfigFile(config, ts.sys.readFile);
        const parsed = ts.parseJsonSourceFileConfigFileContent(configSource, ts.sys, directory, command.options);
        const host = ts.createCompilerHost(parsed.options), original = host.getSourceFile, outputs = {};
        host.getSourceFile = (file, ...args) => {
            if (path.dirname(file) !== libraryDirectory) return original(file, ...args);
            const key = file + ':' + JSON.stringify(args[0]);
            if (!libraries.has(key)) libraries.set(key, original(file, ...args));
            return libraries.get(key);
        };
        host.writeFile = (file, text, bom) => outputs[path.basename(file)] = (bom ? '\ufeff' : '') + text;
        const program = ts.createProgram({rootNames: parsed.fileNames, options: parsed.options, host, configFileParsingDiagnostics: parsed.errors});
        const diagnostics = [...command.errors, ...ts.getPreEmitDiagnostics(program)].map(d => ({
            code: d.code, file: d.file ? path.basename(d.file.fileName) : null,
            start: d.start, length: d.length, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'),
        }));
        const emitted = program.emit();
        const cliDirectory = path.join(directory, 'native');
        const cli = spawnSync(process.execPath, [path.join(libraryDirectory, 'tsc.js'), '--project', config, ...flags, '--outDir', cliDirectory], {encoding: 'utf8'});
        const cliFiles = fs.existsSync(cliDirectory) ? fs.readdirSync(cliDirectory).sort() : [];
        const reference = {
            sources: Object.fromEntries(['main.ts', 'tsconfig.json', 'flags.json'].map(name => [name, crypto.createHash('sha256').update(fs.readFileSync(path.join(input, name))).digest('hex')])),
            diagnostics, emitSkipped: emitted.emitSkipped, files: Object.keys(outputs).sort(),
            cliExit: cli.status, cliFiles,
        };
        if (!diagnostics.length) {
            assert.strictEqual(cli.status, 0, id + ': ' + cli.stdout + cli.stderr);
            const module = {exports: {}};
            vm.runInNewContext(outputs['main.js'], {module, exports: module.exports});
            reference.result = module.exports.result;
            assert.strictEqual(reference.result, 42, id);
            reference.declaration = outputs['main.d.ts'];
        } else {
            assert.notStrictEqual(cli.status, 0, id);
        }
        cases.push({id, flags, reference});
    }
    const observation = {typescript: ts.version, cases};
    const matrix = cases.map(item => item.id + '/main.ts\t' + (item.reference.diagnostics.length ? 'reject' : 'accept') + '\n').join('');
    if (process.env.BLUEICE_WRITE_OUTPUT_VALIDATION_MATRIX === '1') {
        fs.writeFileSync(path.join(corpus, 'reference.json'), JSON.stringify(observation, null, 2) + '\n');
        fs.writeFileSync(path.join(corpus, 'output-validation-checker-matrix.tsv'), matrix);
    } else {
        assert.deepStrictEqual(observation, JSON.parse(fs.readFileSync(path.join(corpus, 'reference.json'), 'utf8')));
        assert.strictEqual(fs.readFileSync(path.join(corpus, 'output-validation-checker-matrix.tsv'), 'utf8').replaceAll('\r\n', '\n'), matrix);
    }
    console.log(JSON.stringify({typescript: ts.version, cases: cases.length, accepts: cases.filter(item => !item.reference.diagnostics.length).length}));
} finally {
    fs.rmSync(root, {recursive: true, force: true});
}
