// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral Temporal time-zone identifier resolution and UTC-offset
//! lookup (Phase 26 Stage 1 Track E).
//!
//! Every function here is plain Rust with no `Value`/heap/Realm coupling —
//! directly unit-testable without a VM, per Phase 26's foundation/adapter
//! split (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! # Where the transition data comes from
//!
//! Track E's open question was whether `icu_time` (a pinned dependency of
//! `blueice-ecma402`) supplies real historical IANA transition rules. It does
//! not, and ICU4X says so itself: its only offset-calculating API,
//! `icu_time::zone::VariantOffsetsCalculator`, is
//! `#[deprecated(note = "this API is a bad approximation of a time zone
//! database")]`, and its `ZoneNameTimestamp` key documents that "In ICU4X, we
//! deal with _time zone display names_" at a coarse 15-minute granularity
//! limited to post-1970 — enough to know when a region switched from Eastern
//! to Central Time for *naming* purposes, not what offset was in effect at a
//! given instant.
//!
//! The real transition history in this workspace is the pinned, bundled IANA
//! Time Zone Database behind `jiff`/`jiff-tzdb`, which
//! `blueice_ecma402::DateTimeFormat` already resolves arbitrary-instant
//! offsets from (`backend/ecma402/src/date_time_format.rs`'s
//! `datetime_from_milliseconds`). This module reads the same pinned database
//! directly, so a Temporal offset and an `Intl.DateTimeFormat` offset for the
//! same zone and instant cannot disagree.

use super::epoch::{self, CivilDate, CivilTime};
use jiff::{tz::TimeZoneDatabase, Timestamp};
use num_bigint::BigInt;
use std::sync::OnceLock;

/// Nanoseconds in one day.
const DAY_NANOSECONDS: i64 = 86_400_000_000_000;

/// Seconds in one complete Gregorian 400-year cycle. Calendar dates, weekdays
/// and recurring IANA rules all repeat over this span, which is what makes it a
/// sound projection for instants outside Jiff's own civil range.
const GREGORIAN_400_YEAR_SECONDS: i64 = 146_097 * 86_400;

/// A resolved Temporal time zone: either a fixed UTC offset or a named IANA
/// zone. Temporal has no user-pluggable time-zone protocol (the current spec
/// revision dropped `Temporal.TimeZone` as an object type entirely), so this
/// closed pair is the whole domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TimeZone {
    /// A fixed offset in whole minutes east of UTC. Temporal's
    /// `TimeZoneIdentifier` production is `UTCOffset[~SubMinutePrecision]`,
    /// so a time-zone *identifier* never carries seconds — unlike the offsets
    /// that historical IANA data itself reports.
    Offset(i32),
    /// A named IANA zone, in the ASCII-case-normalized spelling the pinned
    /// database records. Temporal's `ToTemporalTimeZoneIdentifier` returns
    /// this `[[Identifier]]`, deliberately *not* the primary identifier a
    /// `Link` line would resolve to, so `Asia/Calcutta` stays
    /// `Asia/Calcutta`.
    Iana(&'static str),
}

/// Temporal's `disambiguation` option: which instant a local wall-clock time
/// means when a zone transition makes it ambiguous (a repeated hour) or
/// nonexistent (a skipped hour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disambiguation {
    Compatible,
    Earlier,
    Later,
    Reject,
}

/// A local date-time this zone does not map to exactly one instant, and which
/// the requested `disambiguation` refused to choose between. The spec raises a
/// `RangeError` for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AmbiguousLocalTime;

/// `ToTemporalDisambiguation`'s value set. `None` means the caller should
/// raise the spec's `RangeError` for an unrecognized option value.
pub(crate) fn parse_disambiguation(value: &str) -> Option<Disambiguation> {
    Some(match value {
        "compatible" => Disambiguation::Compatible,
        "earlier" => Disambiguation::Earlier,
        "later" => Disambiguation::Later,
        "reject" => Disambiguation::Reject,
        _ => return None,
    })
}

/// `ToTemporalTimeZoneIdentifier`'s string path: resolves either a bare
/// `TimeZoneIdentifier` or the time-zone portion of a fuller ISO date-time
/// string. `None` is the spec's `RangeError`.
pub(crate) fn parse_identifier(source: &str) -> Option<TimeZone> {
    parse_bare_identifier(source).or_else(|| parse_from_date_time_string(source))
}

/// `TimeZoneIdentifier ::: UTCOffset[~SubMinutePrecision] | TimeZoneIANAName`
fn parse_bare_identifier(source: &str) -> Option<TimeZone> {
    if matches!(source.as_bytes().first(), Some(b'+' | b'-')) {
        return parse_minute_offset(source).map(TimeZone::Offset);
    }
    named(source)
}

/// Looks an IANA name up in the pinned bundled database, case-insensitively,
/// returning its recorded spelling. An unknown name is `None` — Temporal
/// never falls back to an offset that appeared alongside it in a string.
fn named(name: &str) -> Option<TimeZone> {
    jiff_tzdb::get(name).map(|(canonical, _)| TimeZone::Iana(canonical))
}

