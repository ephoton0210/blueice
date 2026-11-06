// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Source-only G-E5 input construction. Never reads a BlueTSC observation.
const fs = require('node:fs'), path = require('node:path');
const assert = require('node:assert/strict'), crypto = require('node:crypto');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
const parents = {
  ge1_downlevel: 500,
  ge2_module_helpers: 2527,
  ge3_decorator_compositions: 727,
  gd2_declaration_maps: 196,
};
const profiles = {
  external: { sourceMap: true, inlineSourceMap: false, inlineSources: true, declarationMap: true },
  externalWithoutContent: { sourceMap: true, inlineSourceMap: false, inlineSources: false, declarationMap: true },
  inline: { sourceMap: false, inlineSourceMap: true, inlineSources: true, declarationMap: true },
  rooted: { sourceMap: true, inlineSourceMap: false, inlineSources: true, declarationMap: true,
    sourceRoot: '../sources/', mapRoot: '../maps' },
};
const regressions = {
  'generator-body-switch': 'export function* sequence(): Generator<number, void, unknown> { switch (1) { case 1: break; }; yield 42; }\n',
  'function-body-call': 'export function read(): number { return Math.abs(-1); }\n',
  'function-record-return': 'export function read(): { value: number } { return { value: 1 }; }\n',
  'function-callable-return': 'export function read(): () => number { return () => 1; }\n',
  'function-generic-callable-constraint': 'export function read<T extends (x: number) => number>(value: T): T { return value; }\n',
  'function-generic-record-return': 'export function read<T extends { value: number }>(value: T): { out: T } { return { out: value }; }\n',
  'function-tuple-callable-return': 'export function read(): [() => number, { value: number }] { return [() => 1, { value: 1 }]; }\n',
  'function-default-call': 'export function read(value: number = Math.abs(-1)): number { return value; }\n',
  'enum-implicit-numeric': 'export enum Kind { First, Second, Third }\n',
  'enum-initialized-numeric': 'export enum Kind { First = 4, Second, Last = 10 }\n',
  'enum-explicit-strings': 'export enum Kind { First = "one", Second = "two" }\n',
  'enum-in-namespace': 'export namespace Types { export enum Kind { First, Second = 2 } }\n',
};
const ownerControls = [
  'canonical-source-and-map-output-authority',
  'atomic-map-publication-and-corruption-rejection',
  'charged-map-generation-decoding-and-lookup',
  'map-options-source-revision-and-cache-identity',
  'generated-helper-locations-never-claim-user-source',
  'real-process-stale-substituted-corrupt-maps-fail-closed',
];

function build() {
  const authorities = {}, cases = [], representatives = [];
  for (const [suite, expected] of Object.entries(parents)) {
    const filename = path.join(fixtures, suite, 'cases.json');
    const bytes = fs.readFileSync(filename), input = JSON.parse(bytes);
    const referenceBytes = fs.readFileSync(path.join(fixtures, suite, 'reference.json'));
    const reference = new Map(JSON.parse(referenceBytes).cases.map(row => [row.id, row]));
    const accepted = input.cases.filter(row => row.accepts && !row.nativeRuntimeDefect);
    assert.equal(accepted.length, expected, suite);
    authorities[suite] = { casesSha256: sha256(bytes), referenceSha256: sha256(referenceBytes),
      admittedConfigurations: expected };
    const forms = new Set();
    for (const original of accepted) {
      assert.deepEqual(reference.get(original.id).diagnostics, [], suite + '/' + original.id);
      const row = { origin: { suite, id: original.id }, form: suite + '/' + original.form };
      if (!forms.has(original.form)) { representatives.push(row); forms.add(original.form); }
      for (const variant of ['original', 'crlfAstral']) cases.push({
        id: suite + '/' + original.id + '/' + variant + '/external', ...row,
        variant, profile: 'external',
      });
    }
    authorities[suite].forms = forms.size;
  }
  assert.equal(representatives.length, 125);
  for (const form of Object.keys(regressions)) {
    for (const module of ['CommonJS', 'ESNext']) {
      const row = { regression: form, form: 'regression/' + form, module };
      if (module === 'CommonJS') representatives.push(row);
      for (const variant of ['lf', 'crlf', 'lfAstral', 'crlfAstral']) cases.push({
        id: 'regression/' + form + '/' + module + '/' + variant + '/external', ...row,
        variant, profile: 'external',
      });
    }
  }
  for (const row of representatives) {
    for (const profile of ['externalWithoutContent', 'inline', 'rooted']) cases.push({
      id: (row.origin ? row.origin.suite + '/' + row.origin.id : 'regression/' + row.regression + '/' + row.module)
        + '/original/' + profile,
      ...row, variant: row.origin ? 'original' : 'lf', profile,
    });
  }
  assert.equal(cases.length, 8407);
  assert.equal(new Set(cases.map(row => row.id)).size, cases.length);
  return { _license: 'MPL-2.0', schema: 1, typescript: '5.9.3',
    parentAuthorities: authorities, profiles, regressions, cases, repeatBuilds: 2, ownerControls };
}

if (require.main === module) {
  const target = path.join(fixtures, 'ge5_source_maps', 'cases.json');
  const bytes = JSON.stringify(build(), null, 2) + '\n';
  if (process.argv.includes('--write')) { fs.mkdirSync(path.dirname(target), { recursive: true }); fs.writeFileSync(target, bytes); }
  else assert.equal(fs.readFileSync(target, 'utf8'), bytes, 'G-E5 input changed');
  console.log(JSON.stringify({ configurations: 8407, originalAdmittedConfigurations: 3950,
    sourceVariantsPerOriginal: 2, signatureAndEnumRegressions: 12,
    metadataRepresentatives: 137, repeatBuilds: 2, sha256: sha256(bytes) }));
}
module.exports = { build };
