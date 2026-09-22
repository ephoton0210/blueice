// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * Rebuilds the table of non-primary IANA time zone identifiers used by
 * `backend/ecma402/src/time_zone_identifiers.rs`.
 *
 * ECMA-402's `AvailableNamedTimeZoneIdentifiers` gives every IANA Zone or Link
 * name a *primary identifier*: the name itself for a Zone and for any name in
 * the TZ column of `zone.tab`, otherwise the Zone the Link resolves to,
 * adjusted so that a Link that crosses a country border resolves to the zone of
 * its own country. Names of the UTC group (`Etc/UTC`, `Etc/GMT`, `GMT` and
 * everything linked to them) share the primary identifier `UTC`.
 *
 * Usage:
 *   node backend/ecma402/tools/generate_time_zone_identifiers.mjs \
 *     --standard-zi /path/to/standard/tzdata.zi \
 *     --backzone-zi /path/to/backzone-built/tzdata.zi \
 *     --zone-tab /path/to/zone.tab \
 *     [--links-js test262/test/intl402/Temporal/ZonedDateTime/links.js] \
 *     [--check]
 *
 * Inputs (all three must come from the same tz database release, whose
 * `# version` header is recorded in the output):
 *
 *   --standard-zi  `tzdata.zi` of the IANA distribution's default build
 *                  (`make tzdata.zi`; the PyPI `tzdata` package ships this
 *                  form). It says which names are Zones and which are Links.
 *   --backzone-zi  `tzdata.zi` of a build with `PACKRATDATA=backzone
 *                  PACKRATLIST=zone.tab` (Debian and Ubuntu's `tzdata`
 *                  package ships this form: `Africa/Accra` is a Zone there).
 *                  Its Links point at the zone of the alias's own country, and
 *                  stand in for the `backzone` file's `Link` lines, which the
 *                  specification consults for multi-zone countries.
 *   --zone-tab     `zone.tab` of the same release.
 *   --links-js     optional. Test262's `links.js` lists every non-primary
 *                  identifier with a zone it must equal; when given, the
 *                  generated table must agree with it exactly.
 *   --check        compare against the checked-in table instead of writing it.
 *
 * The output replaces `backend/ecma402/src/time_zone_identifiers/table.rs`.
 * It has no timestamps and is byte-for-byte reproducible.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const outputPath = path.resolve(scriptDirectory, '../src/time_zone_identifiers/table.rs');

// A Link whose own territory is not the territory of the zone the
// backzone-built database links it to: `Atlantic/Jan_Mayen` (SJ) links to
// `Europe/Oslo` (NO) there. The specification resolves it through `zone.tab`'s
// only SJ line (`Arctic/Longyearbyen`).
const TERRITORY_OVERRIDES = new Map([['Atlantic/Jan_Mayen', 'SJ']]);
const UTC_GROUP = new Set(['Etc/UTC', 'Etc/GMT', 'GMT']);

function argument(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? undefined : process.argv[index + 1];
}

function readZoneInfo(file) {
  const zones = new Set();
  const links = new Map();
  let version;
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    const fields = line.split(/\s+/);
    if (line.startsWith('# version ')) version = fields[2];
    else if (fields[0] === 'Z') zones.add(fields[1]);
    else if (fields[0] === 'L') links.set(fields[2], fields[1]);
  }
  if (!version) throw new Error(`${file}: no "# version" header`);
  return { version, zones, links };
}

function readZoneTab(file) {
  const territory = new Map();
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    if (line === '' || line.startsWith('#')) continue;
    const [code, , name] = line.split('\t');
    territory.set(name, code);
  }
  return territory;
}

const standard = readZoneInfo(argument('--standard-zi'));
const backzone = readZoneInfo(argument('--backzone-zi'));
const zoneTab = readZoneTab(argument('--zone-tab'));
if (standard.version !== backzone.version) {
  throw new Error(`tz release mismatch: ${standard.version} vs ${backzone.version}`);
}

