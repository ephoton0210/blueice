// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned CLDR regional data used by `Intl.Locale` information methods.
//!
//! The table is generated from Unicode CLDR JSON 48.2.1, commit
//! `26a79cb42bfcc90def764102aa2af126d9ef3108`, by
//! `../../tools/generate_cldr_locale_information.mjs`. It combines CLDR
//! calendar preferences, time data, week data, and BCP-47 timezone aliases.
//! Derived data is under Unicode License V3; see `unit_patterns/LICENSE-CLDR`.

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::Read, sync::OnceLock};

const CLDR_CALENDAR_REGION_COUNT: usize = 52;
const CLDR_HOUR_CYCLE_RULE_COUNT: usize = 276;
const CLDR_WEEK_REGION_COUNT: usize = 151;
const CLDR_TIME_ZONE_REGION_COUNT: usize = 247;
const CLDR_TIME_ZONE_COUNT: usize = 419;

#[derive(Debug)]
struct WeekData {
    first_day: u8,
    weekend: Vec<u8>,
}

struct PinnedLocaleInformation {
    calendars: HashMap<String, Vec<String>>,
    hour_cycles: HashMap<String, String>,
    weeks: HashMap<String, WeekData>,
    time_zones: HashMap<String, Vec<String>>,
}

static PINNED_LOCALE_INFORMATION: OnceLock<PinnedLocaleInformation> = OnceLock::new();

fn pinned() -> &'static PinnedLocaleInformation {
    PINNED_LOCALE_INFORMATION.get_or_init(|| {
        let encoded = include_str!("locale_information_data.b64")
            .lines()
            .collect::<String>();
        let compressed = STANDARD
            .decode(encoded)
            .expect("embedded CLDR locale-information data must be valid base64");
        let mut rows = String::new();
        GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut rows)
            .expect("embedded CLDR locale-information data must be valid gzip");

        let mut calendars = HashMap::new();
        let mut hour_cycles = HashMap::new();
        let mut weeks = HashMap::new();
        let mut time_zones = HashMap::new();
        for line in rows.lines() {
            let mut fields = line.splitn(3, '\t');
            let kind = fields
                .next()
                .expect("embedded CLDR locale-information row has a kind");
            let key = fields
                .next()
                .expect("embedded CLDR locale-information row has a key");
            let value = STANDARD
                .decode(
                    fields
                        .next()
                        .expect("embedded CLDR locale-information row has a value"),
                )
                .expect("embedded CLDR locale-information value must be valid base64");
            let value = String::from_utf8(value)
                .expect("embedded CLDR locale-information value must be UTF-8");
            match kind {
                "calendar" => assert!(
                    calendars
                        .insert(
                            key.to_owned(),
                            value.split(',').map(str::to_owned).collect()
                        )
                        .is_none(),
                    "embedded CLDR locale-information data has duplicate calendar region"
                ),
                "hour" => assert!(
                    hour_cycles.insert(key.to_owned(), value).is_none(),
                    "embedded CLDR locale-information data has duplicate hour-cycle rule"
                ),
                "week" => {
                    let (first_day, weekend) = value
                        .split_once('|')
                        .expect("embedded CLDR week data has a separator");
                    let first_day = first_day
                        .parse::<u8>()
                        .expect("embedded CLDR week data has a numeric first day");
                    let weekend = weekend
                        .split(',')
                        .map(|day| {
                            day.parse::<u8>()
                                .expect("embedded CLDR week data has numeric weekend days")
                        })
                        .collect::<Vec<_>>();
                    assert!(
                        (1..=7).contains(&first_day)
                            && weekend.iter().all(|day| (1..=7).contains(day)),
                        "embedded CLDR week data contains valid weekday numbers"
                    );
                    assert!(
                        weeks
                            .insert(key.to_owned(), WeekData { first_day, weekend })
                            .is_none(),
                        "embedded CLDR locale-information data has duplicate week region"
                    );
                }
                "timeZone" => assert!(
                    time_zones
                        .insert(
                            key.to_owned(),
                            value.split(',').map(str::to_owned).collect()
                        )
                        .is_none(),
                    "embedded CLDR locale-information data has duplicate time-zone region"
                ),
                _ => panic!("embedded CLDR locale-information row has an unknown kind"),
            }
        }
        assert_eq!(
            calendars.len(),
            CLDR_CALENDAR_REGION_COUNT,
            "embedded CLDR calendar preferences must retain every region"
        );
        assert_eq!(
            hour_cycles.len(),
            CLDR_HOUR_CYCLE_RULE_COUNT,
            "embedded CLDR time data must retain every rule"
        );
        assert_eq!(
            weeks.len(),
            CLDR_WEEK_REGION_COUNT,
            "embedded CLDR week data must retain every region"
        );
        assert_eq!(
            time_zones.len(),
            CLDR_TIME_ZONE_REGION_COUNT,
            "embedded CLDR BCP-47 time zones must retain every region"
        );
        assert_eq!(
            time_zones.values().map(Vec::len).sum::<usize>(),
            CLDR_TIME_ZONE_COUNT,
            "embedded CLDR BCP-47 time zones must retain every zone"
        );
        PinnedLocaleInformation {
            calendars,
            hour_cycles,
            weeks,
            time_zones,
        }
    })
}

pub(super) fn calendars_for_region(region: &str) -> &'static [String] {
    pinned()
        .calendars
        .get(region)
        .or_else(|| pinned().calendars.get("001"))
        .map(Vec::as_slice)
        .expect("CLDR locale-information data has a world calendar fallback")
}

pub(super) fn default_calendar(region: &str) -> &'static str {
    calendars_for_region(region)
        .first()
        .map(String::as_str)
        .expect("CLDR calendar preferences have a default calendar")
}

pub(super) fn hour_cycle_for_locale(language: &str, region: &str) -> &'static str {
    let data = pinned();
    let language_region = format!(
        "{}-{}",
        language.to_ascii_lowercase(),
        region.to_ascii_lowercase()
    );
    data.hour_cycles
        .get(&language_region)
        .or_else(|| data.hour_cycles.get(&language.to_ascii_lowercase()))
        .or_else(|| data.hour_cycles.get(&region.to_ascii_lowercase()))
        .or_else(|| data.hour_cycles.get("001"))
        .map(String::as_str)
        .expect("CLDR time data has a world hour-cycle fallback")
}

pub(super) fn time_zones_for_region(region: &str) -> &'static [String] {
    pinned()
        .time_zones
        .get(region)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(super) fn week_data_for_region(region: &str) -> (u8, &'static [u8]) {
    let data = pinned();
    let week = data
        .weeks
        .get(region)
        .or_else(|| data.weeks.get("001"))
        .expect("CLDR week data has a world fallback");
    (week.first_day, week.weekend.as_slice())
}

#[cfg(test)]
pub(super) fn coverage() -> (usize, usize, usize, usize, usize) {
    let data = pinned();
    (
        data.calendars.len(),
        data.hour_cycles.len(),
        data.weeks.len(),
        data.time_zones.len(),
        data.time_zones.values().map(Vec::len).sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_the_complete_pinned_cldr_regional_dataset() {
        assert_eq!(coverage(), (52, 276, 151, 247, 419));
        assert_eq!(calendars_for_region("TW"), ["gregory", "roc", "chinese"]);
        assert_eq!(hour_cycle_for_locale("fr", "CA"), "h23");
        assert_eq!(week_data_for_region("AF"), (6, &[4, 5][..]));
        assert_eq!(
            time_zones_for_region("DE"),
            ["Europe/Berlin", "Europe/Busingen"]
        );
    }
}
