// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct conversion contracts, including generic kinds normally dispatched
//! through specialized adapters by JavaScript callers.

use super::*;

#[cfg_attr(test, test)]
fn duration_endpoints_keep_a_blank_boundary_anchor_and_reject_foreign_receivers() {
    let record = blueice_ecma402::DurationRecord::default();
    let midnight = (0, 0, 0, 0, 0, 0);
    for date in [(-271_821, 4, 19), (2000, 2, 29), (275_760, 9, 13)] {
        assert_eq!(
            Vm::temporal_duration_plain_endpoints(AnyCalendarKind::Iso, date, &record),
            Ok(((date, midnight), (date, midnight)))
        );
    }
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let mut vm = Vm::default();
    assert_eq!(
        vm.temporal_duration_receiver(&Value::Object(object)),
        Err(RuntimeError::Heap(HeapError::InvalidObject(object)))
    );
    assert!(
        matches!(vm.temporal_duration_relative_to(&Value::Object(object)),Err(RuntimeError::Heap(HeapError::InvalidObject(actual))) if actual == object)
    );
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn relative_duration_anchors_preserve_the_calendar_and_local_date() {
    use crate::vm::temporal::{time_zone, DurationAnchor};
    let mut vm = Vm::default();
    for source in [
        "2000-02-29[u-ca=gregory]",
        "2000-02-29T12:34+01:00[+01:00][u-ca=gregory]",
    ] {
        let anchor = vm
            .temporal_duration_relative_to(&Value::String(source.into()))
            .unwrap()
            .unwrap();
        assert_eq!(anchor.calendar(), AnyCalendarKind::Gregorian);
        assert_eq!(anchor.date(), (2000, 2, 29));
        if let DurationAnchor::Zoned {
            zone, local_time, ..
        } = anchor
        {
            assert_eq!(
                zone.identifier(),
                time_zone::parse_identifier("+01:00").unwrap().identifier()
            );
            assert_eq!(local_time, (12, 34, 0, 0, 0, 0));
        }
    }
}

#[cfg_attr(test, test)]
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

#[cfg_attr(test, test)]
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

impl crate::Vm {
    /// Runs arithmetic contracts only in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_temporal_string_boundary_contracts() {
        duration_endpoints_keep_a_blank_boundary_anchor_and_reject_foreign_receivers();
        relative_duration_anchors_preserve_the_calendar_and_local_date();
        generic_time_conversion_retains_all_fields_and_ignores_calendar_annotations();
        generic_instant_conversion_requires_an_offset_and_checks_the_resolved_instant();
    }
}
