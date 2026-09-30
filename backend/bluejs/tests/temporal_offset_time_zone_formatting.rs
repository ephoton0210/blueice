// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A `ZonedDateTime` (and an `Intl.DateTimeFormat`) in an offset time zone
//! names a zero offset "GMT", not "GMT+0"
//! (`intl402/Temporal/ZonedDateTime/prototype/toLocaleString/offset-time-zones.js`,
//! Phase 26 Stage 3, `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! The fix lives in `blueice-ecma402`
//! (`tests/date_time_format_zero_offset.rs` covers it per locale); this test
//! drives it through the public JavaScript surface the fixture uses.

use blueice_bluejs::{compile, parse, Value, Vm};

fn run(source: &str) {
    let script = format!(
        r#"(function() {{
const failures = [];
function same(label, actual, expected) {{
  if (actual !== expected) failures.push(label + ": expected " + JSON.stringify(expected) + " got " + JSON.stringify(actual));
}}
{source}
return failures.length === 0 ? "ok" : "\n" + failures.join("\n");
}})()"#
    );
    let value = Vm::default()
        .execute(&compile(&parse(&script).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{script}\n  -> {error:?}"));
    match value {
        Value::String(text) if text == "ok" => {}
        Value::String(text) => panic!("mismatches:{}", text.to_utf8().unwrap()),
        other => panic!("expected \"ok\" or a mismatch list, got {other:?}"),
    }
}

#[test]
fn a_zoned_date_time_at_a_zero_offset_prints_gmt() {
    run(r#"
      const at = (offset) => new Temporal.ZonedDateTime(0n, offset).toLocaleString("en");
      same("+00:00", at("+00:00"), "1/1/1970, 12:00:00 AM GMT");
      same("+01:00", at("+01:00"), "1/1/1970, 1:00:00 AM GMT+1");
      same("-01:00", at("-01:00"), "12/31/1969, 11:00:00 PM GMT-1");
      same("+05:30", at("+05:30"), "1/1/1970, 5:30:00 AM GMT+5:30");
      // the same three checks Test262's fixture makes
      const utc = at("+00:00");
      if (!(utc.includes("GMT") && !utc.includes("+") && !utc.includes("-"))) failures.push("fixture +00:00: " + utc);
    "#);
}

#[test]
fn intl_date_time_format_offset_styles_print_gmt_at_a_zero_offset() {
    run(r#"
      const zone = (timeZone, timeZoneName) =>
        new Intl.DateTimeFormat("en", { timeZone, timeZoneName, hour: "numeric" }).formatToParts(0)
          .find((part) => part.type === "timeZoneName").value;
      same("+00:00 short", zone("+00:00", "short"), "GMT");
      same("+00:00 longOffset", zone("+00:00", "longOffset"), "GMT");
      same("UTC shortOffset", zone("UTC", "shortOffset"), "GMT");
      same("UTC longOffset", zone("UTC", "longOffset"), "GMT");
      // real names and non-zero offsets are untouched
      same("UTC short", zone("UTC", "short"), "UTC");
      same("+01:00 longOffset", zone("+01:00", "longOffset"), "GMT+01:00");
      same("Asia/Kolkata shortOffset", zone("Asia/Kolkata", "shortOffset"), "GMT+5:30");
      // the offset in effect decides for a named zone: London is +00:00 in January only
      const london = (ms) => new Intl.DateTimeFormat("en", { timeZone: "Europe/London", timeZoneName: "shortOffset", hour: "numeric" })
        .formatToParts(ms).find((part) => part.type === "timeZoneName").value;
      same("London January", london(1577836800000), "GMT");
      same("London July", london(1593561600000), "GMT+1");
    "#);
}
