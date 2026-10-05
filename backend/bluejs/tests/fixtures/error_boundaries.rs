// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Error and Intl contracts at valid heap, descriptor, and resource boundaries.

use super::*;
use crate::{compile, parse};

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn error_and_intl_receivers_reject_another_live_heaps_handle() {
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let value = Value::Object(object);
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    vm.intl_global().unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(object)));
    for result in [
        vm.has_property(object, &"name".into()).map(|_| ()),
        vm.typeof_value(&value).map(|_| ()),
        vm.error_is_error(&value).map(|_| ()),
        vm.error_stack_getter(&value).map(|_| ()),
        vm.error_header_field(object, "name").map(|_| ()),
        vm.date_time_format_data(&value).map(|_| ()),
        vm.unwrap_date_time_format(&value).map(|_| ()),
        vm.create_date_time_format(&value, &[], false).map(|_| ()),
        vm.date_time_format_format(&value, &Value::Undefined)
            .map(|_| ()),
        vm.date_time_format_value(&value, false).map(|_| ()),
        vm.number_format_data(&value).map(|_| ()),
        vm.unwrap_number_format(&value).map(|_| ()),
    ] {
        assert_eq!(result, expected);
    }
    assert!(vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn error_headers_ignore_getters_non_strings_and_exotic_prototypes() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        execute(
            &mut vm,
            r#"
        var getter = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').get;
        var checked = 0;
        for (var entry of [
            ['ordinary', 'message', 'ordinary: message'],
            ['ordinary', '', 'ordinary'],
            [7, 'message', 'Error: message'],
            ['ordinary', 7, 'ordinary'],
            [undefined, undefined, 'Error']
        ]) {
            var error = new Error();
            Object.defineProperty(error, 'name', {value:entry[0], configurable:true});
            Object.defineProperty(error, 'message', {value:entry[1], configurable:true});
            if (getter.call(error) !== entry[2]) throw 'wrong header';
            checked++;
        }
        var error = new Error();
        Object.defineProperty(error, 'name', {get() {throw 7;}, configurable:true});
        Object.defineProperty(error, 'message', {get() {throw 9;}, configurable:true});
        if (getter.call(error) !== 'Error') throw 'ran header getter';
        for (var prototype of [null, new Proxy({}, {get() {throw 7;}}), $262.createRealm().global.Object.prototype]) {
            var error = new Error(); Object.setPrototypeOf(error, prototype);
            if (getter.call(error) !== 'Error') throw 'read exotic header';
            checked++;
        }
        if (getter.call({}) !== undefined) throw 'unbranded stack';
        var rejected = false; try {getter.call(7);} catch (error) {rejected = error instanceof TypeError;}
        checked === 8 && rejected && Error.isError(new Error()) && !Error.isError({}) && !Error.isError(7)
    "#
        ),
        Value::Bool(true)
    );
    let value = execute(&mut vm, "new Error('message')");
    let root = vm.heap.root(value.object_id().unwrap()).unwrap();
    for limit in [10, 14] {
        vm.config.max_string_bytes = limit;
        assert_eq!(
            vm.error_stack_getter(&value),
            Err(RuntimeError::StringLimit { limit })
        );
    }
    vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
    assert_eq!(
        vm.error_stack_getter(&value),
        Ok(Value::String("Error: message".into()))
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn error_brand_checks_preserve_a_released_foreign_realm_error() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let error = execute(
        &mut vm,
        "$262.createRealm().global.eval('new Error(\"foreign\")')",
    );
    assert_eq!(vm.error_is_error(&error), Ok(Value::Bool(true)));
    vm.test262_realms.clear();
    let expected = RuntimeError::TypeError("foreign Test262 realm is no longer available".into());
    assert_eq!(vm.error_is_error(&error), Err(expected.clone()));
    assert_eq!(vm.error_stack_getter(&error), Err(expected));
}

