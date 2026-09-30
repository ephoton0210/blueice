// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * Rebuilds the pinned CLDR regional data used by `Intl.Locale` information
 * methods and DateTimeFormat's default calendar/hour-cycle selection.
 *
 * Usage:
 *   node backend/ecma402/tools/generate_cldr_locale_information.mjs \
 *     /path/to/cldr-json \
 *     backend/ecma402/src/locale_data/locale_information_data.b64
 *
 * CI pins Node 24 and the source revision below on macOS, Linux, and Windows.
 * The BCP-47 time-zone aliases deliberately retain their first IANA spelling:
 * that is the spelling CLDR exposes to `Intl.Locale.prototype.getTimeZones`.
 */

import childProcess from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import zlib from 'node:zlib';

const CLDR_JSON_REVISION = '26a79cb42bfcc90def764102aa2af126d9ef3108';
const EXPECTED = Object.freeze({
  calendarRegions: 52,
  hourCycleRules: 276,
  weekRegions: 151,
  timeZoneRegions: 247,
  timeZones: 419,
});
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultOutput = path.resolve(
  scriptDirectory,
  '../src/locale_data/locale_information_data.b64',
);
const [cldrRoot, output = defaultOutput] = process.argv.slice(2);

if (!cldrRoot) {
  throw new Error(
    'usage: generate_cldr_locale_information.mjs <cldr-json-root> [output.b64]',
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

function read(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(root, relativePath), 'utf8'));
}

const supplementalRoot = 'cldr-core/supplemental';
const calendarPreferenceData = read(
  `${supplementalRoot}/calendarPreferenceData.json`,
).supplemental.calendarPreferenceData;
const timeData = read(`${supplementalRoot}/timeData.json`).supplemental.timeData;
const weekData = read(`${supplementalRoot}/weekData.json`).supplemental.weekData;
const territoryContainment = read(
  `${supplementalRoot}/territoryContainment.json`,
).supplemental.territoryContainment;
const timeZoneData = read('cldr-bcp47/bcp47/timezone.json').keyword.u.tz;

function calendarIdentifier(identifier) {
  return identifier === 'gregorian' ? 'gregory' : identifier;
}

const weekdayNumber = Object.freeze({
  mon: 1,
  tue: 2,
  wed: 3,
  thu: 4,
  fri: 5,
  sat: 6,
  sun: 7,
});

function weekend(start, end) {
  const values = [];
  for (let day = weekdayNumber[start]; ; day = day === 7 ? 1 : day + 1) {
    values.push(day);
    if (day === weekdayNumber[end]) return values;
  }
}

function knownTerritories() {
  const territories = new Set(['001']);
  for (const [territory, data] of Object.entries(territoryContainment)) {
    if (territory.startsWith('_')) continue;
    territories.add(territory);
    for (const child of data._contains ?? []) territories.add(child);
  }
  return territories;
}

const rows = [];
for (const [region, calendars] of Object.entries(calendarPreferenceData).sort(([a], [b]) =>
  a.localeCompare(b),
)) {
  rows.push(['calendar', region, calendars.map(calendarIdentifier).join(',')]);
}
for (const [key, data] of Object.entries(timeData).sort(([a], [b]) => a.localeCompare(b))) {
  const hourCycle = data._preferred === 'h' ? 'h12' : data._preferred === 'H' ? 'h23' : null;
  if (!hourCycle) throw new Error(`unsupported preferred hour cycle for ${key}: ${data._preferred}`);
  rows.push(['hour', key.toLowerCase(), hourCycle]);
}

const weekRegions = new Set([
  ...Object.keys(weekData.firstDay),
  ...Object.keys(weekData.weekendStart),
  ...Object.keys(weekData.weekendEnd),
]);
for (const region of [...weekRegions].sort()) {
  if (region.includes('-alt-')) continue;
  const firstDay = weekdayNumber[weekData.firstDay[region] ?? weekData.firstDay['001']];
  const start = weekData.weekendStart[region] ?? weekData.weekendStart['001'];
  const end = weekData.weekendEnd[region] ?? weekData.weekendEnd['001'];
  rows.push(['week', region, `${firstDay}|${weekend(start, end).join(',')}`]);
}

const territories = knownTerritories();
const zonesByRegion = new Map();
for (const [bcp47Key, data] of Object.entries(timeZoneData)) {
  if (bcp47Key.startsWith('_') || data._deprecated || data._preferred) continue;
  const region = data._region ?? bcp47Key.slice(0, 2).toUpperCase();
  if (!territories.has(region)) continue;
  const timeZone = data._alias?.split(' ')[0];
  if (!timeZone || timeZone === 'Etc/Unknown') continue;
  const values = zonesByRegion.get(region) ?? [];
  values.push(timeZone);
  zonesByRegion.set(region, values);
}
for (const [region, zones] of [...zonesByRegion.entries()].sort(([a], [b]) => a.localeCompare(b))) {
  const canonicalZones = [...new Set(zones)].sort();
  rows.push(['timeZone', region, canonicalZones.join(',')]);
}

const counts = {
  calendarRegions: Object.keys(calendarPreferenceData).length,
  hourCycleRules: Object.keys(timeData).length,
  weekRegions: [...weekRegions].filter((region) => !region.includes('-alt-')).length,
  timeZoneRegions: zonesByRegion.size,
  timeZones: [...zonesByRegion.values()].reduce(
    (total, zones) => total + new Set(zones).size,
    0,
  ),
};
if (Object.entries(EXPECTED).some(([key, expected]) => counts[key] !== expected)) {
  throw new Error(`unexpected CLDR locale-information table shape: ${JSON.stringify(counts)}`);
}

const table = rows
  .map(([kind, key, value]) => `${kind}\t${key}\t${Buffer.from(value).toString('base64')}`)
  .join('\n')
  .concat('\n');
const compressed = zlib.gzipSync(Buffer.from(table), { mtime: 0 });
const encoded = compressed.toString('base64').replace(/.{1,76}/g, '$&\n');
fs.writeFileSync(path.resolve(output), encoded);
console.log(
  JSON.stringify({
    cldrJsonRevision: revision,
    ...counts,
    rows: rows.length,
    tsvBytes: Buffer.byteLength(table),
    gzipBytes: compressed.length,
    base64Bytes: Buffer.byteLength(encoded),
    sha256: crypto.createHash('sha256').update(encoded).digest('hex'),
  }),
);