/// `UTCOffset[~SubMinutePrecision]`: `±HH`, `±HH:MM` or `±HHMM`, and nothing
/// finer. A seconds component — even an explicitly zero one — makes the text
/// invalid *as an identifier*, which is why `-07:00:00` is rejected while
/// `-07:00` is accepted.
fn parse_minute_offset(source: &str) -> Option<i32> {
    let sign = match source.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return None,
    };
    let digits = &source[1..];
    let (hour, minute) = match digits.len() {
        2 => (digits, "0"),
        4 => (&digits[..2], &digits[2..]),
        5 if digits.as_bytes()[2] == b':' => (&digits[..2], &digits[3..]),
        _ => return None,
    };
    if !hour
        .bytes()
        .chain(minute.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    (hour <= 23 && minute <= 59).then_some(sign * (hour * 60 + minute))
}

/// The fallback path: an ISO date-time string whose time-zone part names the
/// zone. A bracketed time-zone annotation always wins over any offset in the
/// string itself; failing that, `Z` means UTC and a bare minute-precision
/// offset is a fixed-offset zone. A date-time with no zone part at all is not
/// a time zone.
fn parse_from_date_time_string(source: &str) -> Option<TimeZone> {
    // A negative-zero extended year is invalid everywhere in Temporal's
    // grammar, and `str::parse` would otherwise read `-000000` as year 0.
    if source.starts_with("-000000") || source.starts_with("\u{2212}000000") {
        return None;
    }
    // Rejects text that is not an ISO date-time at all (`"19761118"`, `""`,
    // an arbitrary word) before looking for a zone inside it.
    super::iso::parse_date(source)?;
    if let Some(annotation) = time_zone_annotation(source) {
        return parse_bare_identifier(annotation);
    }
    let date_end = source
        .find(['T', 't', '[', 'Z', 'z'])
        .unwrap_or(source.len());
    let time = source[date_end..].strip_prefix(['T', 't'])?;
    // Any remaining bracket group is a calendar or unknown annotation (a
    // time-zone one would have been taken above), so it is not part of the
    // zone designator.
    let time = time.split('[').next().unwrap_or(time);
    let index = time.char_indices().find_map(|(index, character)| {
        matches!(character, 'Z' | 'z' | '+' | '-').then_some(index)
    })?;
    let zone = &time[index..];
    if matches!(zone.as_bytes().first(), Some(b'Z' | b'z')) {
        return (zone.len() == 1).then_some(TimeZone::Iana("UTC"));
    }
    parse_minute_offset(zone).map(TimeZone::Offset)
}

/// Returns the body of a leading `TimeZoneAnnotation`, if the first bracket
/// group is one. A `key=value` body is a calendar/unknown annotation, not a
/// time zone.
fn time_zone_annotation(source: &str) -> Option<&str> {
    let start = source.find('[')?;
    let rest = &source[start + 1..];
    let end = rest.find(']')?;
    let body = &rest[..end];
    let body = body.strip_prefix('!').unwrap_or(body);
    (!body.contains('=')).then_some(body)
}

/// The one pinned IANA database this engine resolves offsets from, shared
/// with `blueice_ecma402::DateTimeFormat`'s own zone handling.
fn database() -> &'static TimeZoneDatabase {
    static DATABASE: OnceLock<TimeZoneDatabase> = OnceLock::new();
    DATABASE.get_or_init(TimeZoneDatabase::bundled)
}

/// Converts epoch seconds into a Jiff `Timestamp`, or `None` when the instant
/// lies outside Jiff's own civil range.
///
/// Two deliberate details. The bounds are checked here rather than relying on
/// `Timestamp::from_second`'s error, because Jiff trips an internal debug
/// assertion before returning one for inputs this far out. And the caller
/// always floors nanoseconds to whole seconds first, because a Jiff
/// `Timestamp` stores its second and sub-second parts with a *shared* sign:
/// for a pre-1970 instant with a sub-second part, its second field is the
/// ceiling, so an offset looked up from it would come from the wrong side of a
/// transition falling in that second. Offsets only ever change on a second
/// boundary, so flooring first is both exact and sufficient.
fn jiff_timestamp(seconds: i64) -> Option<Timestamp> {
    (seconds >= Timestamp::MIN.as_second() && seconds <= Timestamp::MAX.as_second())
        .then(|| Timestamp::from_second(seconds).expect("the range was just checked"))
}

