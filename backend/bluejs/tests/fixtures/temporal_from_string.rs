// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct conversion contracts, including generic kinds normally dispatched
//! through specialized adapters by JavaScript callers.

use super::*;

#[test]
fn generic_time_conversion_retains_all_fields_and_ignores_calendar_annotations() {
    let mut vm = Vm::default();
    for source in ["12:34:56.123456789", "12:34:56.123456789[u-ca=unknown]"] {
        let value = vm
            .temporal_value_from_string(TemporalKind::PlainTime, source)
            .unwrap();
        assert_eq!(value.kind, TemporalKind::PlainTime);
        assert_eq!(
            (
                value.hour,
                value.minute,
                value.second,
                value.millisecond,
                value.microsecond,
                value.nanosecond
            ),
            (12, 34, 56, 123, 456, 789)
        );
    }
}

#[test]
fn generic_instant_conversion_requires_an_offset_and_checks_the_resolved_instant() {
    let mut vm = Vm::default();
    for source in [
        "1970-01-01",
        "1970-01-01T00:00",
        "+275760-09-13T00:00:00-00:01",
    ] {
        assert!(
            matches!(
                vm.temporal_value_from_string(TemporalKind::Instant, source),
                Err(RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
    for source in [
        "1970-01-01T00:00Z",
        "1970-01-01T01:00+01:00",
        "1970-01-01T00:00Z[u-ca=unknown]",
    ] {
        let value = vm
            .temporal_value_from_string(TemporalKind::Instant, source)
            .unwrap();
        assert_eq!(value.epoch_nanoseconds, 0.into(), "{source}");
    }
}
