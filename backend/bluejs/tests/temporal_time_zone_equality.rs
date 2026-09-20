// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `TimeZoneEquals` compares *primary* time zone identifiers.
//!
//! Before this change two named zones were "equal" when the bundled tz
//! database gave them byte-identical TZif data (plus a hand-listed GMT group).
//! That conflates zones the tz database merged into one Link for storage but
//! that ECMA-402 keeps distinct because `zone.tab` lists them
//! (`Africa/Accra` and `Africa/Abidjan`, `Europe/Amsterdam` and
//! `Europe/Brussels`), and it misses aliases whose data merely happens to differ.
//! Equality now follows `AvailableNamedTimeZoneIdentifiers`' primary
//! identifiers (`blueice_ecma402::primary_time_zone_identifier`), which also
//! decide `Intl.supportedValuesOf("timeZone")`.
//!
//! Expectations are taken from the Test262 fixtures named at each test. The
//! groups in `zones_tzdata_merged_into_one_link_are_still_distinct` are the
//! places where the old byte comparison was wrong: zones whose TZif data is
//! identical in the bundled database, all listed in `zone.tab`, which
//! `canonical-not-equal.js` requires to differ.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
function assertSame(actual, expected, message) {
  if (!Object.is(actual, expected)) {
    throw new Error(message + ": got " + String(actual) + " want " + String(expected));
  }
}
const zdt = (id) => new Temporal.ZonedDateTime(0n, id);
"#;

