// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Collect complete immutable CLI observations; the separate policy verifier decides acceptance.
const fs = require('node:fs'), path = require('node:path'), cp = require('node:child_process'), assert = require('node:assert/strict'), crypto = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..'), support = path.join(repo, 'backend/bluets/tests/fixtures/oracle_support'), observerPath = path.join(__dirname, 'gc3_native_observer.cjs');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc'), acorn = require(path.join(support, 'load_acorn.cjs'))(process.env.BLUEICE_ACORN_ORACLE || 'acorn'), library = path.dirname(ts.getDefaultLibFilePath({}));
const { executionScript } = require('./gc3_protocol.cjs'), relativeFixturePath = require(path.join(support, 'fixture_paths.cjs'));
const hash = x => crypto.createHash('sha256').update(x).digest('hex'), fileHash = p => hash(fs.readFileSync(p)), toolHash = fileHash(__filename);
function relative(name) { assert.equal(typeof name, 'string'); assert(name && name.split('/').every(p => p && p !== '.' && p !== '..') && !path.isAbsolute(name) && !/[\\:]/.test(name), 'bounded registry path ' + name); return name; }
function same(a, b) { return require('node:util').isDeepStrictEqual(a, b); }
const syntaxProjection = rows => [...rows].sort((a, b) => a.file.localeCompare(b.file));
const preservedProjection = value => value ? { ...value, syntax: syntaxProjection(value.syntax) } : null;
function diagnosticProjection(d, nested = false) { return { code: d.code, ...!nested ? { category: d.category } : {}, file: d.file, start: d.start, length: d.length, message: d.message, related: (d.related || []).map(x => diagnosticProjection(x, true)) }; }
function run(options) {
    const compiler = path.resolve(options.compiler), registryPath = path.resolve(options.registry), nativePath = path.resolve(options.native), output = path.resolve(options.output), expectedCompiler = options.compilerSha256;
    assert.match(expectedCompiler, /^[a-f0-9]{64}$/);
    assert.equal(ts.version, '5.9.3');
    assert.equal(acorn.version, '8.15.0');
    const registryBytes = fs.readFileSync(registryPath), registry = JSON.parse(registryBytes), nativeBytes = fs.readFileSync(nativePath), native = JSON.parse(nativeBytes), registryHash = hash(registryBytes), nativeHash = hash(nativeBytes), observerHash = fileHash(observerPath);
    assert.equal(registry.schema, 1);
    assert.equal(native.registry.sha256, registryHash);
    assert.equal(native.recorderSha256, '86e93ee5e2a0403b3f632264ce3a9ad0e8b488f7b911d4b6197e966c092f953a');
    const policyPath = path.join(repo, 'backend/bluets/tests/fixtures/gc3_compositions/policy.json'), policyBytes = fs.readFileSync(policyPath), policy = JSON.parse(policyBytes);
    assert.equal(policy.registrySha256, registryHash);
    assert.equal(policy.nativeReferenceSha256, nativeHash);
    const protocolPath = require.resolve('./gc3_protocol.cjs'), protocolHash = fileHash(protocolPath);
    function guard() { assert.equal(fileHash(compiler), expectedCompiler, 'immutable CLI'); assert.equal(fileHash(registryPath), registryHash, 'unchanged registry'); assert.equal(fileHash(nativePath), nativeHash, 'unchanged Native reference'); assert.equal(fileHash(observerPath), observerHash, 'unchanged shared physical observer'); assert.equal(fileHash(__filename), toolHash, 'unchanged observer'); assert.equal(fileHash(protocolPath), protocolHash); assert.equal(hash(fs.readFileSync(policyPath)), hash(policyBytes)); }
    guard();
    if (!fs.existsSync(output))
        fs.mkdirSync(output, { recursive: true });
    assert.equal(fs.readdirSync(output).length, 0, 'fresh Blue output root');
    fs.writeFileSync(path.join(output, 'registry.json'), registryBytes);
    const startedAt = new Date().toISOString(), cases = [], progress = path.join(output, 'progress.jsonl');
    for (const expected of native.cases) {
        guard();
        if (expected.execution)
            assert.equal(executionScript(expected.execution.format === 'ESM'), expected.execution.command[2], 'unchanged frozen physical execution protocol');
        const form = registry.forms.find(f => f.id === expected.form);
        assert(form, 'original Native form');
        const configuration = form.configurations.find(c => c.id === expected.configuration);
        assert(configuration);
        assert.deepEqual(configuration.options, expected.options);
        assert.deepEqual(configuration.roots, expected.roots);
        assert.equal(configuration.emitMode, expected.emitMode);
        for (const [n, t] of Object.entries(form.files)) {
            relative(n);
            assert.equal(hash(t), expected.sources[n]);
        }
        const directory = path.join(output, 'cases', String(cases.length).padStart(3, '0')), logDirectory = path.join(output, 'logs', String(cases.length).padStart(3, '0'));
        fs.mkdirSync(directory, { recursive: true });
        fs.mkdirSync(logDirectory, { recursive: true });
        for (const [n, t] of Object.entries(form.files)) {
            const p = path.join(directory, n);
            fs.mkdirSync(path.dirname(p), { recursive: true });
            fs.writeFileSync(p, t);
        }
        const configPath = path.join(directory, 'tsconfig.json');
        fs.writeFileSync(configPath, JSON.stringify({ compilerOptions: configuration.options, files: configuration.roots }, null, 2) + '\n');
        const compilerEnvironment = { ...process.env };
        const scrubbedKeys = [];
        for (const k of Object.keys(compilerEnvironment))
            if (k.startsWith('BLUETS_') || k.startsWith('BLUEICE_GE5_') || k.startsWith('BLUEICE_WRITE_')) {
                delete compilerEnvironment[k];
                scrubbedKeys.push(k);
            }
        const argv = ['--project', configPath, '--diagnostics-json', '--listFiles'], compiled = cp.spawnSync(compiler, argv, { cwd: directory, env: compilerEnvironment, timeout: 30000, maxBuffer: 64 * 1024 * 1024 }), stdout = compiled.stdout || Buffer.alloc(0), stderr = compiled.stderr || Buffer.alloc(0);
        fs.writeFileSync(path.join(logDirectory, 'compiler.stdout.raw'), stdout);
        fs.writeFileSync(path.join(logDirectory, 'compiler.stderr.raw'), stderr);
        guard();
        const rawDiagnostics = [], unstructuredDiagnosticLines = [], originProblems = [], positionProblems = [], metadataUnavailable = [];
        for (const line of stderr.toString('utf8').split(/\r?\n/)) {
            if (!line.trim())
                continue;
            try {
                rawDiagnostics.push(JSON.parse(line));
            }
            catch {
                unstructuredDiagnosticLines.push(line);
            }
        }
        function sourceFor(module) { if (typeof module !== 'string')
            return null; if (Object.hasOwn(form.files, module))
            return { name: module, text: form.files[module] }; const name = relativeFixturePath(directory, module); if (name && Object.hasOwn(form.files, name))
            return { name, text: form.files[name] }; if (path.dirname(module) === library && Object.hasOwn(expected.libraryInputs, path.basename(module))) {
            const text = fs.readFileSync(module, 'utf8');
            if (hash(text) === expected.libraryInputs[path.basename(module)])
                return { name: '<typescript-lib>/' + path.basename(module), text };
        } return null; }
        function convert(t, severity, index, nested = false) {
            const span = t?.span, source = span && sourceFor(span.module);
            let name = span?.module ?? null, start = null, length = null;
            if (source && Number.isInteger(span.start) && Number.isInteger(span.end) && span.start >= 0 && span.end >= span.start) {
                const buffer = Buffer.from(source.text), prefix = buffer.subarray(0, span.start), part = buffer.subarray(span.start, span.end), decodedPrefix = prefix.toString('utf8'), decodedPart = part.toString('utf8');
                if (span.end <= buffer.length && Buffer.from(decodedPrefix).equals(prefix) && Buffer.from(decodedPart).equals(part)) {
                    name = source.name;
                    start = decodedPrefix.length;
                    length = decodedPart.length;
                    const f = ts.createSourceFile(name, source.text, ts.ScriptTarget.ES2022, true), p = f.getLineAndCharacterOfPosition(start), actualPosition = t.position;
                    if (actualPosition && (!same(actualPosition, { line: p.line + 1, column: p.character + 1, length })))
                        positionProblems.push({ index, span, reportedPosition: actualPosition, derivedPosition: { line: p.line + 1, column: p.character + 1, length } });
                }
                else
                    originProblems.push({ index, reason: 'invalid byte range or non-UTF8 boundary', span });
            }
            else
                originProblems.push({ index, reason: 'original physical source or exact pinned-library origin unavailable', span: span ?? null, reportedPosition: t?.position ?? null });
            let category = null;
            if (!nested)
                category = severity === 'error' ? 'Error' : severity === 'warning' ? 'Warning' : severity === 'suggestion' ? 'Suggestion' : 'Message';
            else if (t.category !== undefined)
                category = t.category;
            else
                metadataUnavailable.push({ index, field: 'related.category', reason: 'not exposed by existing Blue diagnostic JSON schema; not a new comparison gate' });
            return { code: t?.code ?? null, category, file: name, start, length, message: typeof t?.message === 'string' ? t.message.replaceAll(directory, '<root>') : null, related: (t?.relatedInformation || []).map((x, i) => convert(x, severity, index + '.related.' + i, true)) };
        }
        const diagnostics = rawDiagnostics.map((d, i) => { if (!d.typescript) {
            originProblems.push({ index: String(i), reason: 'TypeScript counterpart absent', btsCode: d.btsCode, noTypeScriptCounterpart: d.noTypeScriptCounterpart ?? null, span: d.span ?? null });
            return { code: null, category: d.severity === 'error' ? 'Error' : d.severity === 'warning' ? 'Warning' : 'Message', file: d.span?.module ?? null, start: null, length: null, message: d.rawMessage ?? null, related: [] };
        } return convert(d.typescript, d.severity, String(i)); });
        const selectedInputs = [...new Set(stdout.toString('utf8').split(/\r?\n/).filter(Boolean).map(p => relativeFixturePath(directory, p)).filter(p => p && Object.hasOwn(form.files, p)))].sort(), stdoutUnselectedLines = stdout.toString('utf8').split(/\r?\n/).filter(p => p && !selectedInputs.some(n => path.resolve(directory, n) === path.resolve(p)));
        const outputs = {}, authoredInputMutations = [];
        function scan(dir) { for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
            const p = path.join(dir, e.name);
            if (e.isDirectory())
                scan(p);
            else if (e.isFile()) {
                const n = path.relative(directory, p).replaceAll('\\', '/');
                if (n === 'tsconfig.json')
                    continue;
                const bytes = fs.readFileSync(p);
                if (Object.hasOwn(form.files, n)) {
                    if (hash(bytes) !== hash(form.files[n]))
                        authoredInputMutations.push({ file: n, beforeSha256: hash(form.files[n]), afterSha256: hash(bytes) });
                }
                else
                    outputs[n] = bytes.toString('utf8');
            }
        } }
        scan(directory);
        const syntax = [];
        for (const [n, text] of Object.entries(outputs)) {
            if (!/\.(?:jsx|mjs|cjs|js)$/.test(n))
                continue;
            const preserved = configuration.emitMode === 'preserved-jsx', f = ts.createSourceFile(n, text, ts.ScriptTarget.ES2022, true, preserved ? ts.ScriptKind.JSX : ts.ScriptKind.JS), parseDiagnostics = f.parseDiagnostics.map(d => ({ code: d.code, category: ts.DiagnosticCategory[d.category], file: n, start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'), related: [] }));
            let acornError = null;
            if (!preserved)
                try {
                    acorn.parse(text, { ecmaVersion: 2022, sourceType: ts.isExternalModule(f) ? 'module' : 'script' });
                }
                catch (e) {
                    acornError = String(e);
                }
            syntax.push({ file: n, preservedJSX: preserved, parseDiagnostics, acornError });
        }
        const parsed = ts.parseJsonSourceFileConfigFileContent(ts.readJsonConfigFile(configPath, ts.sys.readFile), ts.sys, directory, {}, configPath), entryNames = ts.getOutputFileNames(parsed, path.join(directory, expected.resolvedEntry), false).map(p => relativeFixturePath(directory, p)).filter(n => n && /\.(?:jsx|mjs|cjs|js)$/.test(n));
        assert(entryNames.length <= 1);
        const emittedEntry = entryNames.find(n => Object.hasOwn(outputs, n)) || null;
        let execution = null, preservedSyntax = null;
        if (configuration.emitMode === 'preserved-jsx')
            preservedSyntax = { executed: false, entry: emittedEntry, expectedExtension: configuration.options.jsx === 'preserve' ? '.jsx' : '.js', observedExtension: emittedEntry ? path.extname(emittedEntry) : null, syntax };
        else if (emittedEntry) {
            const entry = emittedEntry, filename = path.join(directory, entry), f = ts.createSourceFile(entry, outputs[entry], ts.ScriptTarget.ES2022, true, ts.ScriptKind.JS), esm = entry.endsWith('.mjs') || !entry.endsWith('.cjs') && ts.isExternalModule(f), script = executionScript(esm), env = { ...process.env };
            delete env.NODE_PATH;
            delete env.NODE_OPTIONS;
            const executed = cp.spawnSync(process.execPath, ['-e', script, filename], { cwd: directory, env, encoding: 'utf8', timeout: 15000, maxBuffer: 16 * 1024 * 1024 });
            fs.writeFileSync(path.join(logDirectory, 'execution.stdout.raw'), executed.stdout || '');
            fs.writeFileSync(path.join(logDirectory, 'execution.stderr.raw'), executed.stderr || '');
            execution = { entry, format: esm ? 'ESM' : 'CommonJS', command: [process.execPath, '-e', script, '<root>/' + entry], exit: executed.status, signal: executed.signal, error: executed.error ? String(executed.error) : null, stdout: (executed.stdout || '').replaceAll(directory, '<root>'), stderr: (executed.stderr || '').replaceAll(directory, '<root>'), physicalJavaScriptSha256: fileHash(filename) };
        }
        const panic = /panicked at|thread '[^']+' panicked|fatal runtime error/i.test(stderr.toString('utf8')), verdict = compiled.signal || compiled.error || panic ? 'crash' : compiled.status === 0 ? 'accept' : 'reject', files = Object.keys(outputs).sort(), rawDeclarationDifferences = [];
        for (const n of [...new Set([...expected.files, ...files])].sort())
            if (/\.d\.[cm]?ts$/.test(n) && outputs[n] !== expected.outputs[n])
                rawDeclarationDifferences.push({ file: n, nativeSha256: expected.outputs[n] === undefined ? null : hash(expected.outputs[n]), blueSha256: outputs[n] === undefined ? null : hash(outputs[n]) });
        const outcome = e => e ? { exit: e.exit, signal: e.signal, error: e.error, stdout: e.stdout, stderr: e.stderr } : null, comparison = { verdictMatch: verdict === expected.verdict, declaredDiagnosticFieldsMatch: same(diagnostics.map(d => diagnosticProjection(d)), expected.diagnostics.map(d => diagnosticProjection(d))), selectedUserInputsMatch: same(selectedInputs, expected.selectedInputs), artifactKeysMatch: same(files, expected.files), rawDeclarationsMatch: rawDeclarationDifferences.length === 0, syntaxMatch: same(syntaxProjection(syntax), syntaxProjection(expected.syntax)), physicalOutcomeMatch: same(outcome(execution), outcome(expected.execution)), preservedSyntaxMatch: same(preservedProjection(preservedSyntax), preservedProjection(expected.preservedSyntax)) };
        const differences = [];
        for (const [k, v] of Object.entries(comparison))
            if (!v)
                differences.push(k);
        if (panic)
            differences.push('compiler-panic');
        if (compiled.signal)
            differences.push('compiler-signal');
        if (compiled.error)
            differences.push('compiler-capture-error');
        if (unstructuredDiagnosticLines.length)
            differences.push('unstructured-compiler-stderr');
        if (originProblems.length)
            differences.push('diagnostic-origin-unavailable');
        if (positionProblems.length)
            differences.push('reported-position-inconsistent');
        if (authoredInputMutations.length)
            differences.push('authored-input-mutated');
        if (rawDiagnostics.some(d => /unsupported|not supported/i.test(String(d.rawMessage) + ' ' + JSON.stringify(d.noTypeScriptCounterpart))))
            differences.push('unsupported-diagnostic');
        const row = { id: expected.id, form: expected.form, family: expected.family, configuration: expected.configuration, identity: expected.identity, options: configuration.options, roots: configuration.roots, resolvedEntry: expected.resolvedEntry, emitMode: configuration.emitMode, sources: expected.sources, verdict, compiler: { path: compiler, argv: [...argv.slice(0, 1), '<root>/tsconfig.json', ...argv.slice(2)], exit: compiled.status, signal: compiled.signal, error: compiled.error ? String(compiled.error) : null, stdoutSha256: hash(stdout), stderrSha256: hash(stderr), stdoutRaw: path.relative(output, path.join(logDirectory, 'compiler.stdout.raw')), stderrRaw: path.relative(output, path.join(logDirectory, 'compiler.stderr.raw')), scrubbedEnvironmentKeys: scrubbedKeys.sort() }, rawDiagnostics, unstructuredDiagnosticLines, diagnostics, originProblems, positionProblems, metadataUnavailable, selectedInputs, stdoutUnselectedLines, files, outputs, outputSha256: Object.fromEntries(Object.entries(outputs).map(([n, t]) => [n, hash(t)])), authoredInputMutations, syntax, execution, preservedSyntax, comparison, rawDeclarationDifferences, differences, native: { id: expected.id, verdict: expected.verdict, diagnostics: expected.diagnostics, selectedInputs: expected.selectedInputs, files: expected.files, execution: outcome(expected.execution) }, directory: path.relative(output, directory) };
        cases.push(row);
        fs.writeFileSync(path.join(logDirectory, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
        fs.appendFileSync(progress, JSON.stringify({ id: row.id, verdict, compilerExit: compiled.status, differences, artifacts: files.length, executionExit: execution?.exit, metadataUnavailable: metadataUnavailable.length }) + '\n');
        console.log('OBSERVED ' + cases.length + '/' + native.cases.length + ' ' + row.id + ' ' + verdict + ' ' + differences.join(','));
    }
    guard();
    const clusters = {};
    for (const c of cases)
        for (const name of c.differences)
            (clusters[name] ??= []).push(c.id);
    const report = { schema: 1, scope: 'Read-only immutable BlueTS baseline against the same G-C3 Native-v4 registry; no implementation or parity acceptance', startedAt, finishedAt: new Date().toISOString(), registry: { path: registryPath, sha256: registryHash }, native: { path: nativePath, sha256: nativeHash }, physicalObserver: { path: observerPath, sha256: observerHash }, compiler: { path: compiler, sha256: expectedCompiler }, baselineHarnessSha256: toolHash, configurations: cases.length, counts: { accept: cases.filter(c => c.verdict === 'accept').length, reject: cases.filter(c => c.verdict === 'reject').length, crash: cases.filter(c => c.verdict === 'crash').length, casesWithDifferences: cases.filter(c => c.differences.length).length, nodeExecutions: cases.filter(c => c.execution).length, runtimeFailures: cases.filter(c => c.execution && (c.execution.exit !== 0 || c.execution.error)).length, preservedModeConfigurations: cases.filter(c => c.emitMode === 'preserved-jsx').length, preservedEmittedEntries: cases.filter(c => c.preservedSyntax?.entry).length, metadataUnavailable: cases.reduce((s, c) => s + c.metadataUnavailable.length, 0) }, comparisonBoundary: 'Existing K.10 code/file/UTF16 start/length/message/all recursive related; primary category follows actual severity. Missing Blue related category is recorded separately, not a new gate.', clusters, cases, fullGC3Accepted: false };
    fs.writeFileSync(path.join(output, 'baseline.json'), JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify({ conclusion: 'complete readonly baseline recorded; differences preserved', configurations: cases.length, ...report.counts, clusters: Object.fromEntries(Object.entries(clusters).map(([k, v]) => [k, v.length])) }));
    return report;
}
module.exports = { run };
if (require.main === module) {
    try {
        const names = { '--registry': 'registry', '--native': 'native', '--compiler': 'compiler', '--compiler-sha256': 'compilerSha256', '--output-dir': 'output' }, options = {};
        for (let i = 2; i < process.argv.length; i += 2) {
            assert(names[process.argv[i]] && process.argv[i + 1] && !options[names[process.argv[i]]]);
            options[names[process.argv[i]]] = process.argv[i + 1];
        }
        for (const k of Object.values(names))
            assert(options[k], 'explicit ' + k);
        run(options);
    }
    catch (error) {
        console.error(error.stack);
        process.exitCode = 1;
    }
}
