// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bracketed annotation suffixes (`[time-zone]`, `[u-ca=...]`) and the
//! `TimeZoneIdentifier` shape check.

use super::offset::{parse_minute_precision_offset, scan_offset};
use super::scan::Cursor;

/// The annotation suffix of an ISO date/time/offset string: an optional
/// leading time-zone annotation followed by zero or more `[key=value]`
/// annotations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Annotations {
    /// The time-zone annotation's body (without `!`), if one was present.
    pub(crate) time_zone: Option<String>,
    /// The first `u-ca=` annotation's value, if any.
    pub(crate) calendar: Option<String>,
}

/// Scans the zero-or-more bracket annotations that may follow an ISO
/// date/time/offset prefix.
///
/// Grammar notes (from Temporal's annotation syntax): an optional leading
/// time-zone annotation (no `=` in its body) is returned as
/// [`Annotations::time_zone`] after being checked against
/// `TimeZoneIdentifier` — an IANA-shaped name, or a UTC offset with at most
/// minute precision (Test262's `instant-string-sub-minute-offset.js` makes
/// the sub-minute rejection a syntax error, not a later resolution failure).
/// Whether a named identifier actually *exists* is a separate, later concern
/// (Stage 1 Track E's `time_zone.rs`). Every subsequent annotation is
/// `[!]key=value`; a key containing any non-lowercase character is always a
/// syntax error, regardless of the critical (`!`) flag. A second or later
/// `u-ca` annotation is ignored rather than resolved, but more than one
/// `u-ca` annotation with any of them critical is a syntax error. Any other
/// unrecognized key is ignored unless marked critical, in which case this
/// returns `Err`.
pub(crate) fn parse_annotation_suffix(mut cursor: &str) -> Result<Annotations, ()> {
    let mut result = Annotations::default();
    if let Some(rest) = cursor.strip_prefix('[') {
        let end = rest.find(']').ok_or(())?;
        let body = rest[..end].strip_prefix('!').unwrap_or(&rest[..end]);
        if !body.contains('=') {
            if !is_time_zone_identifier(body) {
                return Err(());
            }
            result.time_zone = Some(body.to_string());
            cursor = &rest[end + 1..];
        }
    }
    let mut calendars = 0_usize;
    let mut critical_calendar = false;
    while !cursor.is_empty() {
        let rest = cursor.strip_prefix('[').ok_or(())?;
        let end = rest.find(']').ok_or(())?;
        let body = &rest[..end];
        cursor = &rest[end + 1..];
        let (critical, body) = body
            .strip_prefix('!')
            .map_or((false, body), |rest| (true, rest));
        let (key, value) = body.split_once('=').ok_or(())?;
        if key.is_empty() || value.is_empty() {
            return Err(());
        }
        let key_valid = key.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase() || byte == b'_'
            } else {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            }
        });
        let value_valid = value.split('-').all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
        if !key_valid || !value_valid {
            return Err(());
        }
        if key == "u-ca" {
            calendars += 1;
            critical_calendar |= critical;
            if calendars > 1 && critical_calendar {
                return Err(());
            }
            result.calendar.get_or_insert_with(|| value.to_string());
        } else if critical {
            return Err(());
        }
    }
    Ok(result)
}

/// `TimeZoneIdentifier`: either a minute-precision UTC offset, or an IANA
/// name whose *shape* is checked here — whether the name denotes a real
/// zone is a time-zone-resolution concern, not a grammar one
/// (`Temporal/Instant/from/argument-string.js` accepts
/// `[NotATimeZone]`).
pub(super) fn is_valid_time_zone_identifier(body: &str) -> bool {
    if matches!(body.as_bytes().first(), Some(b'+' | b'-')) {
        let mut cursor = Cursor::new(body);
        return scan_offset(&mut cursor, false).is_some() && cursor.done();
    }
    !body.is_empty()
        && body.split('/').all(|component| {
            (1..=14).contains(&component.len())
                && component != "."
                && component != ".."
                && component
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'.' | b'_'))
                && component.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
                })
        })
}

pub(super) fn is_valid_annotation_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase() || byte == b'_'
            } else {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            }
        })
}