/// Runs `body` after [`PRELUDE`], failing with the JS-side message on a throw.
fn check(body: &str) {
    let source =
        format!("{PRELUDE}\ntry {{\n{body}\n\"ok\"\n}} catch (e) {{ \"FAILED: \" + e.message }}");
    let value = Vm::default()
        .execute(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{body}\n  -> {error:?}"));
    match value {
        Value::String(text) => assert_eq!(text.to_utf8().unwrap(), "ok", "{body}"),
        other => panic!("unexpected completion value {other:?}"),
    }
}

/// `intl402/Temporal/ZonedDateTime/prototype/equals/{canonicalize-timezone,
/// canonicalize-iana-identifiers-before-comparing}.js` and `links.js`.
#[test]
fn aliases_equal_their_primary_zone_and_keep_their_spelling() {
    check(
        r#"
        for (const [alias, zone] of [
          ["Asia/Calcutta", "Asia/Kolkata"], ["Australia/Canberra", "Australia/Sydney"],
          ["Atlantic/Jan_Mayen", "Arctic/Longyearbyen"], ["Pacific/Truk", "Pacific/Chuuk"],
          ["Etc/UCT", "UTC"], ["Etc/GMT0", "UTC"], ["Etc/GMT+0", "UTC"], ["Greenwich", "UTC"],
          ["CET", "Europe/Brussels"], ["MET", "Europe/Brussels"], ["EST5EDT", "America/New_York"],
          ["Iceland", "Atlantic/Reykjavik"], ["Africa/Timbuktu", "Africa/Bamako"], ["America/Virgin", "America/St_Thomas"],
          ["US/Pacific", "America/Los_Angeles"], ["Europe/Kiev", "Europe/Kyiv"],
        ]) {
          const a = zdt(alias), z = zdt(zone);
          assertSame(a.timeZoneId, alias, "the alias spelling is preserved: " + alias);
          assertSame(a.equals(z), true, alias + " equals " + zone);
          assertSame(z.equals(a), true, zone + " equals " + alias);
          assertSame(a.equals(z.toString()), true, alias + " equals the string of " + zone);
        }
        // Names are matched ASCII-case-insensitively and reported as the database spells them.
        assertSame(zdt("asia/calcutta").timeZoneId, "Asia/Calcutta", "case-insensitive lookup");
        assertSame(zdt("asia/calcutta").equals(zdt("ASIA/KOLKATA")), true, "case-insensitive equals");
        // A fixed-offset Etc zone is its own primary identifier, so it is not UTC.
        assertSame(zdt("Etc/GMT+1").equals(zdt("UTC")), false, "Etc/GMT+1 is not UTC");
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/prototype/equals/canonical-not-equal.js`:
/// names in `zone.tab` are primary, however the tz database stores them.
#[test]
fn zones_tzdata_merged_into_one_link_are_still_distinct() {
    check(
        r#"
        // Groups of zone.tab names whose TZif data is byte-identical in the bundled database.
        const groups = [
          ["Africa/Abidjan", "Africa/Accra", "Africa/Bamako", "Africa/Banjul", "Africa/Conakry", "Africa/Dakar",
           "Africa/Freetown", "Africa/Lome", "Africa/Nouakchott", "Africa/Ouagadougou", "Atlantic/St_Helena"],
          ["Europe/Amsterdam", "Europe/Brussels", "Europe/Luxembourg"],
          ["Europe/Prague", "Europe/Bratislava"],
          ["Europe/Oslo", "Europe/Berlin", "Europe/Copenhagen", "Europe/Stockholm", "Arctic/Longyearbyen"],
          ["America/Puerto_Rico", "America/St_Thomas", "America/Anguilla", "America/Antigua", "America/Aruba", "America/Tortola"],
          ["Pacific/Pago_Pago", "Pacific/Midway"],
          ["Africa/Lagos", "Africa/Bangui", "Africa/Douala", "Africa/Kinshasa"],
          ["Asia/Bangkok", "Asia/Phnom_Penh", "Asia/Vientiane"],
        ];
        for (const group of groups) {
          for (let i = 0; i < group.length; i++) {
            for (let j = i + 1; j < group.length; j++) {
              assertSame(zdt(group[i]).equals(zdt(group[j])), false, group[i] + " does not equal " + group[j]);
            }
          }
        }
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/supported-values-of.js`: every listed
/// identifier constructs and keeps its own spelling, and none of them equals
/// another; an offset zone equals only the same offset.
#[test]
fn every_supported_identifier_constructs_and_no_two_of_them_are_equal() {
    check(
        r#"
        const ids = Intl.supportedValuesOf("timeZone");
        assertSame(ids.length, 446, "the primary identifiers");
        const instances = ids.map((id) => {
          const instance = zdt(id);
          assertSame(instance.timeZoneId, id, "timeZoneId of " + id);
          return instance;
        });
        // Comparing each identifier with a fixed spread of others keeps this quick;
        // the exhaustive pairing is `canonical-not-equal.js` itself.
        for (let i = 0; i < ids.length; i++) {
          for (const step of [1, 2, 17, 101]) {
            const j = (i + step) % ids.length;
            if (i !== j) assertSame(instances[i].equals(instances[j]), false, ids[i] + " vs " + ids[j]);
          }
        }
        assertSame(zdt("+01:00").equals(zdt("+01:00")), true, "same offset");
        assertSame(zdt("+01:00").equals(zdt("+02:00")), false, "different offsets");
        assertSame(zdt("+00:00").equals(zdt("UTC")), false, "an offset is never a named zone");
    "#,
    );
}

/// The registry used to resolve zone arguments must accept every identifier,
/// aliases included, though `Intl.supportedValuesOf` no longer lists them.
#[test]
fn zone_arguments_accept_aliases_that_supported_values_no_longer_list() {
    check(
        r#"
        const listed = new Set(Intl.supportedValuesOf("timeZone"));
        for (const alias of ["Asia/Calcutta", "US/Pacific", "Etc/GMT", "Etc/UTC", "GMT", "CET", "Iceland", "Zulu"]) {
          assertSame(listed.has(alias), false, alias + " is not listed");
          assertSame(Temporal.Now.plainDateISO(alias) instanceof Temporal.PlainDate, true, "Now accepts " + alias);
          assertSame(Temporal.Instant.from("2020-01-01T00:00Z").toZonedDateTimeISO(alias).timeZoneId, alias, "instant in " + alias);
          assertSame(Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: alias }).timeZoneId, alias, "bag in " + alias);
          assertSame(Temporal.ZonedDateTime.from("2020-01-01T00:00[" + alias + "]").timeZoneId, alias, "string in " + alias);
        }
    "#,
    );
}
