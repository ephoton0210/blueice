// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// One observation format for stock and independently patched Native compilers.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const sorted = object => Object.fromEntries(Object.entries(object).sort(([a], [b]) => a.localeCompare(b)));

module.exports = function compiler(ts) {
  const libraries = new Map(), libRoot = path.dirname(ts.getDefaultLibFilePath({}));
  return function compile(base, row) {
    const filename = path.join(base, 'tsconfig.json');
    const text = JSON.stringify({ compilerOptions: row.options, files: [row.entry] }, null, 2) + '\n';
    fs.writeFileSync(filename, text);
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(filename, text), ts.sys, base, {}, filename);
    const host = ts.createCompilerHost(parsed.options), read = host.getSourceFile, outputs = {};
    host.getSourceFile = (filename, ...args) => {
      if (path.dirname(filename) !== libRoot) return read(filename, ...args);
      const key = filename + JSON.stringify(args[0]);
      if (!libraries.has(key)) libraries.set(key, read(filename, ...args));
      return libraries.get(key);
    };
    host.writeFile = (filename, text, bom) => {
      const relative = path.relative(base, filename).replaceAll(path.sep, '/');
      assert(!relative.startsWith('../') && !path.isAbsolute(relative), 'Native output escaped input root');
      outputs[relative] = (bom ? '\ufeff' : '') + text;
    };
    const program = ts.createProgram(parsed.fileNames, parsed.options, host);
    const describe = diagnostic => ({ code: diagnostic.code,
      message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n').replaceAll(base, '<root>'),
      module: diagnostic.file ? path.relative(base, diagnostic.file.fileName).replaceAll(path.sep, '/') : null,
      start: diagnostic.start ?? null, length: diagnostic.length ?? null,
      related: (diagnostic.relatedInformation || []).map(describe) });
    let diagnostics = ts.sortAndDeduplicateDiagnostics([...parsed.errors, ...ts.getPreEmitDiagnostics(program)]);
    if (!diagnostics.length) diagnostics = ts.sortAndDeduplicateDiagnostics([...diagnostics, ...program.emit().diagnostics]);
    const selectedInputs = program.getSourceFiles().map(source => path.relative(base, source.fileName).replaceAll(path.sep, '/'))
      .filter(name => !name.startsWith('../')).sort();
    return { configuration: text, diagnostics: diagnostics.map(describe), selectedInputs, outputs: sorted(outputs) };
  };
};
