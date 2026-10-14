// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// K.3.2: configuration, native CLI and option-combination primary positions.
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawnSync } = require('child_process');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const ts = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'))(
    process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-position-oracle-'));
const cases = [];
const mismatches = [];
const libraryRoot = path.dirname(ts.getDefaultLibFilePath({}));
const libraries = new Map();
const slash = value => value.replaceAll('\\', '/');
function serialize(d, directory) {
    const result = { code: d.code, position: null };
    if (d.file && d.start !== undefined) {
        const p = d.file.getLineAndCharacterOfPosition(d.start);
        result.file = slash(path.relative(directory, d.file.fileName));
        result.position = { line: p.line + 1, column: p.character + 1, length: d.length || 0 };
    }
    return result;
}
function config(directory, args, show) {
    const command = ts.parseCommandLine(args);
    let errors = command.errors;
    if (errors.length) return errors;
    if (command.options.project && command.fileNames.length) {
        return commandError(directory, args);
    }
    let filename = command.options.project || directory;
    if (!path.isAbsolute(filename)) filename = path.join(directory, filename);
    if (fs.existsSync(filename) && fs.statSync(filename).isDirectory()) filename = path.join(filename, 'tsconfig.json');
    if (!fs.existsSync(filename)) return commandError(directory, args);
    const parsed = ts.getParsedCommandLineOfConfigFile(filename, command.options,
        { ...ts.sys, onUnRecoverableConfigFileDiagnostic: d => errors.push(d) });
    if (!parsed) return errors;
    errors = [...errors, ...parsed.errors];
    if (show || errors.length) return errors;
    const options = { ...parsed.options, noEmit: true };
    const host = ts.createCompilerHost(options);
    const read = host.getSourceFile;
    host.getSourceFile = (name, language, ...rest) => {
        if (path.dirname(name) !== libraryRoot) return read(name, language, ...rest);
        const key = name + ':' + JSON.stringify(language);
        if (!libraries.has(key)) libraries.set(key, read(name, language, ...rest));
        return libraries.get(key);
    };
    return ts.getPreEmitDiagnostics(ts.createProgram({ rootNames: parsed.fileNames, options, host }));
}
function commandError(directory, args) {
    const output = spawnSync(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc', [...args, '--pretty', 'false'],
        { cwd: directory, encoding: 'utf8', env: { ...process.env, FORCE_COLOR: '0' } });
    const text = output.stdout + output.stderr;
    const codes = [...text.matchAll(/^error TS(\d+): (.*)$/gm)];
    if (!output.status || !codes.length) throw new Error(`Missing command diagnostic: ${text}`);
    return codes.map(match => ({ code: Number(match[1]), category: ts.DiagnosticCategory.Error, messageText: match[2] }));
}
function record(id, directory, args, metadata, accepts) {
    const diagnostics = ts.sortAndDeduplicateDiagnostics(config(directory, args, args.includes('--showConfig')));
    if ((diagnostics.length === 0) !== accepts) throw new Error(`${id}: verdict changed`);
    const first = diagnostics[0] ? serialize(diagnostics[0], directory) : null;
    cases.push({ id, ...metadata, args, accepts, first });
    if (process.env.BLUEICE_PROJECT_POSITION_BINARY) {
        const replay = [...args];
        if (accepts && !args.includes('--showConfig')) replay.push('--noEmit');
        replay.push('--diagnostics-json');
        const output = spawnSync(process.env.BLUEICE_PROJECT_POSITION_BINARY, replay, { cwd: directory, encoding: 'utf8' });
        let actual;
        try { actual = output.stderr.trim().split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line)); }
        catch { mismatches.push({ id, expected: first, actual: 'unstructured diagnostic' }); return; }
        const primary = actual[0]?.typescript || null;
        const same = first ? primary && primary.code === first.code
            && (first.position === null ? primary.position === null : ['line', 'column', 'length'].every(key => primary.position?.[key] === first.position[key]))
            && (!first.file || primary.span.module === first.file)
            : output.status === 0 && actual.length === 0;
        if (!same) mismatches.push({ id, expected: first, actual: primary });
    }
}
try {
    const coordinateCases = [];
    for (const source of ['const label = "🧊"; absent;', '// 🧊\r\nconst label = "🧊"; absent;', '// 🧊\rconst label = "🧊"; absent;', '// 🧊\nconst label = "🧊"; absent;']) {
        const directory = path.join(temporary, 'coordinates');
        fs.mkdirSync(directory, { recursive: true });
        fs.writeFileSync(path.join(directory, 'main.ts'), source);
        fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify({ files: ['main.ts'], compilerOptions: { strict: true, target: 'es2022' } }));
        const diagnostics = config(directory, ['--project', '.', '--noEmit'], false);
        if (diagnostics.length !== 1 || diagnostics[0].code !== 2304) throw new Error('Coordinate control changed');
        coordinateCases.push({ source, first: serialize(diagnostics[0], directory) });
    }
    const configs = path.join(fixtures, 'project_config');
    for (const line of fs.readFileSync(path.join(configs, 'config-checker-matrix.tsv'), 'utf8').trim().split(/\r?\n/)) {
        const [entry, verdict] = line.split('\t');
        record(`config:${entry}`, path.join(configs, entry), ['--project', '.', '--showConfig'],
            { matrix: 'config', entry }, verdict === 'accept');
    }
    const cli = JSON.parse(fs.readFileSync(path.join(fixtures, 'cli_surface/reference.json'), 'utf8'));
    for (const row of cli) {
        const directory = path.join(temporary, row.name);
        fs.cpSync(path.join(fixtures, 'cli_surface/project'), directory, { recursive: true });
        const filename = path.join(directory, 'tsconfig.json');
        const content = JSON.parse(fs.readFileSync(filename, 'utf8'));
        if (row.variant === 'error') fs.writeFileSync(path.join(directory, 'src/value.ts'), 'export const value: number = "wrong";\n');
        if (row.variant === 'config-noemit') content.compilerOptions.noEmit = true;
        if (row.variant === 'config-listfiles') content.compilerOptions.listFiles = true;
        if (row.variant === 'selectors') { content.include = ['src/**/*.ts']; content.exclude = ['src/ignore*']; }
        if (row.variant === 'in-place') delete content.compilerOptions.outDir;
        fs.writeFileSync(filename, JSON.stringify(content, null, 2));
        // A nested invocation uses the same nearest discovered configuration.
        record(`cli:${row.name}`, directory, row.args, { matrix: 'cli', variant: row.variant || null }, row.exit === 0);
    }
    const rust = fs.readFileSync(path.join(root, 'backend/bluets/tests/option_combinations_oracle.rs'), 'utf8');
    const sources = {};
    for (const [constant, filename] of [['LIB', 'lib.ts'], ['MAIN', 'main.ts']]) {
        sources[`src/${filename}`] = rust.match(new RegExp(`const ${constant}: &str = r#"([\\s\\S]*?)"#;`))[1];
    }
    const directory = path.join(temporary, 'options');
    fs.mkdirSync(path.join(directory, 'src'), { recursive: true });
    for (const [filename, text] of Object.entries(sources)) fs.writeFileSync(path.join(directory, filename), text);
    const combinations = JSON.parse(fs.readFileSync(path.join(fixtures, 'option_combinations/cases.json'), 'utf8'));
    for (const row of combinations) {
        fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify({ compilerOptions: row.options, files: ['src/main.ts'] }));
        record(`options:${row.name}`, directory, ['--project', '.', '--noEmit'], { matrix: 'options', options: row.options }, true);
    }
    const output = JSON.stringify({ version: ts.version, coordinateCases, optionSources: sources, cases }, null, 2) + '\n';
    const destination = path.join(fixtures, 'diagnostics/project-positions.json');
    if (process.env.BLUEICE_WRITE_PROJECT_POSITIONS === '1') fs.writeFileSync(destination, output);
    else if (fs.readFileSync(destination, 'utf8').replaceAll('\r\n', '\n') !== output) throw new Error('Project position evidence changed');
    process.stdout.write(JSON.stringify({ cases: cases.length, primaries: cases.filter(c => c.first).length }) + '\n');
    if (process.env.BLUEICE_PROJECT_POSITION_STATUS_PATH) {
        if (!process.env.BLUEICE_PROJECT_POSITION_BINARY) throw new Error('A replay binary is required for position status');
        fs.writeFileSync(process.env.BLUEICE_PROJECT_POSITION_STATUS_PATH, JSON.stringify({ version: ts.version, cases: cases.length, mismatches }, null, 2) + '\n');
        process.stdout.write(`Project position mismatches: ${mismatches.length}\n`);
    }
} finally {
    fs.rmSync(temporary, { recursive: true, force: true });
}
