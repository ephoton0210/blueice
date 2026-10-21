// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny native Error inputs retain actual text capacity and cancellation identity.

use super::*;

type LanguageError = (&'static str, fn(String) -> RuntimeError);

fn neutral() -> Vm {
    let mut vm = Vm::default();
    // These private constructor calls use the ordinary native-entry fuel.
    vm.remaining_instructions = vm.config.instruction_budget;
    vm
}

fn page() -> Vm {
    let mut vm = neutral();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn loan<T>(
    vm: &mut Vm,
    source: &Vm,
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}

fn spare_capacity_sample(capacity: usize, neutral: bool) -> (usize, usize) {
    let source = page();
    let mut vm = if neutral { self::neutral() } else { page() };
    let measure = |vm: &mut Vm| {
        vm.error_object("TypeError", String::new())?;
        vm.collect_realm_garbage()?;
        let scope = if neutral {
            source.host_scheduling.as_ref().unwrap().child()
        } else {
            vm.host_scheduling.as_ref().unwrap().child()
        };
        let mut message = String::with_capacity(capacity);
        message.push_str("tiny");
        let actual = message.capacity();
        let (result, peak) = scope.measure_payload_peak(|| vm.error_object("TypeError", message));
        let object = result?.object_id().unwrap();
        assert_eq!(
            vm.heap.get_own(object, "message")?,
            Some(Value::String("tiny".into()))
        );
        Ok((actual, peak))
    };
    if neutral {
        loan(&mut vm, &source, measure).unwrap()
    } else {
        measure(&mut vm).unwrap()
    }
}

#[cfg_attr(test, test)]
fn error_input_storage_scoped_conversion_keeps_actual_utf8_capacity() {
    let small = spare_capacity_sample(1024, false);
    let large = spare_capacity_sample(2048, false);
    assert_eq!(large.1 - small.1, large.0 - small.0);
}

#[cfg_attr(test, test)]
fn error_input_storage_neutral_conversion_keeps_the_borrowed_account() {
    let small = spare_capacity_sample(1024, true);
    let large = spare_capacity_sample(2048, true);
    assert_eq!(large.1 - small.1, large.0 - small.0);
}

#[cfg_attr(test, test)]
fn error_input_storage_failed_scope_refuses_a_thrown_primitive() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let _ = scope.fail();
    assert_eq!(
        vm.error_value(RuntimeError::Thrown(Value::String("tiny".into()))),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn error_input_storage_failed_borrowed_scope_refuses_a_thrown_object() {
    let source = page();
    let mut vm = Vm::default();
    let object = vm.heap.alloc_object(None).unwrap();
    let result = loan(&mut vm, &source, |vm| {
        let _ = source.host_scheduling.as_ref().unwrap().fail();
        vm.error_value(RuntimeError::Thrown(Value::Object(object)))
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.current_realm_root_owner().unwrap().is_none());
    assert!(vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn error_input_storage_unicode_messages_preserve_each_language_error() {
    let mut vm = page();
    let cases: [LanguageError; 6] = [
        ("ReferenceError", RuntimeError::ReferenceError),
        ("TypeError", RuntimeError::TypeError),
        ("RangeError", RuntimeError::RangeError),
        ("SyntaxError", RuntimeError::SyntaxError),
        ("SyntaxError", RuntimeError::ModuleResolution),
        ("Test262Error", RuntimeError::Test262),
    ];
    for (name, constructor) in cases {
        let error = vm.error_value(constructor("é🙂".into())).unwrap();
        let object = error.object_id().unwrap();
        let root = vm.heap.root(object).unwrap();
        vm.collect_realm_garbage().unwrap();
        assert_eq!(
            vm.heap.get_own(object, "message").unwrap(),
            Some(Value::String("é🙂".into()))
        );
        assert_eq!(
            vm.get_property(&error, &"name".into()).unwrap(),
            Value::String(name.into())
        );
        vm.heap.unroot(root).unwrap();
    }
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn error_input_storage_healthy_thrown_primitive_is_a_pure_move() {
    let mut vm = page();
    let value = Value::String("é🙂".into());
    assert_eq!(
        vm.error_value(RuntimeError::Thrown(value)),
        Ok(Value::String("é🙂".into()))
    );
}

#[cfg_attr(test, test)]
fn error_input_storage_host_abort_remains_an_error() {
    let mut vm = page();
    assert_eq!(
        vm.error_value(RuntimeError::RegexWorker("tiny".into())),
        Err(RuntimeError::RegexWorker("tiny".into()))
    );
    assert_eq!(
        vm.error_value(RuntimeError::InstructionLimit),
        Err(RuntimeError::InstructionLimit)
    );
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.error_value(RuntimeError::InstructionLimit),
        Err(RuntimeError::InstructionLimit)
    );
}

#[cfg_attr(test, test)]
fn error_input_storage_error_copies_do_not_duplicate_source_spare_capacity() {
    let mut vm = page();
    vm.error_object("TypeError", String::new()).unwrap();
    vm.collect_realm_garbage().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut message = String::with_capacity(2048);
    message.push_str("tiny");
    let bytes = message.capacity();
    let original = Completion::Throw(RuntimeError::TypeError(message));
    let storage = Some(scope.shared_charge(bytes).unwrap());
    let (copy, completion_peak) =
        scope.measure_payload_peak(|| generator_input::copy_scalar_completion(&original, &storage));
    drop(copy.unwrap());
    let Completion::Throw(error) = &original else {
        unreachable!()
    };
    let (value, conversion_peak) =
        scope.measure_payload_peak(|| vm.copy_error_value(error, storage.as_ref()));
    let value = value.unwrap();
    assert_eq!(
        vm.heap
            .get_own(value.value().object_id().unwrap(), "message")
            .unwrap(),
        Some(Value::String("tiny".into()))
    );
    assert!(completion_peak == "tiny".len() && conversion_peak < bytes, "completion peak={completion_peak}, conversion peak={conversion_peak}, source capacity={bytes}");
}

#[cfg_attr(test, test)]
fn error_input_storage_string_limit_keeps_its_existing_range_error_message() {
    let mut vm = page();
    for (limit, text) in [
        (0, "BlueJS string exceeds 0 bytes"),
        (7, "BlueJS string exceeds 7 bytes"),
    ] {
        let error = vm
            .error_value(RuntimeError::StringLimit { limit })
            .unwrap()
            .object_id()
            .unwrap();
        assert_eq!(
            vm.heap.get_own(error, "message").unwrap(),
            Some(Value::String(text.into()))
        );
    }
}

#[cfg_attr(test, test)]
fn error_input_storage_utf16_copy_admits_before_conversion_and_outlives_the_vm() {
    let vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let owner = vm.argument_list_owner().unwrap();
    let message = error_input_storage::ErrorMessage::adopt("éA🙂".into(), owner.as_ref()).unwrap();
    let (value, peak) = scope.measure_payload_peak(|| message.into_utf16());
    let value = value.unwrap();
    assert_eq!(peak, 8);
    assert_eq!(value.value(), &Value::String("éA🙂".into()));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 8);
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn error_input_storage_retained_conversion_moves_the_existing_utf8_credit() {
    let mut vm = page();
    vm.error_object("TypeError", String::new()).unwrap();
    vm.collect_realm_garbage().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut message = String::with_capacity(2048);
    message.push_str("tiny");
    let bytes = message.capacity();
    let storage = Some(scope.shared_charge(bytes).unwrap());
    let (result, peak) = scope.measure_payload_peak(|| {
        vm.error_value_with_input(RuntimeError::TypeError(message), storage)
    });
    let value = result.unwrap();
    assert!(
        peak < bytes,
        "moving retained UTF-8 input must not reserve another source buffer: {peak}"
    );
    let object = value.value().object_id().unwrap();
    assert_eq!(
        vm.heap.get_own(object, "message").unwrap(),
        Some(Value::String("tiny".into()))
    );
    assert!(value.owner_view().unwrap().unwrap().belongs_to(&scope));
}

#[cfg_attr(test, test)]
fn error_input_storage_failed_original_input_refuses_before_lazy_intrinsic_publication() {
    let source = page();
    let before = source.host_scheduling_snapshot().payload_bytes;
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let storage = Some(scope.shared_charge("tiny".len()).unwrap());
    let mut vm = neutral();
    let globals = vm.globals.len();
    let _ = scope.fail();
    assert!(matches!(
        vm.error_value_with_input(RuntimeError::TypeError("tiny".into()), storage),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.globals.len(), globals);
    assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn error_input_storage_foreign_scoped_input_refuses_before_lazy_intrinsic_publication() {
    let source = page();
    let before = source.host_scheduling_snapshot().payload_bytes;
    let storage = Some(
        source
            .host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge("tiny".len())
            .unwrap(),
    );
    let mut vm = page();
    let globals = vm.globals.len();
    assert!(matches!(
        vm.error_value_with_input(RuntimeError::TypeError("tiny".into()), storage),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.globals.len(), globals);
    assert!(source.check_scheduling_failure().is_ok());
    assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn install_failure(vm: &mut Vm) {
    vm.install_host_function("fail", 0, |_: &[HostValue]| {
        Err(HostFunctionError::new("tiny"))
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn error_input_storage_native_promise_reaction_preserves_the_message_with_gc() {
    for borrowed in [false, true] {
        let mut vm = page();
        install_failure(&mut vm);
        run(
            &mut vm,
            "var caught;Promise.resolve(7).then(fail).catch(error=>caught=error.message);0;",
        );
        let owner = vm.argument_list_owner().unwrap();
        let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
        if borrowed {
            drop(vm.host_scheduling.take());
        }
        vm.with_realm_call_roots(roots, |vm| {
            vm.run_host_microtask_checkpoint()?;
            vm.collect_realm_garbage()?;
            assert_eq!(
                vm.lookup_global_name("caught")?,
                Some(Value::String("tiny".into()))
            );
            Ok(())
        })
        .unwrap();
    }
}

#[cfg_attr(test, test)]
fn error_input_storage_custom_promise_capability_receives_the_original_error() {
    let mut vm = page();
    install_failure(&mut vm);
    run(&mut vm, "var caught;function Species(executor){executor(function(){},function(error){caught=error.message})}var first=Promise.resolve(7);first.constructor={ [Symbol.species]:Species };first.then(fail);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        vm.lookup_global_name("caught").unwrap(),
        Some(Value::String("tiny".into()))
    );
}

#[cfg_attr(test, test)]
fn error_input_storage_async_generator_rejection_preserves_the_message_with_gc() {
    let mut vm = page();
    install_failure(&mut vm);
    run(
        &mut vm,
        "var iterator=(async function*(){yield 7;fail()})();var caught;iterator.next();0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    run(
        &mut vm,
        "iterator.next().catch(error=>caught=error.message);0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        vm.lookup_global_name("caught").unwrap(),
        Some(Value::String("tiny".into()))
    );
}

#[cfg_attr(test, test)]
fn error_input_storage_execution_payload_transfer_has_no_duplicate_peak() {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let error = RuntimeError::TypeError("tiny".into());
    let bytes = host_scheduling::runtime_error_payload_bytes(&error);
    let charge = vm.charge_execution_payload(bytes);
    vm.synchronize_scheduling_usage().unwrap();
    let (storage, peak) = scope.measure_payload_peak(|| charge.into_native(&mut vm, bytes));
    let storage = storage.unwrap().unwrap();
    assert_eq!(peak, 0);
    assert_eq!(storage.bytes(), bytes);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(error);
    drop(storage);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_error_input_storage_contracts() {
        error_input_storage_scoped_conversion_keeps_actual_utf8_capacity();
        error_input_storage_neutral_conversion_keeps_the_borrowed_account();
        error_input_storage_failed_scope_refuses_a_thrown_primitive();
        error_input_storage_failed_borrowed_scope_refuses_a_thrown_object();
        error_input_storage_unicode_messages_preserve_each_language_error();
        error_input_storage_healthy_thrown_primitive_is_a_pure_move();
        error_input_storage_host_abort_remains_an_error();
        error_input_storage_error_copies_do_not_duplicate_source_spare_capacity();
        error_input_storage_string_limit_keeps_its_existing_range_error_message();
        error_input_storage_utf16_copy_admits_before_conversion_and_outlives_the_vm();
        error_input_storage_retained_conversion_moves_the_existing_utf8_credit();
        error_input_storage_failed_original_input_refuses_before_lazy_intrinsic_publication();
        error_input_storage_foreign_scoped_input_refuses_before_lazy_intrinsic_publication();
        error_input_storage_native_promise_reaction_preserves_the_message_with_gc();
        error_input_storage_custom_promise_capability_receives_the_original_error();
        error_input_storage_async_generator_rejection_preserves_the_message_with_gc();
        error_input_storage_execution_payload_transfer_has_no_duplicate_peak();
    }
}
