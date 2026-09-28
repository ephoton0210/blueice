// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn invalid_object_handles_propagate_from_both_date_converters() {
    let mut vm = Vm::default();
    let mut other_vm = Vm::default();
    let foreign = Value::Object(other_vm.heap.alloc_object(None).unwrap());
    assert!(matches!(
        vm.temporal_to_plain_date(&foreign, &Value::Undefined),
        Err(RuntimeError::Heap(_))
    ));
    assert!(matches!(
        vm.temporal_to_plain_date_time(&foreign, &Value::Undefined),
        Err(RuntimeError::Heap(_))
    ));
}

#[test]
fn all_calendar_date_unit_mappings_are_explicit() {
    use plain_date::DateUnit;
    use rounding::TemporalUnit;

    assert_eq!(
        Vm::temporal_unit_to_date_unit(TemporalUnit::Year),
        DateUnit::Year
    );
    assert_eq!(
        Vm::temporal_unit_to_date_unit(TemporalUnit::Month),
        DateUnit::Month
    );
    assert_eq!(
        Vm::temporal_unit_to_date_unit(TemporalUnit::Week),
        DateUnit::Week
    );
    assert_eq!(
        Vm::temporal_unit_to_date_unit(TemporalUnit::Day),
        DateUnit::Day
    );
    assert_eq!(
        Vm::temporal_unit_to_date_unit(TemporalUnit::Hour),
        DateUnit::Day
    );
}
