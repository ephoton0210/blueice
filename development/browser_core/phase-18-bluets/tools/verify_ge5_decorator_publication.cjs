// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Supplemental cycle observations do not replace the immutable map cohort.
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const assert = require('node:assert/strict'), {spawnSync} = require('node:child_process');
const graph = require('./ge5_inputs.cjs'), observed = require('./ge2_observations.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const compile = require('./ge5_native_compile.cjs')(ts), maps = require('./ge5_maps.cjs');
const source = "import { peek } from './observer.js';\nconst events: any[] = [];\nfunction member(target: any, name: string): void { events.push(['member', typeof peek()]); }\nfunction replace(value: any): any { events.push(['before', peek() === value, typeof peek()]); function Replacement(this: any) { this.tag = 'new'; } return Replacement; }\n@replace export class C { tag: string = 'old'; @member method(): number { return 1; } }\nevents.push(['after', peek() === C, new C().tag]); export const result: any[] = events;\n";
const observer = "import { C } from './main.js';\nexport function peek(): any { return C; }\n";
const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-ge5-decorator-publication-')));
const compiler = path.join(root, 'target/debug/bluetsc'), compilerSha256 = graph.sha256(fs.readFileSync(compiler));
const rows = [], failures = [];
function outputs(base) {
  const result = {};
  function visit(directory) {
    for (const name of fs.readdirSync(directory).sort()) {
      const file = path.join(directory, name);
      if (fs.statSync(file).isDirectory()) visit(file);
      else result[path.relative(base, file).replaceAll(path.sep, '/')] = fs.readFileSync(file, 'utf8');
    }
  }
  visit(path.join(base, 'out')); return result;
}
function cli(base) {
  const result = spawnSync(compiler, ['--project', path.join(base, 'tsconfig.json'), '--diagnostics-json'], {encoding: 'utf8'});
  assert.equal(result.status, 0, result.stderr); assert.equal(result.stderr, ''); return outputs(base);
}
try {
  const modules = process.argv.includes('--probe') ? ['CommonJS'] : ['CommonJS', 'AMD', 'UMD', 'System'];
  for (const target of ['ES5', 'ES2015', 'ES2022', 'ESNext']) for (const module of modules) for (const variant of ['original', 'crlfAstral']) {
    const id = [target, module, variant].join('/'), base = path.join(directory, String(rows.length + failures.length));
    const row = {form: 'legacy-class-publication-cycle', entry: 'main.ts', runtimeEntry: 'out/main.js', runtimeModule: module,
      files: {'main.ts': graph.coordinate(source, variant), 'observer.ts': graph.coordinate(observer, variant), 'package.json': '{"type":"commonjs"}\n'},
      options: {target, module, moduleResolution: 'Node10', lib: ['ES2022'], types: [], strict: true,
        experimentalDecorators: true, useDefineForClassFields: false, declaration: true, declarationMap: true,
        sourceMap: true, inlineSources: true, noEmitOnError: true, rootDir: '.', outDir: 'out'}};
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
      if (process.argv.includes('--probe')) console.log(JSON.stringify({id, native: expected, blue: result}));
      assert.deepEqual(result, expected, 'initial publication and final replacement through the actual cycle');
      const controls = [];
      // Each mutation is executed with a fresh module cache in a real process.
      const syntax = ts.createSourceFile('main.js', actual['out/main.js'], ts.ScriptTarget.Latest, true, ts.ScriptKind.JS), publications = [];
      function visit(node) {
        if (ts.isExpressionStatement(node) && ts.isBinaryExpression(node.expression)
          && node.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken
          && node.expression.left.getText() === 'exports.C' && node.expression.right.getText() === 'C') publications.push(node);
        ts.forEachChild(node, visit);
      }
      visit(syntax);
      assert.equal(publications.length, target === 'ES5' ? 1 : 2, 'complete target-specific publication inventory');
      for (const [index, node] of publications.entries()) {
        const start = node.getStart(syntax), end = node.end;
        const changed = {...actual, 'out/main.js': actual['out/main.js'].slice(0, start)
          + actual['out/main.js'].slice(start, end).replace(/[^\r\n]/g, ' ') + actual['out/main.js'].slice(end)};
        const changedResult = observed.execute({}, row, changed);
        assert.notDeepEqual(changedResult, expected, 'erasing a publication must change the cycle observation');
        controls.push({label: 'erase publication ' + index, rejected: true});
      }
      rows.push({id, repeatBuilds: 2, native: expected, blue: result, controls});
    } catch (error) { failures.push({id, error: error.message, stack: error.stack}); }
  }
  const report = {compilerSha256, scope: 'supplemental legacy decorator publication-cycle execution; frozen map cohort unchanged', rows, failures};
  if (process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY) {
    fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, {recursive: true});
    fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY, 'decorator-publication.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify({configurations: rows.length + failures.length, passed: rows.length, failures}));
  if (failures.length) process.exitCode = 1;
} finally { fs.rmSync(directory, {recursive: true, force: true}); }