#[cfg_attr(test, test)]
fn temporal_format_options_validate_every_kind_before_selecting_components() {
    let mut vm = Vm::default();
    let formatter = execute(&mut vm, "new Intl.DateTimeFormat('en', {timeZone:'UTC'})");
    let data = vm.date_time_format_data(&formatter).unwrap();
    for kind in [
        crate::heap::TemporalKind::PlainDate,
        crate::heap::TemporalKind::PlainDateTime,
        crate::heap::TemporalKind::PlainMonthDay,
        crate::heap::TemporalKind::PlainTime,
        crate::heap::TemporalKind::PlainYearMonth,
        crate::heap::TemporalKind::Instant,
    ] {
        assert!(vm.temporal_format_options(&data, kind).is_ok());
    }
    for (kind, name) in [
        (crate::heap::TemporalKind::Duration, "Duration"),
        (crate::heap::TemporalKind::ZonedDateTime, "ZonedDateTime"),
    ] {
        assert_eq!(
            vm.temporal_format_options(&data, kind),
            Err(RuntimeError::TypeError(format!(
                "Intl.DateTimeFormat does not support Temporal.{name}"
            ),))
        );
        let source = if kind == crate::heap::TemporalKind::Duration {
            "new Temporal.Duration(0, 0, 0, 1)"
        } else {
            "new Temporal.ZonedDateTime(0n, 'UTC')"
        };
        let value = execute(&mut vm, source);
        let temporal = vm
            .heap
            .temporal_value(value.object_id().unwrap())
            .unwrap()
            .unwrap();
        assert!(matches!(
            vm.temporal_date_time_format_input(temporal, data.options().clone()),
            Err(RuntimeError::TypeError(message)) if message == format!(
                "Intl.DateTimeFormat does not support Temporal.{name}"
            )
        ));
    }
}

#[cfg_attr(test, test)]
fn global_name_lookup_preserves_uninitialized_binding_errors() {
    let mut vm = Vm::default();
    let code = compile(&parse("let pending = 42; pending").unwrap()).unwrap();
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    assert_eq!(
        vm.lookup_global_name("pending"),
        Err(RuntimeError::ReferenceError("pending".into()))
    );
    let result = vm.run(&code);
    assert_eq!(vm.finish_root_execution(result), Ok(Value::Number(42.0)));
    assert_eq!(
        vm.lookup_global_name("pending"),
        Ok(Some(Value::Number(42.0)))
    );

    // The internal lookup contract must also reject a registered cell whose
    // value has not been installed yet, in either eval environment record.
    for outer in [false, true] {
        let cell = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(cell).unwrap();
        let binding = DynamicEvalBinding {
            cell,
            shadowed_cells: Vec::new(),
        };
        if outer {
            vm.dynamic_eval_outer_bindings
                .push(HashMap::from([("pendingEval".into(), binding)]));
        } else {
            vm.dynamic_eval_bindings
                .insert("pendingEval".into(), binding);
        }
        assert_eq!(
            vm.lookup_global_name("pendingEval"),
            Err(RuntimeError::ReferenceError("pendingEval".into()))
        );
        vm.heap.set(cell, "value", Value::Number(42.0)).unwrap();
        assert_eq!(
            vm.lookup_global_name("pendingEval"),
            Ok(Some(Value::Number(42.0)))
        );
        vm.dynamic_eval_bindings.clear();
        vm.dynamic_eval_outer_bindings.clear();
        vm.heap.unroot(root).unwrap();
    }
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn temporal_styles_reject_disjoint_components_before_range_formatting() {
    let mut vm = Vm::default();
    assert_eq!(
        execute(
            &mut vm,
            r#"
        var checked = 0;
        for (var pair of [
            [new Intl.DateTimeFormat('en', {dateStyle:'short', timeZone:'UTC'}), new Temporal.PlainTime(12)],
            [new Intl.DateTimeFormat('en', {timeStyle:'short', timeZone:'UTC'}), new Temporal.PlainDate(2026, 10, 2)]
        ]) {
            for (var method of ['format', 'formatToParts', 'formatRange', 'formatRangeToParts']) {
                try {pair[0][method](pair[1], pair[1]); throw 'accepted disjoint style';}
                catch (error) {if (!(error instanceof TypeError)) throw error; checked++;}
            }
        }
        checked === 8
    "#
        ),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_error_boundary_contracts() {
        cold_stack_setter_and_global_lookup_preserve_real_failures();
        error_and_intl_receivers_reject_another_live_heaps_handle();
        error_headers_ignore_getters_non_strings_and_exotic_prototypes();
        error_brand_checks_preserve_a_released_foreign_realm_error();
        temporal_format_options_validate_every_kind_before_selecting_components();
        global_name_lookup_preserves_uninitialized_binding_errors();
        temporal_styles_reject_disjoint_components_before_range_formatting();
    }
}

#[cfg_attr(test, test)]
fn cold_stack_setter_and_global_lookup_preserve_real_failures() {
    let mut vm = Vm::default();
    vm.string_intrinsics().unwrap();
    let object = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(object).unwrap();
    let receiver = Value::Object(object);
    let value = Value::String("answer".into());
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.error_stack_setter(&receiver, &value),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        vm.error_stack_setter(&receiver, &value),
        Ok(Value::Undefined)
    );
    assert_eq!(vm.get_property(&receiver, &"stack".into()), Ok(value));
    vm.heap.unroot(root).unwrap();

    execute(
        &mut vm,
        "Object.setPrototypeOf(globalThis, new Proxy({}, {has(){throw 7;}}));",
    );
    assert_eq!(
        vm.lookup_global_name("absent"),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}
