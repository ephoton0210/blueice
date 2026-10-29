// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Observe actual artifacts without giving generated declarations source authority.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const {spawnSync} = require('node:child_process');
const {pathToFileURL} = require('node:url');
const crypto = require('node:crypto');
const {owned, write, imports} = require('./gm1_observations.cjs');

const sorted = value => Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)));
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
function changes(root, files) {
  for (const [name, text] of Object.entries(files || {})) {
    if (text === null) fs.rmSync(owned(root, name));
    else write(root, {[name]: text});
  }
}
function collect(root, inputs) {
  const outputs = {};
  function visit(directory) {
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name);
      if (fs.lstatSync(file).isDirectory()) visit(file);
      else {
        const relative = path.relative(root, file).replaceAll(path.sep, '/');
        if (/\.(?:js|mjs|cjs|jsx|d\.ts|d\.mts|d\.cts|map)$/.test(relative) && !(relative in inputs)) {
          outputs[relative] = fs.readFileSync(file, 'utf8');
        }
      }
    }
  }
  visit(root);
  return sorted(outputs);
}
function diagnostic(ts, root, item) {
  return {
    code: item.code,
    message: ts.flattenDiagnosticMessageText(item.messageText, '\n').replaceAll(root, '<root>'),
    module: item.file ? path.relative(root, path.resolve(root, item.file.fileName)).replaceAll(path.sep, '/') : null,
    start: item.start ?? null, length: item.length ?? null,
    related: (item.relatedInformation || []).map(value => diagnostic(ts, root, value)),
  };
}
function artifacts(ts, acorn, row, outputs, inputs) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-gm2-observe-')));
  try {
    const auxiliary = Object.fromEntries(Object.entries(inputs).filter(([name]) =>
      name.endsWith('package.json') || /\.d\.(?:ts|mts|cts)$/.test(name)));
    write(root, auxiliary);
    write(root, outputs);
    const declarations = Object.fromEntries(Object.entries(outputs).filter(([name]) => /\.d\.(?:ts|mts|cts)$/.test(name)));
    const exports = Object.entries(row.exports || {}).filter(([name]) =>
      name in outputs || name.replace(/\.(?:js|mjs|cjs)$/, '.d.ts') in outputs);
    let source = '';
    let reject = '';
    for (const [index, [name, binding]] of exports.entries()) {
      const specifier = './' + name.replace(/\.(?:js|mjs|cjs)$/, '.js');
      if (!(name.replace(/\.(?:js|mjs|cjs)$/, '.d.ts') in declarations)) continue;
      const prefix = `import * as m${index} from ${JSON.stringify(specifier)};\n`;
      source += prefix + `const v${index}: ${binding.type} = m${index}.${binding.name};\n`;
      reject += prefix + `const v${index}: boolean = m${index}.${binding.name};\n`;
    }
    function consumer(text) {
      if (!source || !Object.keys(declarations).length) return null;
      write(root, {'consumer.ts': text});
      const program = ts.createProgram([path.join(root, 'consumer.ts')], {
        strict: true, target: ts.ScriptTarget.ES2022,
        module: row.module === 'CommonJS' ? ts.ModuleKind.CommonJS : ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Node10,
        lib: ['lib.es2022.d.ts'], types: [], noEmit: true,
      });
      return ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(d => diagnostic(ts, root, d));
    }
    const consumers = {accept: consumer(source), reject: consumer(reject)};
    if (consumers.accept) {
      assert.deepEqual(consumers.accept, [], row.id + ': valid consumer');
      assert(consumers.reject.length, row.id + ': invalid consumer');
    }
    const executions = {};
    for (const [name, code] of Object.entries(outputs).filter(([name]) => /\.(?:js|mjs|cjs)$/.test(name))) {
      acorn.parse(code, {ecmaVersion: row.target === 'ES5' ? 5 : 2022,
        sourceType: row.module === 'CommonJS' ? 'script' : 'module'});
    }
    for (const [name, binding] of exports) {
      if (!(name in outputs)) continue;
      const file = owned(root, name);
      const load = row.module === 'CommonJS' ? `Promise.resolve(require(${JSON.stringify(file)}))` :
        `import(${JSON.stringify(pathToFileURL(file).href)})`;
      const script = `${load}.then(m=>process.stdout.write(JSON.stringify(m[${JSON.stringify(binding.name)}]))).catch(e=>{process.stderr.write(e.name+': '+e.message);process.exitCode=1;});`;
      const result = spawnSync(process.execPath, ['-e', script], {encoding: 'utf8'});
      if (result.error) throw result.error;
      executions[name] = {exit: result.status, stdout: result.stdout,
        stderr: result.stderr.replaceAll(root, '<root>')};
      assert.equal(result.status, 0, row.id + ': ' + result.stderr);
    }
    return {imports: imports(ts, declarations), consumers, executions: sorted(executions)};
  } finally { fs.rmSync(root, {recursive: true, force: true}); }
}
module.exports = {owned, write, sorted, hash, changes, collect, diagnostic, artifacts};