const zonesOfTerritory = new Map();
for (const [name, code] of zoneTab) {
  zonesOfTerritory.set(code, [...(zonesOfTerritory.get(code) ?? []), name]);
}

const names = [...standard.zones, ...standard.links.keys()].sort();
for (const name of zoneTab.keys()) {
  if (!names.includes(name)) throw new Error(`zone.tab names unknown zone ${name}`);
}

function resolveZone(name) {
  while (standard.links.has(name)) name = standard.links.get(name);
  return name;
}

/** `AvailableNamedTimeZoneIdentifiers` step 5 for one identifier. */
function primaryOf(name) {
  let primary = name;
  if (!zoneTab.has(name) && standard.links.has(name)) {
    const zone = resolveZone(name);
    const linkTarget = backzone.links.get(name);
    const territory = TERRITORY_OVERRIDES.get(name) ?? zoneTab.get(linkTarget);
    if (zone.startsWith('Etc/')) {
      primary = zone;
    } else if (territory === undefined || territory === zoneTab.get(zone)) {
      primary = zone;
    } else if (zonesOfTerritory.get(territory).length === 1) {
      primary = zonesOfTerritory.get(territory)[0];
    } else {
      primary = linkTarget;
    }
  }
  return UTC_GROUP.has(primary) ? 'UTC' : primary;
}

const nonPrimary = names
  .map((name) => [name, primaryOf(name)])
  .filter(([name, primary]) => name !== primary);
const primaries = new Set(names.map(primaryOf));
if (!primaries.has('UTC') || primaryOf('UTC') !== 'UTC') throw new Error('UTC must be primary');
for (const [, primary] of nonPrimary) {
  if (primaryOf(primary) !== primary) throw new Error(`${primary} is not a fixed point`);
}

if (argument('--links-js')) {
  const source = fs.readFileSync(argument('--links-js'), 'utf8');
  const expected = new Map();
  for (const table of source.matchAll(/const \w+TestCases = \{([^}]*)\};/g)) {
    for (const [, link, zone] of table[1].matchAll(/"([^"]+)":\s*"([^"]+)"/g)) {
      expected.set(link, zone);
    }
  }
  const generated = new Map(nonPrimary);
  const keys = (map) => [...map.keys()].sort().join('\n');
  if (keys(expected) !== keys(generated)) throw new Error('links.js and the table list different links');
  for (const [link, zone] of expected) {
    if (primaryOf(link) !== primaryOf(zone)) throw new Error(`links.js: ${link} != ${zone}`);
  }
}

// rustfmt breaks a tuple wider than its 60-column `fn_call_width`, and the
// output must be rustfmt-stable so that `--check` and `cargo fmt` agree.
const row = ([name, primary]) => {
  const tuple = `("${name}", "${primary}")`;
  return tuple.length <= 60
    ? `    ${tuple},\n`
    : `    (\n        "${name}",\n        "${primary}",\n    ),\n`;
};
const rows = nonPrimary.map(row).join('');
const output = `// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// @generated by backend/ecma402/tools/generate_time_zone_identifiers.mjs.
// Do not edit by hand; see that script for the inputs and how to rebuild.
//
// tz database release ${standard.version}: ${names.length} names, ${nonPrimary.length} of them not primary,
// ${primaries.size} primary identifiers.

pub(super) const TZDATA_VERSION: &str = "${standard.version}";

/// Every IANA Zone or Link name that is not its own ECMA-402 primary
/// identifier, with that identifier. Sorted by name (byte order).
pub(super) static NON_PRIMARY_IDENTIFIERS: [(&str, &str); ${nonPrimary.length}] = [
${rows}];
`;

if (process.argv.includes('--check')) {
  if (fs.readFileSync(outputPath, 'utf8') !== output) {
    console.error(`${outputPath} differs from the regenerated table`);
    process.exit(1);
  }
  console.log(`${outputPath} is up to date`);
} else {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, output);
  console.log(`wrote ${outputPath} (${nonPrimary.length} of ${names.length} names are not primary)`);
}
