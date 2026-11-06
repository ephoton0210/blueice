// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A borrowed method caller cannot replace the source receiver or arguments.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process'), graph = require('./ge5_inputs.cjs'), execution = require('./ge5_class_execution.cjs');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const compiler = path.resolve(process.argv[2]), hash = graph.sha256(fs.readFileSync(compiler));
if (process.argv[3]) assert.equal(hash, process.argv[3]);
const input = require(path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')), parents = graph.parents(root, input);
const text = `export function* captured(this:any,value:number):Generator<number,number,number>{try{yield this.seed+value+arguments[0];return this.seed+arguments[0];}finally{this.seen.push(arguments[0]);}}
function scenario(mode:string){const right:any={seed:10,seen:[]},wrong:any={seed:900,seen:[]},iterator:any=captured.call(right,2),errors:string[]=[];
 function borrowed(method:string,value:any){try{return iterator[method].call(wrong,value);}catch(error){if(error instanceof TypeError){errors.push("TypeError");return iterator[method](value);}throw error;}}
 const first=mode==="next"?borrowed("next",0):iterator.next();let last:any;
 try{last=mode==="return"?borrowed("return",9):mode==="throw"?borrowed("throw","stop"):iterator.next();}catch(error){last={error:String(error)};}
 return[first,last,right.seen,wrong.seen,errors];}
export const result:string=JSON.stringify([scenario("next"),scenario("return"),scenario("throw")]);
`;
const temporary = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'ge5-sync-generator-capture-'));
function outputs(directory, result = {}) {
  if (fs.existsSync(directory)) for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
    const name = path.join(directory, entry.name);
    if (entry.isDirectory()) outputs(name, result);
    else result[path.relative(temporary, name).replace(/^\d+\//, '')] = fs.readFileSync(name, 'utf8');
  }
  return result;
}
(async () => {
  const verified = [], failures = [], comparisonFailures = [], keys = new Set(); let number = 0;
  try {
    for (const [index, specification] of input.cases.entries()) if (specification.form === 'ge2_module_helpers/generator-switch-delegation') {
      const row = graph.resolve(input, specification, parents);
      if (!['CommonJS', 'ESNext'].includes(row.options.module) || row.options.importHelpers && row.options.noEmitHelpers) continue;
      const astral = row.files['main.ts'].includes('\r\n');
      const key = [row.options.target, row.options.module, row.options.importHelpers, row.options.noEmitHelpers, astral].join(':');
      if (keys.has(key)) continue; keys.add(key);
      const source = astral ? '/*🧊 original receiver */\r\n' + text.replace(/\n/g, '\r\n') : text;
      const directory = path.join(temporary, String(number++));
      for (const [name, content] of Object.entries({...row.files, 'main.ts': source})) {
        const file = path.join(directory, name); fs.mkdirSync(path.dirname(file), {recursive: true}); fs.writeFileSync(file, content);
      }
      fs.writeFileSync(path.join(directory, 'tsconfig.json'), JSON.stringify({compilerOptions: row.options, files: ['main.ts']}));
      try {
        function build() {
          const result = spawnSync(compiler, ['--project', path.join(directory, 'tsconfig.json'), '--diagnostics-json'], {encoding: 'utf8', maxBuffer: 8e6});
          assert(!result.error); assert.equal(result.status, 0, result.stderr); assert.equal(result.stderr, '');
          return {...outputs(path.join(directory, 'out')), ...outputs(path.join(directory, 'types'))};
        }
        const actual = build(); assert.deepEqual(build(), actual, 'complete repeated receiver/arguments public CLI build');
        const configuration = ts.parseJsonConfigFileContent({compilerOptions: row.options, files: ['main.ts']}, ts.sys, directory);
        assert.deepEqual(configuration.errors, []);
        const program = ts.createProgram(configuration.fileNames, configuration.options);
        const diagnostics = ts.getPreEmitDiagnostics(program);
        assert.equal(diagnostics.length, 0, diagnostics.map(d => ts.flattenDiagnosticMessageText(d.messageText, ' ')).join('\n'));
        const nativeOutputs = {};
        const emitted = program.emit(undefined, (file, text) => {nativeOutputs[path.relative(directory, file)] = text;});
        assert(!emitted.emitSkipped); assert.equal(emitted.diagnostics.length, 0);
        const native = await execution.execute(nativeOutputs, row.files, row.options);
        const observed = await execution.execute(actual, row.files, row.options);
        const errors = row.options.target === 'ES5' ? [] : ['TypeError'];
        const expected = JSON.stringify([[{value:14,done:false},{value:12,done:true},[2],[],errors],
          [{value:14,done:false},{value:9,done:true},[2],[],errors],
          [{value:14,done:false},{error:'stop'},[2],[],errors]]);
        assert.equal(observed, expected, 'Blue preserves the original receiver and original arguments in every target');
        if (row.options.target === 'ES5') {
          const values = JSON.parse(native); assert.equal(values.length, 3);
          for (const value of values) {
            assert.equal(value[0].value, '12[object Object]', 'pinned Native ES5 callback arguments differ from source arguments');
            assert.deepEqual(value[3], [], 'the wrong borrowed receiver remains untouched');
            assert.equal(value[2].length, 1); assert.equal(typeof value[2][0].label, 'number');
            assert.deepEqual(value[2][0].trys, []); assert.deepEqual(value[2][0].ops, []);
          }
          assert.equal(values[0][1].value, '10[object Object]');
          assert.deepEqual(values[1][1], {value: 9, done: true}); assert.deepEqual(values[2][1], {error: 'stop'});
          comparisonFailures.push({key, index, native, actual: observed, kind: 'supplemental Native ES5 generator callback arguments behavior'});
        } else assert.equal(native, observed, 'retained original arguments match Native intrinsic generators');
        verified.push({key, index, repeatBuilds: 2, executions: 2, sourceSha256: graph.sha256(source),
          argumentsParity: native === observed, nativeJavaScriptSha256: graph.sha256(nativeOutputs['out/main.js']),
          actualJavaScriptSha256: graph.sha256(actual['out/main.js'])});
      } catch (error) {failures.push({key, index, error: error.message, stack: error.stack});}
    }
    assert.equal(keys.size, 60); assert.equal(graph.sha256(fs.readFileSync(compiler)), hash);
    const report = {scope: 'supplemental original receiver/arguments; Native ES5 difference is outside the frozen 363 source',
      completeFamily: false, frozenConfigurationsAdded: 0, compilerSha256: hash, profiles: keys.size,
      argumentsParity: {passed: verified.filter(row => row.argumentsParity).length, failed: comparisonFailures.length},
      verified, comparisonFailures, failures};
    if (process.env.BLUEICE_GE5_REPORT) fs.writeFileSync(process.env.BLUEICE_GE5_REPORT, JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify(report, null, 2)); if (failures.length) process.exitCode = 1;
  } finally {fs.rmSync(temporary, {recursive: true, force: true});}
})().catch(error => {console.error(error); process.exitCode = 1;});
