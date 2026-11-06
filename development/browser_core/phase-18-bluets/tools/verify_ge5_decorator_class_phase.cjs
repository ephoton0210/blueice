// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Supplemental execution proves that an ES5 class-local binding precedes its outer assignment.
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const assert = require('node:assert/strict'), {spawnSync} = require('node:child_process');
const graph = require('./ge5_inputs.cjs'), observed = require('./ge2_observations.cjs'), maps = require('./ge5_maps.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const compile = require('./ge5_native_compile.cjs')(ts);
const prefix = "import { peek } from './observer.js';\nconst events: any[] = [];\nfunction external(label: string): string { events.push([label, typeof C, typeof peek()]); return label; }\nfunction member(target: any, name: string): void { events.push(['member', name, typeof C, typeof peek()]); }\nfunction replace(value: any): any { events.push(['class', typeof C, typeof peek(), value.field, value.block]); function Replacement(this: any) { this.tag = 'new'; } return Replacement; }\n";
const body = "export class C { tag: string = 'old'; static field: string = external('field'); static block: string = ''; static [external('key')](): number { return 3; } static { C.block = external('block'); } @member method(): number { return 1; } @member static other(): number { return 2; } }\nevents.push(['after', typeof C, typeof peek(), peek() === C, new C().tag]); export const result: any[] = events;\n";
const observer = "import { C } from './main.js';\nexport function peek(): any { return C; }\n";
const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge5-decorator-class-phase-')));
const compiler = path.join(root, 'target/debug/bluetsc'), compilerSha256 = graph.sha256(fs.readFileSync(compiler));
const rows = [], failures = [], probe = process.argv.includes('--probe');
function outputs(base) {
  const result = {};
  function visit(directory) { for (const name of fs.readdirSync(directory).sort()) { const file = path.join(directory, name);
    if (fs.statSync(file).isDirectory()) visit(file); else result[path.relative(base, file).replaceAll(path.sep, '/')] = fs.readFileSync(file, 'utf8'); } }
  visit(path.join(base, 'out')); return result;
}
function cli(base) {
  const result = spawnSync(compiler, ['--project', path.join(base, 'tsconfig.json'), '--diagnostics-json'], {encoding: 'utf8'});
  assert.equal(result.status, 0, result.stdout + result.stderr); assert.equal(result.stderr, ''); assert.equal(result.stdout, ''); return outputs(base);
}
function phaseControls(row, outputs, expected, decorated) {
  const code = outputs['out/main.js'], source = ts.createSourceFile('main.js', code, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const factories = [];
  function visit(node) {
    if (ts.isFunctionExpression(node) && node.body.statements.some(statement => ts.isFunctionDeclaration(statement) && statement.name?.text === 'C')
      && node.body.statements.some(statement => ts.isReturnStatement(statement) && statement.expression?.getText(source) === 'C')) factories.push(node);
    ts.forEachChild(node, visit);
  }
  visit(source); assert.equal(factories.length, 1, 'the source class has one exact ES5 factory');
  const factory = factories[0], phases = factory.body.statements.filter(statement => {
    if (!ts.isExpressionStatement(statement)) return false;
    let observed = false;
    function calls(node) { if (ts.isCallExpression(node) && ['external', '__bluetsDecorate'].includes(node.expression.getText(source))) observed = true; ts.forEachChild(node, calls); }
    calls(statement); return observed;
  });
  assert.equal(phases.length, decorated ? 6 : 5, 'computed key/static initializer/block/member/class complete observable phase inventory');
  let outer = factory;
  while (!ts.isVariableStatement(outer) && !ts.isExpressionStatement(outer)) { outer = outer.parent; assert(outer, 'factory belongs to a retained outer binding statement'); }
  return phases.map((statement, index) => {
    const start = statement.getStart(source), end = statement.end, text = code.slice(start, end);
    const changedCode = code.slice(0, start) + text.replace(/[^\r\n]/g, ' ') + code.slice(end, outer.end) + '\n' + text + '\n' + code.slice(outer.end);
    const parsed = ts.createSourceFile('main.js', changedCode, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
    assert.deepEqual(parsed.parseDiagnostics.map(diagnostic => diagnostic.code), [], 'phase control remains valid JavaScript');
    const changed = observed.execute({}, row, {...outputs, 'out/main.js': changedCode});
    assert.notDeepEqual(changed, expected, 'moving phase ' + index + ' beyond the outer binding must change observation');
    return {index, label: 'move exact initialization phase after outer assignment', rejected: true, text};
  });
}
try {
  for (const decorated of probe ? [true] : [false, true]) for (const define of probe ? [false] : [false, true])
    for (const module of probe ? ['CommonJS'] : ['CommonJS', 'AMD', 'UMD', 'System'])
      for (const variant of probe ? ['original'] : ['original', 'crlfAstral']) for (const sourceMap of probe ? [true] : [false, true]) {
    const id = [decorated ? 'class' : 'member', define ? 'define' : 'assign', module, variant, sourceMap ? 'maps' : 'noMaps'].join('/');
    const base = path.join(directory, String(rows.length + failures.length));
    const row = {form: 'legacy-class-initialization-phase', entry: 'main.ts', runtimeEntry: 'out/main.js', runtimeModule: module,
      files: {'main.ts': graph.coordinate(prefix + (decorated ? '@replace ' : '') + body, variant), 'observer.ts': graph.coordinate(observer, variant), 'package.json': '{"type":"commonjs"}\n'},
      options: {target: 'ES5', module, moduleResolution: 'Node10', lib: ['ES2022'], types: [], strict: true,
        experimentalDecorators: true, useDefineForClassFields: define, declaration: true, declarationMap: sourceMap,
        sourceMap, inlineSources: sourceMap, noEmitOnError: true, rootDir: '.', outDir: 'out'}};
    fs.mkdirSync(base); graph.write(base, row.files);
    try {
      const native = compile(base, row); assert.deepEqual(native.diagnostics, [], id + ': Native acceptance');
      const actual = cli(base); assert.deepEqual(cli(base), actual, 'two public CLI builds');
      assert.deepEqual(Object.keys(actual).sort(), Object.keys(native.outputs).sort(), 'complete artifact inventory');
      for (const [name, value] of Object.entries(native.outputs)) {
        if (name.endsWith('.d.ts')) assert.equal(actual[name], value, 'raw declaration bytes');
        if (name.endsWith('.d.ts.map')) assert.deepEqual(maps.decode(actual[name]), maps.decode(value), 'complete decoded declaration map');
      }
      const expected = observed.execute({}, row, native.outputs), result = observed.execute({}, row, actual);
      assert.equal(expected.configured.exit, 0, 'Native actual completion');
      if (probe) { fs.writeFileSync('/private/tmp/ge5-decorator-class-phase-native.js', native.outputs['out/main.js']); fs.writeFileSync('/private/tmp/ge5-decorator-class-phase-blue.js', actual['out/main.js']); console.log(JSON.stringify({id, native: expected, blue: result})); }
      assert.deepEqual(result, expected, 'class-local initialization precedes the outer binding and module publication');
      const controls = phaseControls(row, actual, expected, decorated);
      const hashes = outputs => Object.fromEntries(Object.entries(outputs).map(([name, text]) => [name, graph.sha256(text)]));
      rows.push({id, repeatBuilds: 2, options: row.options, inputs: row.files, diagnostics: native.diagnostics,
        nativeArtifacts: hashes(native.outputs), blueArtifacts: hashes(actual), native: expected, blue: result, controls});
    } catch (error) { failures.push({id, error: error.message, stack: error.stack}); }
  }
  const report = {compilerSha256, scope: 'supplemental complete ES5 legacy class initialization phase execution; frozen inputs unchanged', rows, failures};
  if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) { fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true}); fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'decorator-class-phase.json'), JSON.stringify(report, null, 2) + '\n'); }
  console.log(JSON.stringify({configurations: rows.length + failures.length, passed: rows.length, controls: rows.reduce((sum, row) => sum + row.controls.length, 0), failures}));
  if (failures.length) process.exitCode = 1;
} finally { fs.rmSync(directory, {recursive: true, force: true}); }
