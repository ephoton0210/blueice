// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Immutable parent graphs and explicit coordinate/profile transformations.
const fs = require('node:fs'), path = require('node:path');
const assert = require('node:assert/strict'), crypto = require('node:crypto');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');

function parents(root, input) {
  return Object.fromEntries(Object.entries(input.parentAuthorities).map(([suite, authority]) => {
    const directory = path.join(root, 'backend/bluets/tests/fixtures', suite);
    const bytes = fs.readFileSync(path.join(directory, 'cases.json'));
    assert.equal(sha256(bytes), authority.casesSha256, suite + ': parent cases changed');
    assert.equal(sha256(fs.readFileSync(path.join(directory, 'reference.json'))),
      authority.referenceSha256, suite + ': parent reference changed');
    const graph = JSON.parse(bytes);
    return [suite, { ...graph, byId: new Map(graph.cases.map(row => [row.id, row])) }];
  }));
}

function coordinate(source, variant) {
  if (variant === 'original') return source;
  assert(['lf', 'crlf', 'lfAstral', 'crlfAstral'].includes(variant));
  const newline = variant.startsWith('crlf') ? '\r\n' : '\n';
  const prefix = variant.endsWith('Astral') ? '/* G-E5 🧭 */ ' : '';
  return prefix + source.replace(/\r\n|\r|\n/g, newline);
}

function resolve(input, row, origins) {
  let files, entry, options;
  if (row.origin) {
    const parent = origins[row.origin.suite], original = parent.byId.get(row.origin.id);
    assert(original && original.accepts && !original.nativeRuntimeDefect, row.id);
    files = { ...(original.provider === 'tslib-2.8.1' ? parent.sharedFiles : {}), ...original.files };
    // Shared helper-provider bytes are authority, not coordinate fixtures.
    files = Object.fromEntries(Object.entries(files).map(([name, text]) => [name,
      Object.hasOwn(original.files, name) && /\.(?:ts|mts|cts)$/.test(name) ? coordinate(text, row.variant) : text]));
    entry = original.entry; options = original.options;
  } else {
    assert(Object.hasOwn(input.regressions, row.regression), row.id);
    files = { 'main.ts': coordinate(input.regressions[row.regression], row.variant) };
    entry = 'main.ts';
    options = { strict: true, target: 'ES2022', module: row.module, moduleResolution: 'Node10',
      lib: ['ES2022'], types: [], declaration: true, noEmitOnError: true, rootDir: '.', outDir: 'out' };
  }
  return { files, entry, options: { ...options, ...input.profiles[row.profile] } };
}

function write(base, files) {
  for (const [name, text] of Object.entries(files)) {
    const target = path.resolve(base, name), relative = path.relative(base, target);
    assert(relative && !relative.startsWith('..') && !path.isAbsolute(relative), name);
    fs.mkdirSync(path.dirname(target), { recursive: true }); fs.writeFileSync(target, text);
  }
}
module.exports = { parents, coordinate, resolve, write, sha256 };
