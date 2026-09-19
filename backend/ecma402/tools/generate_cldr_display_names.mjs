// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * Rebuilds the pinned CLDR DisplayNames and RelativeTimeFormat provider.
 *
 * Usage:
 *   node backend/ecma402/tools/generate_cldr_display_names.mjs \
 *     /path/to/cldr-json \
 *     backend/ecma402/src/locale_data/display_names_data.b64
 *
 * The input must be the exact `unicode-org/cldr-json` commit recorded below.
 * The output deliberately has no timestamps and uses an ASCII comparator, so
 * it is byte-for-byte reproducible on macOS, Linux, and Windows.
 */

import childProcess from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import zlib from 'node:zlib';

const CLDR_JSON_REVISION = '26a79cb42bfcc90def764102aa2af126d9ef3108';
const EXPECTED = Object.freeze({
  locales: 766,
  rows: 709260,
  sha256: '8bd6a4728337428a01b1c4f95cf4c771681b5002a9851976ac72819276bb45a2',
});
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultOutput = path.resolve(scriptDirectory, '../src/locale_data/display_names_data.b64');
const [cldrRoot, output = defaultOutput] = process.argv.slice(2);

if (!cldrRoot) {
  throw new Error('usage: generate_cldr_display_names.mjs <cldr-json-root> [output.b64]');
}

const resolvedRoot = path.resolve(cldrRoot);
const revision = childProcess
  .execFileSync('git', ['-C', resolvedRoot, 'rev-parse', 'HEAD'], { encoding: 'utf8' })
  .trim();
if (revision !== CLDR_JSON_REVISION) {
  throw new Error(
    `CLDR JSON checkout must be ${CLDR_JSON_REVISION}; received ${revision || '<none>'}`,
  );
}

const namesRoot = path.join(resolvedRoot, 'cldr-localenames-full/main');
const datesRoot = path.join(resolvedRoot, 'cldr-dates-full/main');
const rows = new Map();

function add(locale, type, style, code, value) {
  if (typeof value !== 'string' || value.length === 0) return;
  const key = `${locale}\t${type}\t${style}\t${code}`;
  const previous = rows.get(key);
  if (previous !== undefined && previous !== value) {
    throw new Error(`conflicting CLDR display-name row ${key}`);
  }
  rows.set(key, value);
}

function read(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function namesFor(json) {
  return json.main[Object.keys(json.main)[0]].localeDisplayNames;
}

function addNameMap(locale, type, values) {
  if (!values) return;
  for (const [key, value] of Object.entries(values)) {
    let code = key;
    let style = 'long';
    if (key.endsWith('-alt-short')) {
      code = key.slice(0, -'-alt-short'.length);
      style = 'short';
    } else if (key.endsWith('-alt-narrow')) {
      code = key.slice(0, -'-alt-narrow'.length);
      style = 'narrow';
    } else if (key.includes('-alt-')) {
      continue;
    }
    add(locale, type, style, code, value);
  }
}

for (const locale of fs.readdirSync(namesRoot).sort()) {
  const directory = path.join(namesRoot, locale);
  const display = namesFor(read(path.join(directory, 'localeDisplayNames.json')));
  addNameMap(locale, 'calendar', display.types?.calendar);
  for (const [code, value] of Object.entries(display.localeDisplayPattern ?? {})) {
    add(locale, 'pattern', 'long', code, value);
  }
  for (const [file, type] of [
    ['languages.json', 'language'],
    ['territories.json', 'region'],
    ['scripts.json', 'script'],
    ['variants.json', 'variant'],
  ]) {
    const input = path.join(directory, file);
    if (!fs.existsSync(input)) continue;
    const source = namesFor(read(input));
    addNameMap(locale, type, source[`${type}s`] ?? source.territories);
  }
}

const dateFields = [
  ['era', 'era'],
  ['year', 'year'],
  ['quarter', 'quarter'],
  ['month', 'month'],
  ['weekOfYear', 'week'],
  ['weekday', 'weekday'],
  ['day', 'day'],
  ['dayPeriod', 'dayperiod'],
  ['hour', 'hour'],
  ['minute', 'minute'],
  ['second', 'second'],
  ['timeZoneName', 'zone'],
];
const relativeTimeUnits = [
  ['second', 'second'],
  ['minute', 'minute'],
  ['hour', 'hour'],
  ['day', 'day'],
  ['week', 'week'],
  ['month', 'month'],
  ['quarter', 'quarter'],
  ['year', 'year'],
];
for (const locale of fs.readdirSync(datesRoot).sort()) {
  const input = path.join(datesRoot, locale, 'dateFields.json');
  if (!fs.existsSync(input)) continue;
  const fields = read(input).main[locale].dates.fields;
  for (const [code, field] of dateFields) {
    for (const style of ['long', 'short', 'narrow']) {
      const suffix = style === 'long' ? '' : `-${style}`;
      add(locale, 'dateTimeField', style, code, fields[`${field}${suffix}`]?.displayName);
    }
  }
  for (const [unit, field] of relativeTimeUnits) {
    for (const style of ['long', 'short', 'narrow']) {
      const suffix = style === 'long' ? '' : `-${style}`;
      const data = fields[`${field}${suffix}`];
      for (const direction of ['future', 'past']) {
        for (const [key, value] of Object.entries(data?.[`relativeTime-type-${direction}`] ?? {})) {
          const plural = key.match(/^relativeTimePattern-count-(.+)$/)?.[1];
          if (plural) add(locale, 'relativeTimePattern', style, `${unit}|${direction}|${plural}`, value);
        }
      }
      for (const offset of [-1, 0, 1]) {
        add(locale, 'relativeTimeTerm', style, `${unit}|${offset}`, data?.[`relative-type-${offset}`]);
      }
    }
  }
}

const compareAscii = ([left], [right]) => (left < right ? -1 : left > right ? 1 : 0);
const table = [...rows]
  .sort(compareAscii)
  .map(([key, value]) => `${key}\t${Buffer.from(value).toString('base64')}`)
  .join('\n')
  .concat('\n');
const localeCount = new Set([...rows.keys()].map((key) => key.slice(0, key.indexOf('\t')))).size;
if (rows.size !== EXPECTED.rows || localeCount !== EXPECTED.locales) {
  throw new Error(`unexpected CLDR table shape: ${localeCount} locales, ${rows.size} rows`);
}

const compressed = zlib.gzipSync(Buffer.from(table), { mtime: 0 });
const encoded = compressed.toString('base64').replace(/.{1,76}/g, '$&\n');
const digest = crypto.createHash('sha256').update(encoded).digest('hex');
if (digest !== EXPECTED.sha256) {
  throw new Error(`non-reproducible CLDR output: expected ${EXPECTED.sha256}, received ${digest}`);
}
fs.writeFileSync(path.resolve(output), encoded);
console.log(
  JSON.stringify({
    cldrJsonRevision: revision,
    locales: localeCount,
    rows: rows.size,
    tsvBytes: Buffer.byteLength(table),
    gzipBytes: compressed.length,
    base64Bytes: Buffer.byteLength(encoded),
    sha256: digest,
  }),
);
