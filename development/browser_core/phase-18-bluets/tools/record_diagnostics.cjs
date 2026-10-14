// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Record diagnostic evidence from the pinned compiler; never infer TS codes
// from BlueTSC's verdict or wording. No fixture sources or outputs are changed.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const load = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const corpus = path.join(fixtures, 'typescript_oracle');
const destination = path.join(fixtures, 'diagnostics');
const slash = value => value.split(path.sep).join('/');
const libraryRoot = path.dirname(ts.getDefaultLibFilePath({}));
const relative = value => path.dirname(value) === libraryRoot
    ? `<typescript-lib>/${path.basename(value)}` : slash(path.relative(fixtures, value));
const normalizeMessage = value => value
    .replaceAll(`${slash(fixtures)}/`, '<fixtures>/')
    .replaceAll(`${fixtures}${path.sep}`, '<fixtures>/');

function serialize(diagnostic) {
    const result = {
        code: diagnostic.code,
        category: ts.DiagnosticCategory[diagnostic.category].toLowerCase(),
        message: normalizeMessage(ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n')),
    };
    if (diagnostic.file && diagnostic.start !== undefined) {
        const start = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start);
        result.file = relative(diagnostic.file.fileName);
        result.start = diagnostic.start;
        result.length = diagnostic.length || 0;
        result.line = start.line + 1;
        result.column = start.character + 1;
    }
    if (diagnostic.relatedInformation) {
        result.related = diagnostic.relatedInformation.map(serialize);
    }
    return result;
}

function commandOptions(matrix, entry, target) {
    const commonjs = entry.startsWith('immutable-require-')
        || entry.startsWith('imported-type-cjs-') || entry.startsWith('import-decl-cjs/');
    const flags = [
        '--target', target || 'ES2022', '--module', commonjs ? 'commonjs' : 'ES2022', '--strict',
        '--allowImportingTsExtensions', '--noEmit',
    ];
    if (matrix === 'lib' || matrix === 'infer-return') flags.push('--lib', target);
    if (matrix === 'legacy-decorators') flags.push('--experimentalDecorators');
    if (entry.startsWith('imported-type-cjs-interop-')) flags.push('--esModuleInterop');
    if (matrix === 'class-modifiers' || matrix === 'class-dynamic' || matrix === 'class-retirement' || matrix === 'module-exports' || matrix === 'module-types' || matrix === 'ambient' || matrix === 'targets') {
        const filename = path.join(corpus, path.dirname(entry), 'flags.txt');
        if (fs.existsSync(filename)) flags.push(...fs.readFileSync(filename, 'utf8').trim().split(/\s+/));
    }
    if (matrix === 'jsx') {
        const filename = path.join(corpus, path.dirname(entry), 'flags.txt');
        flags.push(...(fs.existsSync(filename)
            ? fs.readFileSync(filename, 'utf8').trim().split(/\s+/)
            : ['--jsx', 'preserve']));
    }
    const parsed = ts.parseCommandLine(flags);
    if (parsed.errors.length) throw new Error(JSON.stringify(parsed.errors.map(serialize)));
    return { flags, options: parsed.options };
}

