// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Proxy receiver semantics, RegExp compile ordering and host API limits.

use blueice_bluejs::{compile, parse, HostValue, RuntimeError, Value, Vm, VmConfig};

fn assert_script(source: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn typed_array_ancestor_set_uses_the_distinct_receiver_and_converts_after_lookup() {
    assert_script(
        r#"
        var typed = new Uint8Array(1), target = Object.create(typed), conversions = 0;
        var value = {valueOf() {conversions++; return 257;}};
        var stored = Reflect.set(target, '0', value, typed);
        var ordinary = {};
        var copied = Reflect.set(target, '0', value, ordinary);
        stored && copied && typed[0] === 1 && ordinary[0] === value && conversions === 1
    "#,
    );
}

#[test]
fn a_resized_typed_array_ancestor_rechecks_the_index_after_value_conversion() {
    assert_script(
        r#"
        var buffer = new ArrayBuffer(1, {maxByteLength:4}), typed = new Uint8Array(buffer);
        var target = Object.create(typed), value = {valueOf() {buffer.resize(4); return 7;}};
        Reflect.set(target, '3', value, typed) && typed[3] === 7
    "#,
    );
}

#[test]
fn revocable_proxies_reject_primitive_targets_and_handlers() {
    assert_script(
        r#"
        var passed = 0;
        for (var value of [undefined, null, 1, 'x', true, Symbol(), 1n]) {
            try {Proxy.revocable(value, {});} catch (e) {if (e instanceof TypeError) passed++;}
            try {Proxy.revocable({}, value);} catch (e) {if (e instanceof TypeError) passed++;}
        }
        passed === 14
    "#,
    );
}

#[test]
fn foreign_descriptors_and_explicit_receivers_keep_their_real_property_owners() {
    assert_script(
        r#"
        var other = $262.createRealm().global;
        var foreign = other.eval('({get value() {return this.mark;}})');
        var receiver = {mark:42};
        var descriptor = Object.getOwnPropertyDescriptor(foreign, 'value');
        typeof descriptor.get === 'function' && descriptor.get.call(receiver) === 42 &&
            Reflect.get(foreign, 'value', receiver) === 42
    "#,
    );
}

#[test]
fn regexp_compile_uses_pattern_slots_without_coercing_a_regexp_argument() {
    assert_script(
        r#"
        var receiver = /old/g, pattern = /new/i, calls = 0;
        pattern.toString = function() {calls++; throw 'coerced';};
        receiver.lastIndex = 3;
        var same = receiver.compile(pattern) === receiver;
        same && receiver.source === 'new' && receiver.flags === 'i' &&
            receiver.lastIndex === 0 && calls === 0
    "#,
    );
}

#[test]
fn regexp_compile_failure_preserves_existing_matcher_until_a_later_success() {
    assert_script(
        r#"
        var receiver = /old/g, sentinel = {}, same = false;
        try {receiver.compile({toString() {throw sentinel;}});} catch (e) {same = e === sentinel;}
        var syntax = false;
        try {receiver.compile('[');} catch (e) {syntax = e instanceof SyntaxError;}
        var retained = receiver.source === 'old' && receiver.flags === 'g';
        receiver.compile('new', 'i');
        same && syntax && retained && receiver.test('NEW')
    "#,
    );
}

#[test]
fn regexp_methods_reject_invalid_receivers_before_coercing_arguments() {
    assert_script(
        r#"
        var calls = 0, argument = {toString() {calls++; return 'x';}}, rejected = 0;
        for (var name of ['compile', 'exec', 'test', 'toString']) {
            try {RegExp.prototype[name].call(null, argument);}
            catch (e) {if (e instanceof TypeError) rejected++;}
        }
        rejected === 4 && calls === 0
    "#,
    );
}

#[test]
fn host_registration_rejects_invalid_names_without_retaining_callbacks() {
    let mut vm = Vm::default();
    for name in ["", "9value", "a-b", "a.b", "a\0b", "aé", "é"] {
        assert!(matches!(
            vm.install_host_function(name, 0, |_args: &[HostValue]| Ok(HostValue::Undefined)),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            vm.install_host_object(name),
            Err(RuntimeError::TypeError(_))
        ));
    }
    let object = vm.install_host_object("$host_0").unwrap();
    vm.install_host_method(object, "_get9", 0, |_args: &[HostValue]| {
        Ok(HostValue::Number(42.0))
    })
    .unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("$host_0._get9()").unwrap()).unwrap())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn host_return_string_limits_propagate_and_leave_the_vm_reusable() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_host_function("hostValue", 0, |_args: &[HostValue]| {
        Ok(HostValue::String("x".repeat(65).into()))
    })
    .unwrap();
    assert!(vm
        .execute_script(&compile(&parse("hostValue()").unwrap()).unwrap())
        .is_err());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap())
            .unwrap(),
        Value::Number(42.0)
    );
}