impl TimeZone {
    /// `ToTemporalTimeZoneIdentifier`'s result, as stored in a
    /// `ZonedDateTime`'s `[[TimeZone]]` slot and returned by `timeZoneId`.
    pub(crate) fn identifier(&self) -> String {
        match self {
            Self::Offset(minutes) => {
                let sign = if *minutes < 0 { '-' } else { '+' };
                let magnitude = minutes.abs();
                format!("{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
            }
            Self::Iana(name) => (*name).to_string(),
        }
    }

    /// `TimeZoneEquals`: whether `self` and `other` denote the same time
    /// zone -- an offset identifier compares by exact minutes, and a named
    /// identifier compares by ECMA-402 *primary identifier*, not by spelling
    /// or by the zone's data. `Self::Iana`'s own stored identifier deliberately
    /// preserves whichever alias was written (`Asia/Calcutta` stays
    /// `Asia/Calcutta`, see that variant's doc comment) so a `ZonedDateTime`'s
    /// `timeZoneId` getter can report it back unchanged.
    ///
    /// Two names have the same primary identifier when one is a
    /// backward-compatibility alias of the other (`Asia/Calcutta` and
    /// `Asia/Kolkata`; every spelling of UTC and GMT). Zones that the tz
    /// database merely *stores* with identical data are distinct
    /// (`Africa/Accra` and `Africa/Abidjan`, both listed in `zone.tab`), so
    /// comparing TZif bytes -- the earlier approach -- is wrong in both
    /// directions. See [`blueice_ecma402::primary_time_zone_identifier`].
    pub(crate) fn time_zone_equals(&self, other: &TimeZone) -> bool {
        match (self, other) {
            (Self::Offset(a), Self::Offset(b)) => a == b,
            (Self::Iana(a), Self::Iana(b)) => {
                a == b
                    || blueice_ecma402::primary_time_zone_identifier(a)
                        .zip(blueice_ecma402::primary_time_zone_identifier(b))
                        .is_some_and(|(one, two)| one == two)
            }
            _ => false,
        }
    }

    /// `GetOffsetNanosecondsFor`: the UTC offset this zone was actually
    /// observing at `epoch_nanoseconds`, from real transition data for a
    /// named zone.
    pub(crate) fn offset_nanoseconds_for(&self, epoch_nanoseconds: &BigInt) -> i64 {
        let name = match self {
            Self::Offset(minutes) => return i64::from(*minutes) * 60_000_000_000,
            Self::Iana(name) => name,
        };
        let zone = database()
            .get(name)
            .expect("a named zone only ever comes from this same pinned database");
        // Temporal's whole Instant range is ~±8.64e21 nanoseconds (~±8.64e12
        // seconds), so every value reaching here — and every one-day or
        // two-day probe around it — floors into `i64` seconds with many orders
        // of magnitude to spare.
        let nanoseconds = i128::try_from(epoch_nanoseconds)
            .expect("Instant-range nanoseconds fit in i128 with room for a two-day probe");
        let epoch_seconds = i64::try_from(nanoseconds.div_euclid(1_000_000_000))
            .expect("Instant-range seconds fit in i64");
        // Jiff's civil `Timestamp` intentionally stops at ISO year ±9999,
        // while Temporal's Instant range reaches ±273,972 years. A
        // fixed-offset IANA zone (`Etc/GMT+5`, `UTC`) stays correct outside
        // that range; anything else is projected onto the equivalent position
        // of the Gregorian 400-year cycle, which preserves the ISO date, time
        // and weekday the recurring IANA rule keys off. This mirrors
        // `blueice_ecma402`'s `datetime_from_milliseconds` exactly, so the two
        // cannot drift.
        let seconds = if let Some(timestamp) = jiff_timestamp(epoch_seconds) {
            zone.to_offset(timestamp).seconds()
        } else if let Ok(offset) = zone.to_fixed_offset() {
            offset.seconds()
        } else {
            let projected = epoch_seconds.rem_euclid(GREGORIAN_400_YEAR_SECONDS);
            let timestamp = jiff_timestamp(projected)
                .expect("a 400-year Gregorian cycle fits Jiff's Timestamp range");
            zone.to_offset(timestamp).seconds()
        };
        i64::from(seconds) * 1_000_000_000
    }

    /// `GetPossibleEpochNanoseconds`: every instant whose local time in this
    /// zone is exactly `date`/`time` — two of them across a fall-back
    /// transition, none inside a spring-forward gap, one otherwise.
    ///
    /// The candidate offsets are probed a day either side of the local time
    /// read as if it were UTC, which brackets any single transition, and each
    /// candidate is kept only if the zone really does observe that offset at
    /// it.
    pub(crate) fn possible_epoch_nanoseconds(
        &self,
        date: CivilDate,
        time: CivilTime,
    ) -> Vec<BigInt> {
        let local = epoch::nanoseconds_since_epoch(date, time, 0);
        if let Self::Offset(minutes) = self {
            return vec![local - BigInt::from(i64::from(*minutes) * 60_000_000_000)];
        }
        let day = BigInt::from(DAY_NANOSECONDS);
        let mut found: Vec<BigInt> = Vec::new();
        for probe in [&local - &day, &local + &day] {
            let offset = self.offset_nanoseconds_for(&probe);
            let candidate = &local - BigInt::from(offset);
            if self.offset_nanoseconds_for(&candidate) == offset && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
        found.sort();
        found
    }

    /// `GetStartOfDay`: the first instant of `date` in this zone.
    ///
    /// Usually that is local midnight, but a zone can skip midnight entirely:
    /// Toronto went straight from 1919-03-30T23:30 to 1919-03-31T00:30, so
    /// 1919-03-31 begins at the transition instant, whose local time is 00:30.
    /// That instant is *earlier* than what `"compatible"` disambiguation of
    /// midnight picks (01:00 there), which is why this is deliberately not
    /// `epoch_nanoseconds_for(date, midnight, Compatible)`.
    pub(crate) fn start_of_day(&self, date: CivilDate) -> BigInt {
        const MIDNIGHT: CivilTime = (0, 0, 0, 0, 0, 0);
        let possible = self.possible_epoch_nanoseconds(date, MIDNIGHT);
        if let Some(first) = possible.first() {
            return first.clone();
        }
        // Midnight does not exist. The first instant of the day is the
        // earliest one whose local wall clock has reached midnight, i.e. the
        // transition itself. `instant + offset(instant)` is the local clock
        // read as an epoch value, so bisecting on it finds that instant
        // exactly, to the nanosecond, without needing a transition-list API.
        let local = epoch::nanoseconds_since_epoch(date, MIDNIGHT, 0);
        let window = BigInt::from(DAY_NANOSECONDS) * 2_u32;
        let reached = |instant: &BigInt| {
            instant + BigInt::from(self.offset_nanoseconds_for(instant)) >= local
        };
        let mut before = &local - &window;
        let mut after = &local + &window;
        while &after - &before > BigInt::from(1) {
            let middle = (&before + &after) / 2_u32;
            if reached(&middle) {
                after = middle;
            } else {
                before = middle;
            }
        }
        after
    }

    /// `GetNamedTimeZoneNextTransition`/`GetNamedTimeZonePreviousTransition`,
    /// generalized over direction: the next (`forward = true`) or previous
    /// (`forward = false`) instant, strictly after/before
    /// `epoch_nanoseconds`, at which this zone's real UTC offset changes.
    ///
    /// `None` when there is no such instant — either because `self` is a
    /// fixed-offset zone (`Temporal.ZonedDateTime.prototype
    /// .getTimeZoneTransition`'s own spec: an offset zone never has
    /// transitions), or because `epoch_nanoseconds` itself falls outside
    /// Jiff's representable range (the same ISO ±9999-year window every
    /// other real-transition-data lookup in this module already stays
    /// inside — a Temporal `Instant` this far out has no real IANA data to
    /// query in the first place, so there is nothing to project this
    /// specific lookup onto, unlike [`Self::offset_nanoseconds_for`]'s own
    /// periodic-cycle projection for a plain offset query).
    ///
    /// Delegates to `jiff::tz::TimeZone::following`/`preceding`, which read
    /// real transition entries directly from the same pinned TZif data
    /// `offset_nanoseconds_for` already resolves offsets from, keeping only the
    /// entries that actually change the total UTC offset
    /// (`rule-change-without-offset-transition.js`'s own Europe/London and
    /// America/Anchorage cases).
    pub(crate) fn adjacent_transition(
        &self,
        epoch_nanoseconds: &BigInt,
        forward: bool,
    ) -> Option<BigInt> {
        let name = match self {
            Self::Offset(_) => return None,
            Self::Iana(name) => name,
        };
        let zone = database()
            .get(name)
            .expect("a named zone only ever comes from this same pinned database");
        let nanoseconds = i128::try_from(epoch_nanoseconds).ok()?;
        // Offsets only ever change on a whole-second boundary, and Jiff's
        // `following`/`preceding` are unreliable for a pre-1970 instant with a
        // sub-second part (its `Timestamp` stores the second rounded toward
        // zero, the sharp edge `jiff_timestamp` documents), so ask about a
        // whole second instead: the one at or just below `epoch_nanoseconds`
        // when looking forward -- the next transition after 23:59:59.999999999
        // is the same as after 23:59:59 -- and the one at or just above it when
        // looking back, where a transition at the second *before* a fractional
        // instant is already in the past.
        let (seconds, fraction) = (
            nanoseconds.div_euclid(1_000_000_000),
            nanoseconds.rem_euclid(1_000_000_000),
        );
        let seconds = if forward || fraction == 0 {
            seconds
        } else {
            seconds + 1
        };
        let timestamp = jiff_timestamp(i64::try_from(seconds).ok()?)?;
        // The TZif data records every change of *rule*, including ones that
        // leave the total UTC offset alone (Europe/London's 1968 switch from
        // "BST as daylight time" to "British Standard Time", both +01:00).
        // Those are not `GetNamedTimeZoneNextTransition`/`Previous
        // Transition`s, so walk on to the first entry whose offset really
        // differs from the one just before it.
        let changes_offset = |transition: &jiff::tz::TimeZoneTransition<'_>| {
            let at = BigInt::from(transition.timestamp().as_nanosecond());
            self.offset_nanoseconds_for(&(&at - 1)) != self.offset_nanoseconds_for(&at)
        };
        let transition = if forward {
            zone.following(timestamp).find(changes_offset)
        } else {
            zone.preceding(timestamp).find(changes_offset)
        }?;
        Some(BigInt::from(transition.timestamp().as_nanosecond()))
    }

    /// `GetEpochNanosecondsFor`: resolves a local date-time to one instant,
    /// applying `disambiguation` when the zone leaves it ambiguous or skips
    /// it entirely.
    pub(crate) fn epoch_nanoseconds_for(
        &self,
        date: CivilDate,
        time: CivilTime,
        disambiguation: Disambiguation,
    ) -> Result<BigInt, AmbiguousLocalTime> {
        let possible = self.possible_epoch_nanoseconds(date, time);
        if possible.len() == 1 {
            return Ok(possible
                .into_iter()
                .next()
                .expect("length was just checked"));
        }
        if disambiguation == Disambiguation::Reject {
            return Err(AmbiguousLocalTime);
        }
        if !possible.is_empty() {
            // An ambiguous local time: it happened twice. `"compatible"`
            // matches legacy `Date`, which takes the earlier of the two.
            let chosen = if disambiguation == Disambiguation::Later {
                possible.last()
            } else {
                possible.first()
            };
            return Ok(chosen.expect("a non-empty list has both ends").clone());
        }
        // A skipped local time: shift it by the size of the gap (backwards for
        // "earlier", forwards for "compatible"/"later") and resolve the
        // shifted time, which is no longer inside the gap.
        let local = epoch::nanoseconds_since_epoch(date, time, 0);
        let day = BigInt::from(DAY_NANOSECONDS);
        let before = self.offset_nanoseconds_for(&(&local - &day));
        let after = self.offset_nanoseconds_for(&(&local + &day));
        let gap = after - before;
        let shift = if disambiguation == Disambiguation::Earlier {
            -gap
        } else {
            gap
        };
        let (date, time) = epoch::instant_fields(&(local + BigInt::from(shift)));
        let possible = self.possible_epoch_nanoseconds(date, time);
        let chosen = if disambiguation == Disambiguation::Earlier {
            possible.first()
        } else {
            possible.last()
        };
        chosen.cloned().ok_or(AmbiguousLocalTime)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset_minutes(source: &str) -> Option<i32> {
        match parse_identifier(source) {
            Some(TimeZone::Offset(minutes)) => Some(minutes),
            _ => None,
        }
    }

    fn iana(source: &str) -> Option<&'static str> {
        match parse_identifier(source) {
            Some(TimeZone::Iana(name)) => Some(name),
            _ => None,
        }
    }

    #[test]
    fn resolves_bare_named_zones_case_insensitively_keeping_the_recorded_spelling() {
        assert_eq!(iana("UTC"), Some("UTC"));
        assert_eq!(iana("uTc"), Some("UTC"));
        assert_eq!(iana("america/new_york"), Some("America/New_York"));
        // Temporal keeps the requested (non-primary) identifier rather than
        // resolving a `Link` line to its target.
        assert_eq!(iana("Asia/Calcutta"), Some("Asia/Calcutta"));
        assert_eq!(parse_identifier("Mars/Olympus_Mons"), None);
        assert_eq!(parse_identifier("America/Nonexistent"), None);
        assert_eq!(parse_identifier(""), None);
    }

    fn zone(name: &str) -> TimeZone {
        parse_identifier(name).unwrap_or_else(|| panic!("{name} is a bundled zone"))
    }

    #[test]
    fn time_zone_equality_follows_primary_identifiers_not_spelling_or_data() {
        // A backward-compatibility alias is the same zone as its target, in
        // either order and whatever the spelling's case.
        assert!(zone("Asia/Calcutta").time_zone_equals(&zone("Asia/Kolkata")));
        assert!(zone("asia/kolkata").time_zone_equals(&zone("ASIA/CALCUTTA")));
        assert!(zone("Etc/GMT+0").time_zone_equals(&zone("UTC")));
        assert!(zone("Zulu").time_zone_equals(&zone("Etc/GMT")));
        // Zones the tz database stores with identical data are still distinct
        // when both are in `zone.tab`.
        assert!(!zone("Africa/Accra").time_zone_equals(&zone("Africa/Abidjan")));
        assert!(!zone("Europe/Amsterdam").time_zone_equals(&zone("Europe/Brussels")));
        assert!(!zone("Etc/GMT+1").time_zone_equals(&zone("UTC")));
        // Offsets compare by minutes and never equal a named zone.
        assert!(zone("+01:00").time_zone_equals(&zone("+01:00")));
        assert!(!zone("+01:00").time_zone_equals(&zone("+02:00")));
        assert!(!zone("+00:00").time_zone_equals(&zone("UTC")));
        assert!(!zone("UTC").time_zone_equals(&zone("+00:00")));
    }

    #[test]
    fn a_name_unknown_to_the_table_only_equals_itself() {
        // Not reachable through `parse_identifier`, which only yields bundled
        // names; the comparison still fails closed rather than panicking.
        let unknown = TimeZone::Iana("Mars/Olympus_Mons");
        assert!(unknown.time_zone_equals(&unknown));
        assert!(!unknown.time_zone_equals(&zone("UTC")));
        assert!(!zone("UTC").time_zone_equals(&unknown));
    }

    #[test]
    fn resolves_bare_offsets_at_minute_precision_only() {
        assert_eq!(offset_minutes("+01:30"), Some(90));
        assert_eq!(offset_minutes("-05:00"), Some(-300));
        assert_eq!(offset_minutes("+0146"), Some(106));
        assert_eq!(offset_minutes("-07"), Some(-420));
        assert_eq!(parse_identifier("-12:12:59.9"), None);
        assert_eq!(parse_identifier("+24:00"), None);
        assert_eq!(parse_identifier("+01:60"), None);
        assert_eq!(parse_identifier("+ab:cd"), None);
        // The `offset_minutes`/`iana` test helpers' own fallback arms: an
        // input that resolves to the *other* variant (or to neither) is
        // `None` through the same helper, not a mismatched value.
        assert_eq!(offset_minutes("UTC"), None);
        assert_eq!(offset_minutes(""), None);
        assert_eq!(iana("+01:30"), None);
    }

    /// [`parse_minute_offset`]'s own leading-sign check is defensive: both
    /// of its current callers already only call it once the first byte is
    /// known to be `+`/`-`, so this is the one place that guard itself is
    /// exercised directly, including on an empty string (no first byte at
    /// all).
    #[test]
    fn parse_minute_offset_rejects_input_with_no_leading_sign() {
        assert_eq!(parse_minute_offset("07:00"), None);
        assert_eq!(parse_minute_offset(""), None);
    }

    #[test]
    fn formats_offset_identifiers_back_to_the_canonical_spelling() {
        assert_eq!(TimeZone::Offset(90).identifier(), "+01:30");
        assert_eq!(TimeZone::Offset(-300).identifier(), "-05:00");
        assert_eq!(TimeZone::Offset(0).identifier(), "+00:00");
        assert_eq!(TimeZone::Offset(-1).identifier(), "-00:01");
        assert_eq!(TimeZone::Iana("UTC").identifier(), "UTC");
    }

    #[test]
    fn takes_the_time_zone_annotation_over_any_offset_in_the_same_string() {
        // Cases taken directly from the pinned Test262 corpus's
        // built-ins/Temporal/Instant/prototype/toZonedDateTimeISO/ fixtures.
        assert_eq!(iana("2021-08-19T17:30[UTC]"), Some("UTC"));
        assert_eq!(iana("2021-08-19T17:30Z[UTC]"), Some("UTC"));
        assert_eq!(iana("2021-08-19T17:30-07:00[UTC]"), Some("UTC"));
        assert_eq!(
            offset_minutes("2021-08-19T17:30:45.123456789-12:12[+01:46]"),
            Some(106)
        );
        // A leap second is valid in the date-time portion, but not in the
        // annotation that names the zone.
        assert_eq!(iana("2016-12-31T23:59:60+00:00[UTC]"), Some("UTC"));
        assert_eq!(
            parse_identifier("2021-08-19T17:30:45.123456789+23:59[+23:59:60]"),
            None
        );
        assert_eq!(
            parse_identifier("1970-01-01T00:00+01:00[America/Nonexistent]"),
            None
        );
        assert_eq!(
            parse_identifier("2021-08-19T17:30:45.123456789-12:12:59.9[-12:12:59.9]"),
            None
        );
        // A calendar annotation is not a time-zone annotation.
        assert_eq!(iana("2021-08-19T17:30Z[u-ca=hebrew]"), Some("UTC"));
        // A time-zone annotation may carry the critical flag.
        assert_eq!(
            iana("2021-08-19T17:30[!Europe/Madrid]"),
            Some("Europe/Madrid")
        );
        assert_eq!(offset_minutes("2021-08-19T17:30Z[!-05:00]"), Some(-300));
        // An unterminated bracket is not an annotation, and leaves nothing
        // else that could name a zone.
        assert_eq!(parse_identifier("2021-08-19T17:30[UTC"), None);
    }

    #[test]
    fn falls_back_to_the_designator_or_offset_when_no_annotation_names_the_zone() {
        assert_eq!(iana("2021-08-19T17:30Z"), Some("UTC"));
        assert_eq!(offset_minutes("2021-08-19T17:30-07:00"), Some(-420));
        assert_eq!(parse_identifier("2021-08-19T17:30"), None);
        assert_eq!(parse_identifier("2000-05-02"), None);
        for source in [
            "2021-08-19T17:30-07:00:01",
            "2021-08-19T17:30-07:00:00",
            "2021-08-19T17:30-07:00:00.000000000",
        ] {
            assert_eq!(parse_identifier(source), None, "{source}");
        }
    }

    #[test]
    fn rejects_a_negative_zero_extended_year_and_non_date_text() {
        assert_eq!(parse_identifier("-000000-10-31T17:45Z"), None);
        assert_eq!(parse_identifier("-000000-10-31T17:45+00:00[UTC]"), None);
        assert_eq!(parse_identifier("19761118"), None);
        assert_eq!(parse_identifier("obviously bad"), None);
    }

    #[test]
    fn maps_option_strings_to_the_four_disambiguation_modes() {
        assert_eq!(
            parse_disambiguation("compatible"),
            Some(Disambiguation::Compatible)
        );
        assert_eq!(
            parse_disambiguation("earlier"),
            Some(Disambiguation::Earlier)
        );
        assert_eq!(parse_disambiguation("later"), Some(Disambiguation::Later));
        assert_eq!(parse_disambiguation("reject"), Some(Disambiguation::Reject));
        assert_eq!(parse_disambiguation("EARLIER"), None);
        assert_eq!(parse_disambiguation(""), None);
    }

    #[test]
    fn reads_real_historical_transitions_rather_than_one_current_offset() {
        let new_york = TimeZone::Iana("America/New_York");
        // 2024-07-08T22:46:44Z is EDT (-4h); 2024-01-10T21:46:44Z is EST
        // (-5h). One zone, two different offsets — the capability
        // `icu_time`'s display-name data cannot provide.
        let summer = BigInt::from(1_720_480_004_i64) * 1_000_000_000_u32;
        let winter = BigInt::from(1_704_941_204_i64) * 1_000_000_000_u32;
        assert_eq!(
            new_york.offset_nanoseconds_for(&summer),
            -4 * 3_600_000_000_000
        );
        assert_eq!(
            new_york.offset_nanoseconds_for(&winter),
            -5 * 3_600_000_000_000
        );
        // A historical offset change, not merely a recurring DST rule:
        // the United States had no nationwide DST rule in 1900, and New York
        // was still on Local Mean Time (-4:56:02) before 1883-11-18.
        let eighteen_eighty = BigInt::from(-2_840_140_800_i64) * 1_000_000_000_u32;
        assert_eq!(
            new_york.offset_nanoseconds_for(&eighteen_eighty),
            -(4 * 3_600 + 56 * 60 + 2) * 1_000_000_000
        );
    }

    #[test]
    fn resolves_offsets_for_instants_outside_jiffs_own_civil_range() {
        // Temporal's Instant range reaches ±273,972 years, far past Jiff's
        // ISO ±9999 civil limit. A fixed-offset IANA zone stays exact; a
        // rule-based zone is projected onto the Gregorian 400-year cycle.
        let far_future = BigInt::from(8_640_000_000_000_000_i64) * 1_000_000;
        assert_eq!(TimeZone::Iana("UTC").offset_nanoseconds_for(&far_future), 0);
        let projected = TimeZone::Iana("America/New_York").offset_nanoseconds_for(&far_future);
        assert!(
            projected == -4 * 3_600_000_000_000 || projected == -5 * 3_600_000_000_000,
            "projected offset should still be a real New York offset, got {projected}",
        );
        let far_past = -(BigInt::from(8_640_000_000_000_000_i64) * 1_000_000_u32);
        let projected = TimeZone::Iana("Europe/Madrid").offset_nanoseconds_for(&far_past);
        assert!(
            projected == 3_600_000_000_000 || projected == 2 * 3_600_000_000_000,
            "projected offset should still be a real Madrid offset, got {projected}",
        );
    }

    #[test]
    fn a_fixed_offset_zone_has_exactly_one_possible_instant() {
        let zone = TimeZone::Offset(210);
        assert_eq!(
            zone.possible_epoch_nanoseconds((2019, 2, 16), (23, 45, 0, 0, 0, 0)),
            vec![BigInt::from(1_550_348_100_i64) * 1_000_000_000]
        );
        for disambiguation in [
            Disambiguation::Compatible,
            Disambiguation::Earlier,
            Disambiguation::Later,
            Disambiguation::Reject,
        ] {
            assert_eq!(
                zone.epoch_nanoseconds_for((2019, 2, 16), (23, 45, 0, 0, 0, 0), disambiguation),
                Ok(BigInt::from(1_550_348_100_i64) * 1_000_000_000),
            );
        }
    }

    #[test]
    fn disambiguates_a_repeated_local_hour_across_a_fall_back_transition() {
        // Expected values are the pinned Test262 fixture's own, from
        // intl402/Temporal/PlainDateTime/prototype/toZonedDateTime/dst-disambiguation.js.
        let zone = TimeZone::Iana("America/Los_Angeles");
        let date = (2000, 10, 29);
        let time = (1, 45, 0, 0, 0, 0);
        let earlier = BigInt::from(972_809_100_i64) * 1_000_000_000_u32;
        let later = BigInt::from(972_812_700_i64) * 1_000_000_000_u32;
        assert_eq!(
            zone.possible_epoch_nanoseconds(date, time),
            vec![earlier.clone(), later.clone()]
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Compatible),
            Ok(earlier.clone())
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Earlier),
            Ok(earlier)
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Later),
            Ok(later)
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Reject),
            Err(AmbiguousLocalTime)
        );
    }

    #[test]
    fn disambiguates_a_skipped_local_hour_across_a_spring_forward_gap() {
        let zone = TimeZone::Iana("America/Los_Angeles");
        let date = (2000, 4, 2);
        let time = (2, 30, 0, 0, 0, 0);
        assert!(zone.possible_epoch_nanoseconds(date, time).is_empty());
        let earlier = BigInt::from(954_667_800_i64) * 1_000_000_000_u32;
        let later = BigInt::from(954_671_400_i64) * 1_000_000_000_u32;
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Earlier),
            Ok(earlier)
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Later),
            Ok(later.clone())
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Compatible),
            Ok(later)
        );
        assert_eq!(
            zone.epoch_nanoseconds_for(date, time, Disambiguation::Reject),
            Err(AmbiguousLocalTime)
        );
    }

    #[test]
    fn a_day_whose_midnight_is_skipped_starts_at_the_transition_itself() {
        // The TZDB edge case the pinned intl402 fixture
        // PlainDate/prototype/toZonedDateTime/dst-skipped-cross-midnight.js
        // exists for: Toronto moved from 00:00 to 00:30 on 1919-03-31, so the
        // day starts 30 minutes before "compatible" midnight resolves to.
        let zone = TimeZone::Iana("America/Toronto");
        let start = zone.start_of_day((1919, 3, 31));
        let midnight = zone
            .epoch_nanoseconds_for(
                (1919, 3, 31),
                (0, 0, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .expect("compatible disambiguation always resolves");
        // The fixture's own expected figure: 30 minutes, not an hour.
        assert_eq!(&midnight - &start, BigInt::from(30 * 60_000_000_000_i64));
        // The start of day really is 00:30 local, and one nanosecond earlier
        // is still the previous day.
        let offset = zone.offset_nanoseconds_for(&start);
        assert_eq!(
            epoch::instant_fields(&(&start + BigInt::from(offset))),
            ((1919, 3, 31), (0, 30, 0, 0, 0, 0))
        );
        let earlier = &start - BigInt::from(1);
        let offset = zone.offset_nanoseconds_for(&earlier);
        assert_eq!(
            epoch::instant_fields(&(earlier + BigInt::from(offset))).0,
            (1919, 3, 30)
        );
        // An ordinary day still starts at local midnight.
        assert_eq!(
            TimeZone::Iana("UTC").start_of_day((2020, 1, 1)),
            BigInt::from(1_577_836_800_i64) * 1_000_000_000_u32
        );
        assert_eq!(
            TimeZone::Offset(-300).start_of_day((1970, 1, 2)),
            BigInt::from(86_400 + 5 * 3_600_i64) * 1_000_000_000_u32
        );
    }

    #[test]
    fn an_unambiguous_named_local_time_resolves_to_one_instant() {
        assert_eq!(
            TimeZone::Iana("UTC").epoch_nanoseconds_for(
                (2020, 1, 1),
                (0, 0, 0, 0, 0, 0),
                Disambiguation::Compatible
            ),
            Ok(BigInt::from(1_577_836_800_i64) * 1_000_000_000)
        );
        assert_eq!(
            TimeZone::Iana("Europe/Madrid").epoch_nanoseconds_for(
                (1970, 1, 1),
                (1, 0, 0, 0, 0, 0),
                Disambiguation::Compatible
            ),
            Ok(BigInt::from(0))
        );
    }

    /// Real values pinned directly from the pinned Test262 corpus's
    /// `intl402/Temporal/ZonedDateTime/prototype/getTimeZoneTransition/
    /// specific-tzdb-values.js`.
    #[test]
    fn adjacent_transition_finds_real_historical_offset_changes() {
        let new_york = TimeZone::Iana("America/New_York");
        assert_eq!(
            new_york
                .adjacent_transition(&(BigInt::from(1_555_448_460_i64) * 1_000_000_000_u32), true,),
            Some(BigInt::from(1_572_760_800_i64) * 1_000_000_000_u32)
        );
        assert_eq!(
            new_york.adjacent_transition(
                &(-BigInt::from(5_364_662_400_i64) * 1_000_000_000_u32),
                true,
            ),
            Some(-BigInt::from(2_717_650_800_i64) * 1_000_000_000_u32)
        );
        let london = TimeZone::Iana("Europe/London");
        assert_eq!(
            london.adjacent_transition(
                &(BigInt::from(1_591_909_260_i64) * 1_000_000_000_u32),
                false,
            ),
            Some(BigInt::from(1_585_443_600_i64) * 1_000_000_000_u32)
        );
        assert_eq!(
            london.adjacent_transition(
                &(-BigInt::from(3_849_984_000_i64) * 1_000_000_000_u32),
                false,
            ),
            Some(BigInt::from(-3_852_662_325_i64) * 1_000_000_000)
        );
    }

    /// A fixed-offset zone never transitions; a named zone with no real
    /// transitions in a direction (`UTC` itself, or querying "next" from
    /// after a zone's last historical DST year) reports `None` too, per
    /// `utc-no-transitions.js`/`offset-timezone-no-transitions.js`/
    /// `no-future-transitions.js`.
    #[test]
    fn adjacent_transition_is_none_for_a_fixed_offset_or_transition_less_zone() {
        let epoch = BigInt::from(0);
        assert_eq!(
            TimeZone::Offset(-600).adjacent_transition(&epoch, true),
            None
        );
        assert_eq!(
            TimeZone::Offset(-600).adjacent_transition(&epoch, false),
            None
        );
        assert_eq!(
            TimeZone::Iana("UTC").adjacent_transition(&epoch, true),
            None
        );
        assert_eq!(
            TimeZone::Iana("UTC").adjacent_transition(&epoch, false),
            None
        );
        // Asia/Kolkata has not observed DST since 1945; from 2024 there is no
        // future transition, only a historical one going backward.
        let kolkata = TimeZone::Iana("Asia/Kolkata");
        let from_2024 = kolkata
            .epoch_nanoseconds_for(
                (2024, 6, 15),
                (12, 0, 0, 0, 0, 0),
                Disambiguation::Compatible,
            )
            .unwrap();
        assert_eq!(kolkata.adjacent_transition(&from_2024, true), None);
        assert!(kolkata.adjacent_transition(&from_2024, false).is_some());
    }

    /// An instant far outside Jiff's own ISO ±9999-year representable range
    /// has no queryable real transition data — this lookup reports `None`
    /// rather than projecting onto a synthetic cycle the way
    /// [`TimeZone::offset_nanoseconds_for`] does for a plain offset query,
    /// per `transition-at-instant-boundaries.js`'s own outer-bound checks.
    #[test]
    fn adjacent_transition_is_none_far_outside_jiffs_representable_range() {
        let far_future = BigInt::from(8_640_000_000_000_000_i64) * 1_000_000;
        let far_past = -(BigInt::from(8_640_000_000_000_000_i64) * 1_000_000_u32);
        let ny = TimeZone::Iana("America/New_York");
        assert_eq!(ny.adjacent_transition(&far_future, true), None);
        assert_eq!(ny.adjacent_transition(&far_past, false), None);
    }
}
