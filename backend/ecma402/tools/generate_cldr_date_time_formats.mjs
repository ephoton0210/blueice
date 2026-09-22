// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * Rebuilds the pinned CLDR Gregorian `availableFormats` provider used by
 * ECMA-402 BasicFormatMatcher.
 *
 * Usage:
 *   node backend/ecma402/tools/generate_cldr_date_time_formats.mjs \
 *     /path/to/cldr-json \
 *     backend/ecma402/src/locale_data/date_time_formats_data.b64 \
 *     backend/ecma402/src/locale_data/date_time_append_items_data.b64
 *
 * CI pins Node 24 and the source revision below on macOS, Linux, and Windows.
 * Rows retain CLDR's `availableFormats` object order because ECMA-402 selects
 * the first record when BasicFormatMatcher scores tie.
 */

import childProcess from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import zlib from 'node:zlib';

const CLDR_JSON_REVISION = '26a79cb42bfcc90def764102aa2af126d9ef3108';
const EXPECTED = Object.freeze({ locales: 766, formats: 38792, appendItems: 8426 });
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultFormatsOutput = path.resolve(scriptDirectory, '../src/locale_data/date_time_formats_data.b64');
const defaultAppendItemsOutput = path.resolve(
  scriptDirectory,
  '../src/locale_data/date_time_append_items_data.b64',
);
const [cldrRoot, formatsOutput = defaultFormatsOutput, appendItemsOutput = defaultAppendItemsOutput] =
  process.argv.slice(2);

if (!cldrRoot) {
  throw new Error(
    'usage: generate_cldr_date_time_formats.mjs <cldr-json-root> [available-formats.b64] [append-items.b64]',
  );
}

const root = path.resolve(cldrRoot);
const revision = childProcess
  .execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' })
  .trim();
if (revision !== CLDR_JSON_REVISION) {
  throw new Error(
    `CLDR JSON checkout must be ${CLDR_JSON_REVISION}; received ${revision || '<none>'}`,
  );
}

const dateRoot = path.join(root, 'cldr-dates-full/main');
const supportedSkeletonLetters = new Set('GyMLdEecabBhHKkmsSzvVOXxZ'.split(''));
const formatRows = [];
const appendItemRows = [];
const appendItemDateField = Object.freeze({
  Day: 'day',
  'Day-Of-Week': 'weekday',
  Era: 'era',
  Hour: 'hour',
  Minute: 'minute',
  Month: 'month',
  Quarter: 'quarter',
  Second: 'second',
  Timezone: 'zone',
  Week: 'week',
  Year: 'year',
});

function skeletonFields(skeleton) {
  const fields = [];
  let quoted = false;
  for (let index = 0; index < skeleton.length; ) {
    const character = skeleton[index];
    if (character === "'") {
      if (skeleton[index + 1] === "'") index += 2;
      else {
        quoted = !quoted;
        index += 1;
      }
      continue;
    }
    if (quoted || !/[A-Za-z]/.test(character)) {
      index += 1;
      continue;
    }
    let end = index + 1;
    while (skeleton[end] === character) end += 1;
    fields.push(character);
    index = end;
  }
  return fields;
}

for (const locale of fs.readdirSync(dateRoot).sort()) {
  const input = path.join(dateRoot, locale, 'ca-gregorian.json');
  if (!fs.existsSync(input)) continue;
  const json = JSON.parse(fs.readFileSync(input, 'utf8'));
  const main = json.main[Object.keys(json.main)[0]];
  const dateTimeFormats = main.dates.calendars.gregorian.dateTimeFormats;
  const dateFieldsInput = path.join(dateRoot, locale, 'dateFields.json');
  if (!fs.existsSync(dateFieldsInput)) {
    throw new Error(`CLDR locale ${locale} has Gregorian formats but no dateFields data`);
  }
  const dateFieldsJson = JSON.parse(fs.readFileSync(dateFieldsInput, 'utf8'));
  const dateFieldsMain = dateFieldsJson.main[Object.keys(dateFieldsJson.main)[0]];
  const dateFields = dateFieldsMain.dates.fields;
  const formats = dateTimeFormats.availableFormats;
  for (const [skeleton, pattern] of Object.entries(formats)) {
    // CLDR alternatives are non-default presentation variants, not extra
    // entries in the locale's ordinary available-format preference list.
    if (skeleton.includes('-alt-')) continue;
    const fields = skeletonFields(skeleton);
    if (fields.length === 0 || fields.some((field) => !supportedSkeletonLetters.has(field))) {
      continue;
    }
    formatRows.push([locale, skeleton, pattern]);
  }
  for (const [field, pattern] of Object.entries(dateTimeFormats.appendItems)) {
    const dateField = appendItemDateField[field];
    const fieldName = dateField && dateFields[dateField]?.displayName;
    if (!fieldName) {
      throw new Error(`CLDR locale ${locale} append item ${field} has no field display name`);
    }
    appendItemRows.push([locale, field, pattern, fieldName]);
  }
}

const formatLocaleCount = new Set(formatRows.map(([locale]) => locale)).size;
const appendItemLocaleCount = new Set(appendItemRows.map(([locale]) => locale)).size;
if (
  formatLocaleCount !== EXPECTED.locales ||
  appendItemLocaleCount !== EXPECTED.locales ||
  formatRows.length !== EXPECTED.formats ||
  appendItemRows.length !== EXPECTED.appendItems
) {
  throw new Error(
    `unexpected CLDR date-time table shape: ${formatLocaleCount}/${appendItemLocaleCount} locales, ${formatRows.length}/${appendItemRows.length} rows`,
  );
}
function encode(rows, encodeRow) {
  const table = rows
    .map(encodeRow)
    .join('\n')
    .concat('\n');
  const compressed = zlib.gzipSync(Buffer.from(table), { mtime: 0 });
  const encoded = compressed.toString('base64').replace(/.{1,76}/g, '$&\n');
  return {
    table,
    compressed,
    encoded,
    sha256: crypto.createHash('sha256').update(encoded).digest('hex'),
  };
}

const formats = encode(
  formatRows,
  ([locale, skeleton, pattern]) =>
    `${locale}\t${skeleton}\t${Buffer.from(pattern).toString('base64')}`,
);
const appendItems = encode(
  appendItemRows,
  ([locale, field, pattern, fieldName]) =>
    `${locale}\t${field}\t${Buffer.from(pattern).toString('base64')}\t${Buffer.from(fieldName).toString('base64')}`,
);
fs.writeFileSync(path.resolve(formatsOutput), formats.encoded);
fs.writeFileSync(path.resolve(appendItemsOutput), appendItems.encoded);
console.log(
  JSON.stringify({
    cldrJsonRevision: revision,
    formats: {
      locales: formatLocaleCount,
      rows: formatRows.length,
      tsvBytes: Buffer.byteLength(formats.table),
      gzipBytes: formats.compressed.length,
      base64Bytes: Buffer.byteLength(formats.encoded),
      sha256: formats.sha256,
    },
    appendItems: {
      locales: appendItemLocaleCount,
      rows: appendItemRows.length,
      tsvBytes: Buffer.byteLength(appendItems.table),
      gzipBytes: appendItems.compressed.length,
      base64Bytes: Buffer.byteLength(appendItems.encoded),
      sha256: appendItems.sha256,
    },
  }),
);
