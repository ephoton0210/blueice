// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Native-only frozen observations. Registry files are the entire authored input.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict'), crypto = require('node:crypto'), cp = require('node:child_process');
const repo = path.resolve(__dirname, '../../../..'), support = path.join(repo, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(support + '/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(support + '/load_acorn.cjs')(process.env.BLUEICE_ACORN_ORACLE || 'acorn');
const recorderBytes = fs.readFileSync(__filename);
const relativeFixturePath = require(support + '/fixture_paths.cjs'), hash = x => crypto.createHash('sha256').update(x).digest('hex'), stable = x => JSON.stringify(x, (_, v) => v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => a.localeCompare(b))) : v);
const { executionScript } = require('./gc3_protocol.cjs');
function relative(name) { assert.equal(typeof name, 'string'); assert(name && name.split('/').every(p => p && p !== '.' && p !== '..') && !path.isAbsolute(name) && !/[\\:]/.test(name), 'bounded relative registry path ' + name); return name; }
function record(registryPath, output) {
    assert.equal(ts.version, '5.9.3');
    assert.equal(acorn.version, '8.15.0');
    registryPath = path.resolve(registryPath);
    output = path.resolve(output);
    const bytes = fs.readFileSync(registryPath), registry = JSON.parse(bytes);
    assert.equal(registry.schema, 1);
    assert.equal(typeof registry.scope, 'string');
    assert(Array.isArray(registry.forms) && registry.forms.length);
    if (!fs.existsSync(output))
        fs.mkdirSync(output, { recursive: true });
    assert.equal(fs.readdirSync(output).length, 0, 'fresh output directory');
    fs.writeFileSync(path.join(output, 'registry.json'), bytes);
    const startedAt = new Date().toISOString(), library = path.dirname(ts.getDefaultLibFilePath({})), libraryCache = new Map(), cases = [], duplicates = [], seen = new Map(), ids = new Set(), formIds = new Set(), progress = path.join(output, 'progress.jsonl');
    let listed = 0;
    for (const form of registry.forms) {
        assert(!formIds.has(form.id), 'unique form id');
        formIds.add(form.id);
        assert.equal(typeof form.family, 'string');
        relative(form.entry);
        assert(form.files && Object.keys(form.files).length);
        for (const [name, text] of Object.entries(form.files)) {
            relative(name);
            assert.equal(typeof text, 'string');
        }
        assert(Array.isArray(form.configurations) && form.configurations.length);
        for (const configuration of form.configurations) {
            listed++;
            const id = form.id + '/' + configuration.id;
            assert(!ids.has(id), 'unique case id');
            ids.add(id);
            assert(['node', 'preserved-jsx'].includes(configuration.emitMode));
            const options = configuration.options;
            for (const key of ['strict', 'target', 'module', 'moduleResolution', 'lib', 'declaration', 'noEmitOnError', 'rootDir', 'outDir'])
                assert(Object.hasOwn(options, key), 'explicit option ' + key);
            assert.equal(options.target, 'ES2022');
            assert.equal(typeof options.strict, 'boolean');
            assert.equal(typeof options.declaration, 'boolean');
            assert.equal(typeof options.noEmitOnError, 'boolean');
            assert(Array.isArray(options.lib) && options.lib.length);
            relative(options.outDir);
            if (options.rootDir !== '.')
                relative(options.rootDir);
            const roots = configuration.roots;
            assert(Array.isArray(roots) && roots.length);
            for (const name of roots) {
                relative(name);
                assert(Object.hasOwn(form.files, name), 'declared root file ' + name);
            }
            const candidates = roots.includes(form.entry) ? [form.entry] : roots.filter(name => name.endsWith('/' + form.entry));
            assert.equal(candidates.length, 1, 'one exact/suffix selected physical entry');
            const resolvedEntry = candidates[0];
            if (/\.tsx$/.test(resolvedEntry))
                assert(Object.hasOwn(options, 'jsx'), 'explicit JSX mode');
            if (['preserve', 'react-native'].includes(options.jsx))
                assert.equal(configuration.emitMode, 'preserved-jsx', 'preserved JSX is never sent to Node');
            const identity = hash(stable({ files: form.files, roots: [...roots].sort(), entry: resolvedEntry, options, emitMode: configuration.emitMode }));
            if (seen.has(identity)) {
                duplicates.push({ id, canonicalCase: seen.get(identity), identity });
                continue;
            }
            seen.set(identity, id);
            const directory = path.join(output, 'cases', String(cases.length).padStart(3, '0'));
            fs.mkdirSync(directory, { recursive: true });
            for (const [name, text] of Object.entries(form.files)) {
                const p = path.join(directory, name);
                fs.mkdirSync(path.dirname(p), { recursive: true });
                fs.writeFileSync(p, text);
            }
            const configPath = path.join(directory, 'tsconfig.json');
            fs.writeFileSync(configPath, JSON.stringify({ compilerOptions: options, files: roots }, null, 2) + '\n');
            const parsed = ts.parseJsonSourceFileConfigFileContent(ts.readJsonConfigFile(configPath, ts.sys.readFile), ts.sys, directory, {}, configPath), host = ts.createCompilerHost(parsed.options), original = host.getSourceFile, outputs = {}, writers = {};
            host.getSourceFile = (filename, ...args) => { if (path.dirname(filename) !== library)
                return original(filename, ...args); const key = filename + JSON.stringify(args[0]); if (!libraryCache.has(key))
                libraryCache.set(key, original(filename, ...args)); return libraryCache.get(key); };
            host.writeFile = (filename, text, bom, onError, sourceFiles) => { const name = relativeFixturePath(directory, filename); assert(name, 'Native output remains inside case'); relative(name); assert(!Object.hasOwn(form.files, name), 'Native artifact cannot overwrite authored support input'); outputs[name] = (bom ? '\ufeff' : '') + text; writers[name] = (sourceFiles || []).map(s => relativeFixturePath(directory, s.fileName)).filter(Boolean); fs.mkdirSync(path.dirname(filename), { recursive: true }); fs.writeFileSync(filename, outputs[name]); };
            const program = ts.createProgram(parsed.fileNames, parsed.options, host), all = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)], emitted = program.emit();
            for (const d of emitted.diagnostics)
                if (!all.some(old => old.code === d.code && old.start === d.start && old.file === d.file))
                    all.push(d);
            const diagnosticFile = filename => relativeFixturePath(directory, filename) || (path.dirname(filename) === library ? '<typescript-lib>/' + path.basename(filename) : filename);
            const convert = d => ({ code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file ? diagnosticFile(d.file.fileName) : null, start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, '\n').replaceAll(directory, '<root>'), related: (d.relatedInformation || []).map(convert) }), diagnostics = all.map(convert), selectedInputs = program.getSourceFiles().map(s => relativeFixturePath(directory, s.fileName)).filter(Boolean).sort(), libraryInputs = Object.fromEntries(program.getSourceFiles().filter(s => path.dirname(s.fileName) === library).map(s => [path.basename(s.fileName), hash(s.text)]));
            const syntax = [];
            for (const [name, text] of Object.entries(outputs)) {
                if (!/\.(?:js|mjs|cjs|jsx)$/.test(name))
                    continue;
                const preserved = configuration.emitMode === 'preserved-jsx', file = ts.createSourceFile(name, text, ts.ScriptTarget.ES2022, true, preserved ? ts.ScriptKind.JSX : ts.ScriptKind.JS);
                const errors = file.parseDiagnostics.map(convert);
                let acornError = null;
                if (!preserved)
                    try {
                        acorn.parse(text, { ecmaVersion: 2022, sourceType: ts.isExternalModule(file) ? 'module' : 'script' });
                    }
                    catch (e) {
                        acornError = String(e);
                    }
                syntax.push({ file: name, preservedJSX: preserved, parseDiagnostics: errors, acornError });
            }
            const emittedEntries = Object.keys(outputs).filter(name => /\.(?:js|mjs|cjs|jsx)$/.test(name) && writers[name].includes(resolvedEntry));
            if (!emittedEntries.length) {
                for (const filename of ts.getOutputFileNames(parsed, path.join(directory, resolvedEntry), false)) {
                    const name = relativeFixturePath(directory, filename);
                    if (Object.hasOwn(outputs, name) && /\.(?:js|mjs|cjs|jsx)$/.test(name))
                        emittedEntries.push(name);
                }
            }
            assert(emittedEntries.length <= 1, 'one physical emitted entry');
            let execution = null, preservedSyntax = null;
            if (configuration.emitMode === 'preserved-jsx') {
                preservedSyntax = { executed: false, entry: emittedEntries[0] || null, expectedExtension: options.jsx === 'preserve' ? '.jsx' : '.js', observedExtension: emittedEntries[0] ? path.extname(emittedEntries[0]) : null, syntax };
                if (emittedEntries.length)
                    assert.equal(preservedSyntax.observedExtension, preservedSyntax.expectedExtension);
            }
            else if (emittedEntries.length) {
                const entry = emittedEntries[0], filename = path.join(directory, entry), source = ts.createSourceFile(entry, outputs[entry], ts.ScriptTarget.ES2022, true, ts.ScriptKind.JS), esm = entry.endsWith('.mjs') || !entry.endsWith('.cjs') && ts.isExternalModule(source);
                const script = executionScript(esm);
                const env = { ...process.env };
                delete env.NODE_PATH;
                delete env.NODE_OPTIONS;
                const run = cp.spawnSync(process.execPath, ['-e', script, filename], { cwd: directory, env, encoding: 'utf8', timeout: 15000, maxBuffer: 16 * 1024 * 1024 });
                execution = { entry, format: esm ? 'ESM' : 'CommonJS', command: [process.execPath, '-e', script, '<root>/' + entry], exit: run.status, signal: run.signal, error: run.error ? String(run.error) : null, stdout: (run.stdout || '').replaceAll(directory, '<root>'), stderr: (run.stderr || '').replaceAll(directory, '<root>'), physicalJavaScriptSha256: hash(fs.readFileSync(filename)) };
            }
            const row = { id, form: form.id, family: form.family, configuration: configuration.id, identity, entry: form.entry, resolvedEntry, roots, options, emitMode: configuration.emitMode, sources: Object.fromEntries(Object.entries(form.files).map(([n, t]) => [n, hash(t)])), verdict: diagnostics.some(d => d.category === 'Error') ? 'reject' : 'accept', diagnostics, emitSkipped: emitted.emitSkipped, selectedInputs, libraryInputs, files: Object.keys(outputs).sort(), outputs, outputSha256: Object.fromEntries(Object.entries(outputs).map(([n, t]) => [n, hash(t)])), syntax, execution, preservedSyntax, directory: path.relative(output, directory) };
            cases.push(row);
            fs.writeFileSync(path.join(directory, 'native-observation.json'), JSON.stringify(row, null, 2) + '\n');
            fs.appendFileSync(progress, JSON.stringify({ id, verdict: row.verdict, diagnostics: diagnostics.length, artifacts: row.files.length, executionExit: execution?.exit, mode: configuration.emitMode }) + '\n');
            console.log('RECORDED ' + cases.length + ' ' + id + ' ' + row.verdict + ' runtime=' + (execution ? execution.exit : 'not-executed'));
        }
    }
    assert.equal(hash(fs.readFileSync(__filename)), hash(recorderBytes), 'recorder unchanged throughout actual collection');
    const report = { schema: 1, scope: registry.scope, authority: 'Native-only TypeScript 5.9.3; no BlueTS comparison or G-C3 completion', typescript: ts.version, acorn: acorn.version, node: process.version, startedAt, finishedAt: new Date().toISOString(), registry: { path: registryPath, sha256: hash(bytes) }, recorderSha256: hash(recorderBytes), listedConfigurations: listed, configurations: cases.length, duplicateConfigurations: duplicates, cases, counts: { accept: cases.filter(c => c.verdict === 'accept').length, reject: cases.filter(c => c.verdict === 'reject').length, nodeExecutions: cases.filter(c => c.execution).length, nativeRuntimeFailures: cases.filter(c => c.execution && (c.execution.exit !== 0 || c.execution.error)).length, preservedModeConfigurations: cases.filter(c => c.emitMode === 'preserved-jsx').length, preservedEmittedEntries: cases.filter(c => c.preservedSyntax?.entry).length, syntaxErrors: cases.reduce((s, c) => s + c.syntax.filter(x => x.parseDiagnostics.length || x.acornError).length, 0) }, fullGC3Accepted: false };
    fs.writeFileSync(path.join(output, 'reference.json'), JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify({ conclusion: 'Native observation collection complete; not parity acceptance', configurations: report.configurations, ...report.counts }));
    return report;
}
module.exports = { record, executionScript };
if (require.main === module) {
    try {
        const args = {};
        for (let i = 2; i < process.argv.length; i += 2) {
            assert(['--registry', '--output-dir'].includes(process.argv[i]) && process.argv[i + 1] && !args[process.argv[i]]);
            args[process.argv[i]] = process.argv[i + 1];
        }
        assert(args['--registry'] && args['--output-dir']);
        record(args['--registry'], args['--output-dir']);
    }
    catch (error) {
        console.error(error.stack);
        process.exitCode = 1;
    }
}