pub(super) fn is_valid_annotation_value(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

/// `TimeZoneAnnotation? Annotations?`: an optional leading time-zone
/// annotation followed by zero or more `[!]key=value` annotations.
///
/// Grammar notes, each taken from a pinned fixture rather than assumed:
/// the time-zone annotation must come first, so a second one anywhere later
/// is a syntax error (`Instant/from/argument-string-multiple-time-zone.js`);
/// an annotation key must be entirely lowercase regardless of the critical
/// (`!`) flag; an unrecognized key is ignored unless critical, in which case
/// this returns `Err`
/// (`PlainDateTime/from/argument-string-critical-unknown-annotation.js`); a
/// repeated `u-ca` annotation is ignored, but only when *no* `u-ca`
/// annotation is critical
/// (`PlainDateTime/from/argument-string-multiple-calendar.js`).
pub(super) fn scan_annotations(
    cursor: &mut Cursor,
) -> Result<(Option<String>, Option<String>), ()> {
    let mut time_zone = None;
    if cursor.peek() == Some(b'[') {
        let body = cursor.peek_bracket().ok_or(())?;
        let name = body.strip_prefix('!').unwrap_or(body);
        if !name.contains('=') {
            if !is_valid_time_zone_identifier(name) {
                return Err(());
            }
            time_zone = Some(name.to_string());
            // `peek_bracket` has just found this very bracket, so taking it
            // cannot fail.
            let _ = cursor.take_bracket();
        }
    }
    let mut calendar = None;
    let mut calendars = 0_usize;
    let mut critical_calendar = false;
    while cursor.peek() == Some(b'[') {
        let body = cursor.take_bracket().ok_or(())?;
        let (critical, body) = body
            .strip_prefix('!')
            .map_or((false, body), |rest| (true, rest));
        let (key, value) = body.split_once('=').ok_or(())?;
        if !is_valid_annotation_key(key) || !is_valid_annotation_value(value) {
            return Err(());
        }
        if key == "u-ca" {
            calendars += 1;
            critical_calendar |= critical;
            if calendar.is_none() {
                calendar = Some(value.to_string());
            }
        } else if critical {
            return Err(());
        }
    }
    if calendars > 1 && critical_calendar {
        return Err(());
    }
    Ok((time_zone, calendar))
}

/// Whether `value` matches Temporal's `TimeZoneIdentifier`: either a UTC
/// offset with at most minute precision, or an IANA-shaped name. Existence
/// of a named zone is deliberately not checked here.
pub(crate) fn is_time_zone_identifier(value: &str) -> bool {
    if value.starts_with(['+', '-']) {
        return parse_minute_precision_offset(value).is_some();
    }
    !value.is_empty()
        && value.split('/').all(|component| {
            (1..=14).contains(&component.len())
                && component
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'.'))
                && component.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_annotations_must_be_terminated_and_keyed() {
        assert_eq!(
            parse_annotation_suffix("[Etc/GMT+5][u-ca=iso8601]"),
            Ok(Annotations {
                time_zone: Some("Etc/GMT+5".into()),
                calendar: Some("iso8601".into()),
            })
        );
        assert_eq!(parse_annotation_suffix("[UTC"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][u-ca=iso8601"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][u-ca]"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC]tail"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][=x]"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][u-ca=]"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][Bad=x]"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][k=a_b]"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][!x=y]"), Err(()));
        assert_eq!(parse_annotation_suffix("[UTC][u-ca=a][!u-ca=b]"), Err(()));
        assert_eq!(parse_annotation_suffix("[bad zone]"), Err(()));
    }

    #[test]
    fn scanned_annotations_must_be_terminated_and_valid() {
        let scan = |source: &str| {
            let mut cursor = Cursor::new(source);
            scan_annotations(&mut cursor)
        };
        assert_eq!(scan("[UTC"), Err(()));
        assert_eq!(scan("[UTC][u-ca=iso8601"), Err(()));
        assert_eq!(scan("[UTC][u-ca]"), Err(()));
        assert_eq!(scan("[bad zone]"), Err(()));
        assert_eq!(scan("[UTC][k=a_b]"), Err(()));
        assert_eq!(scan("[UTC][!x=y]"), Err(()));
        assert_eq!(scan("[UTC][u-ca=a][!u-ca=b]"), Err(()));
        assert_eq!(
            scan("[Etc/GMT+5][u-ca=hebrew]"),
            Ok((Some("Etc/GMT+5".into()), Some("hebrew".into())))
        );
        assert_eq!(scan("[_x][x-y=1]"), Ok((Some("_x".into()), None)));
        assert_eq!(scan("[.a/b.c-d]"), Ok((Some(".a/b.c-d".into()), None)));
    }

    #[test]
    fn time_zone_identifier_shapes() {
        for accepted in [
            "UTC",
            "America/Port-au-Prince",
            "Etc/GMT+5",
            "_x",
            ".x",
            "a.b_c",
            "+01:00",
            "-0130",
        ] {
            assert!(is_time_zone_identifier(accepted), "{accepted}");
        }
        for rejected in ["", "1x", "a//b", "a/", "+01:00:00", "-", "a b", "x/a$"] {
            assert!(!is_time_zone_identifier(rejected), "{rejected}");
        }
        for accepted in ["UTC", "Etc/GMT+5", "_x", ".x", "a.b_c", "+01:00"] {
            assert!(is_valid_time_zone_identifier(accepted), "{accepted}");
        }
        for rejected in [
            "",
            ".",
            "..",
            "1x",
            "+01:00:00",
            "a/verylongcomponentname",
            "a b",
        ] {
            assert!(!is_valid_time_zone_identifier(rejected), "{rejected}");
        }
    }
}