function recordCorpus() {
    const cases = [];
    const templates = new Map();
    const dictionary = new Map(Object.values(ts.Diagnostics).map(item => [item.code, item.message]));
    const libraryFiles = new Map();
    function record(id, names, options, metadata, configurationErrors = []) {
        // Reuse only TypeScript's parsed library files. Each isolated fixture
        // still receives its own complete program and options.
        const host = ts.createCompilerHost(options);
        const readSource = host.getSourceFile;
        host.getSourceFile = (filename, languageVersion, onError, createNew) => {
            if (path.dirname(filename) !== libraryRoot) {
                return readSource(filename, languageVersion, onError, createNew);
            }
            const key = `${filename}:${typeof languageVersion === 'object' ? JSON.stringify(languageVersion) : languageVersion}`;
            if (!libraryFiles.has(key)) {
                libraryFiles.set(key, readSource(filename, languageVersion, onError, createNew));
            }
            return libraryFiles.get(key);
        };
        const program = ts.createProgram({ rootNames: names, options, host });
        const diagnostics = ts.sortAndDeduplicateDiagnostics([
            ...configurationErrors, ...ts.getPreEmitDiagnostics(program),
        ]).map(serialize);
        for (const diagnostic of diagnostics) {
            if (!dictionary.has(diagnostic.code)) throw new Error(`Missing template TS${diagnostic.code}`);
            templates.set(diagnostic.code, dictionary.get(diagnostic.code));
        }
        const accepts = !diagnostics.some(item => item.category === 'error');
        if (metadata.verdict && accepts !== (metadata.verdict === 'accept')) {
            throw new Error(`${id}: recorded ${metadata.verdict}, observed ${JSON.stringify(diagnostics)}`);
        }
        cases.push({ id, ...metadata, inputs: names.map(relative), accepts, first: diagnostics[0] || null, diagnostics });
        if (cases.length % 100 === 0) process.stderr.write(`Recorded ${cases.length} programs\n`);
    }
    for (const filename of fs.readdirSync(corpus).filter(name => name.endsWith('-checker-matrix.tsv')).sort()) {
        const matrix = filename.replace('-checker-matrix.tsv', '');
        const settingsPath = path.join(corpus, `${matrix}-checker-settings.json`);
        const settings = fs.existsSync(settingsPath)
            ? JSON.parse(fs.readFileSync(settingsPath, 'utf8')) : {};
        if (settings.replayChecking !== undefined && settings.replayChecking !== 'strict') {
            throw new Error(`Unsupported replay checking mode for ${matrix}`);
        }
        for (const line of fs.readFileSync(path.join(corpus, filename), 'utf8').trim().split(/\r?\n/)) {
            const fields = line.split('\t');
            const entry = fields[0];
            const target = fields.length === 3 ? fields[1] : undefined;
            const verdict = fields[fields.length - 1];
            const { flags, options } = commandOptions(matrix, entry, target);
            const programSettings = settings.cases && settings.cases[path.dirname(entry)];
            const names = programSettings
                ? programSettings.files.map(file => path.join(corpus, path.dirname(entry), file))
                : [path.join(corpus, entry)];
            const metadata = { matrix, entry, flags, verdict, ...settings };
            delete metadata.cases;
            if (programSettings) {
                metadata.fixtureInputs = programSettings.sources.map(file =>
                    relative(path.join(corpus, path.dirname(entry), file)));
            }
            record(`${matrix}:${entry}`, names, options, metadata);
        }
    }
    const strictness = path.join(fixtures, 'strictness');
    for (const line of fs.readFileSync(path.join(strictness, 'strictness-checker-matrix.tsv'), 'utf8').trim().split(/\r?\n/)) {
        const [entry, verdict] = line.split('\t');
        const configPath = path.join(strictness, entry, 'tsconfig.json');
        const read = ts.readConfigFile(configPath, ts.sys.readFile);
        const parsed = ts.parseJsonConfigFileContent(read.config, ts.sys, path.dirname(configPath));
        const options = { ...parsed.options, noEmit: true };
        record(`strictness:${entry}`, parsed.fileNames, options, {
            matrix: 'strictness', entry, config: relative(configPath), verdict,
        }, [...(read.error ? [read.error] : []), ...parsed.errors]);
    }
    return {
        version: ts.version,
        coordinateUnit: 'UTF-16; one-based line/column, zero-based start',
        templates: [...templates].sort((a, b) => a[0] - b[0]).map(([code, message]) => ({ code, message })),
        cases,
    };
}

const record = recordCorpus();
const json = `${JSON.stringify(record, null, 2)}\n`;
if (process.env.BLUEICE_DIAGNOSTICS_ACTUAL_PATH) {
    fs.writeFileSync(process.env.BLUEICE_DIAGNOSTICS_ACTUAL_PATH, json);
}
const matrix = record.cases.map(item => `${item.id}\t${item.accepts ? 'accept' : 'reject'}\n`).join('');
if (process.env.BLUEICE_WRITE_DIAGNOSTICS_MATRIX === '1') {
    fs.mkdirSync(destination, { recursive: true });
    fs.writeFileSync(path.join(destination, 'diagnostics-checker-matrix.tsv'), matrix);
    fs.writeFileSync(path.join(destination, 'reference.json'), json);
} else {
    for (const [filename, expected] of [['diagnostics-checker-matrix.tsv', matrix], ['reference.json', json]]) {
        const recorded = fs.readFileSync(path.join(destination, filename), 'utf8').replace(/\r\n/g, '\n');
        if (recorded !== expected) throw new Error(`${filename} differs from pinned TypeScript ${ts.version}`);
    }
}
process.stdout.write(JSON.stringify({ programs: record.cases.length, templates: record.templates.length,
    accepted: record.cases.filter(item => item.accepts).length, rejected: record.cases.filter(item => !item.accepts).length }) + '\n');
