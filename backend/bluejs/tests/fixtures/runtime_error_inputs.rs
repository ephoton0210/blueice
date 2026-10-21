// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed native error messages stay paid while iterator cleanup calls guests.

use super::*;
use std::cell::Cell;
use std::rc::Rc;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn install(vm: &mut Vm, message: &str) -> Rc<Cell<Option<usize>>> {
    let message = message.to_owned();
    vm.install_host_function("fail", 0, move |_: &[HostValue]| {
        Err(HostFunctionError::new(message.clone()))
    })
    .unwrap();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    let view = observer(vm);
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        assert!(output.get().is_none());
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    sample
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_completion_copy_retains_each_message() {
    let constructors: [fn(String) -> RuntimeError; 7] = [
        RuntimeError::ReferenceError,
        RuntimeError::TypeError,
        RuntimeError::RangeError,
        RuntimeError::SyntaxError,
        RuntimeError::Test262,
        RuntimeError::RegexWorker,
        RuntimeError::ModuleResolution,
    ];
    for constructor in constructors {
        let vm = page();
        let view = observer(&vm);
        let message = String::from("tiny");
        let bytes = message.capacity();
        let original = Completion::Throw(constructor(message));
        let storage = Some(
            vm.host_scheduling
                .as_ref()
                .unwrap()
                .shared_charge(bytes)
                .unwrap(),
        );
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        let (copied, peak) =
            scope.measure_payload_peak(|| copy_scalar_completion(&original, &storage));
        let (copy, copied_storage) = copied.unwrap();
        assert_eq!(peak, bytes);
        drop(original);
        drop(storage);
        drop(vm);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
        drop(copy);
        drop(copied_storage);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}

fn generator_sample(code: &Bytecode, message: &str) -> usize {
    let mut vm = page();
    let sample = install(&mut vm, message);
    vm.execute_script(code).unwrap();
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    let result = vm.generator_resume(&iterator, Some(Value::Undefined), None, None);
    assert_eq!(result, Err(RuntimeError::TypeError(message.into())));
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_generator_cleanup_keeps_native_error_copy() {
    let source = "var seen=false;var values={ [Symbol.iterator](){return {next(){if(seen)return {done:true};seen=true;return {value:7,done:false}},get return(){observe();return function(){return {done:true}}}}}};var iterator=(function*(){for(var value of values){yield value;fail()}})();iterator.next();0;";
    let code = crate::compile(&crate::parse(source).unwrap()).unwrap();
    let small = generator_sample(&code, "tiny");
    let large = generator_sample(&code, "extended");
    // Native completion and its independent temporary VM completion root.
    assert_eq!(large - small, 2 * ("extended".len() - "tiny".len()));
}

fn close_sample(code: &Bytecode, message: &str, neutral: bool) -> usize {
    let mut vm = page();
    let sample = install(&mut vm, message);
    vm.execute_script(code).unwrap();
    let inner = vm.lookup_global_name("inner").unwrap().unwrap();
    let outer = vm.lookup_global_name("outer").unwrap().unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    if neutral {
        drop(vm.host_scheduling.take());
    }
    vm.with_realm_call_roots(roots, |vm| {
        let outer = vm.direct_iterator_record(&outer)?;
        let inner = vm.direct_iterator_record(&inner)?;
        let mut records = vm.iterator_records(vec![outer, inner]);
        assert_eq!(
            vm.close_iterators_for_return(&mut records, 0),
            Err(RuntimeError::TypeError(message.into()))
        );
        assert!(records.is_empty());
        Ok(())
    })
    .unwrap();
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_iterator_close_keeps_first_message_across_outer_callback() {
    let source = "var inner={next(){return {done:true}},return:fail};var outer={next(){return {done:true}},return(){observe();return {done:true}}};0;";
    let code = crate::compile(&crate::parse(source).unwrap()).unwrap();
    let small = close_sample(&code, "tiny", false);
    let large = close_sample(&code, "extended", false);
    assert_eq!(large - small, "extended".len() - "tiny".len());
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_completed_generator_result_keeps_message_after_vm_drop() {
    let mut vm = page();
    run(&mut vm, "var iterator=(function*(){})();iterator.next();0;");
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    let view = observer(&vm);
    let message = String::from("tiny");
    let bytes = message.capacity();
    let mut storage = Some(
        vm.host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge(bytes)
            .unwrap(),
    );
    let result = vm.generator_resume_with_input(
        &iterator,
        None,
        None,
        Some(Completion::Throw(RuntimeError::TypeError(message))),
        &mut storage,
    );
    assert_eq!(result, Err(RuntimeError::TypeError("tiny".into())));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(result);
    drop(storage);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_public_catch_finally_and_normal_gc_preserve_message() {
    let mut vm = page();
    install(&mut vm, "tiny");
    assert_eq!(run(&mut vm, "var saved;var finished=0;try{fail()}catch(error){saved=error}finally{finished=1}saved.name+':'+saved.message+':'+finished;"), Value::String("TypeError:tiny:1".into()));
    vm.collect_realm_garbage().unwrap();
    assert_eq!(run(&mut vm, "saved.message;"), Value::String("tiny".into()));
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_original_throw_wins_over_close_error() {
    let mut vm = page();
    install(&mut vm, "tiny");
    assert_eq!(run(&mut vm, "var seen=false;var values={ [Symbol.iterator](){return {next(){if(seen)return {done:true};seen=true;return {value:7,done:false}},return:fail}}};var result;try{for(var value of values)throw 'original'}catch(error){result=error}result;"), Value::String("original".into()));
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_neutral_cleanup_uses_the_original_borrowed_account() {
    let source = "var inner={next(){return {done:true}},return:fail};var outer={next(){return {done:true}},return(){observe();return {done:true}}};0;";
    let code = crate::compile(&crate::parse(source).unwrap()).unwrap();
    let small = close_sample(&code, "tiny", true);
    let large = close_sample(&code, "extended", true);
    assert_eq!(large - small, "extended".len() - "tiny".len());
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_suspended_finally_returns_the_original_message_credit() {
    let mut vm = page();
    install(&mut vm, "tiny");
    run(&mut vm, "var iterator=(function*(){try{fail()}finally{yield 7}})();var first=iterator.next();first.value;");
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    let view = observer(&vm);
    let mut storage = None;
    let result =
        vm.generator_resume_with_input(&iterator, Some(Value::Undefined), None, None, &mut storage);
    assert_eq!(result, Err(RuntimeError::TypeError("tiny".into())));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, "tiny".len());
    drop(result);
    drop(storage);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_failed_owner_preserves_the_unattempted_generator() {
    let mut vm = page();
    run(&mut vm, "var iterator=(function*(){yield 7})();0;");
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    let generator = iterator.object_id().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut storage = Some(scope.shared_charge("tiny".len()).unwrap());
    let _ = scope.fail();
    assert_eq!(
        vm.generator_resume_with_input(
            &iterator,
            None,
            None,
            Some(Completion::Throw(RuntimeError::TypeError("tiny".into()))),
            &mut storage
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        vm.heap
            .generator_execution_code(generator)
            .unwrap()
            .unwrap()
            .1
    );
    assert_eq!(storage.as_ref().unwrap().bytes(), 0);
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_foreign_owner_refuses_before_generator_state_changes() {
    let source = page();
    let mut vm = page();
    run(&mut vm, "var iterator=(function*(){yield 7})();0;");
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    let generator = iterator.object_id().unwrap();
    let mut storage = Some(
        source
            .host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge("tiny".len())
            .unwrap(),
    );
    assert_eq!(
        vm.generator_resume_with_input(
            &iterator,
            None,
            None,
            Some(Completion::Throw(RuntimeError::TypeError("tiny".into()))),
            &mut storage
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        vm.heap
            .generator_execution_code(generator)
            .unwrap()
            .unwrap()
            .1
    );
    assert!(source.check_scheduling_failure().is_ok());
    assert_eq!(storage.as_ref().unwrap().bytes(), 0);
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_terminal_close_keeps_outer_cleanup_unattempted() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("fail", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Err(HostFunctionError::new("tiny"))
    })
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(output.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var inner={next(){return {done:true}},return:fail};var outer={next(){return {done:true}},return(){observe();return {done:true}}};0;");
    let inner = vm.lookup_global_name("inner").unwrap().unwrap();
    let outer = vm.lookup_global_name("outer").unwrap().unwrap();
    let outer = vm.direct_iterator_record(&outer).unwrap();
    let inner = vm.direct_iterator_record(&inner).unwrap();
    let mut records = vm.iterator_records(vec![outer, inner]);
    assert_eq!(
        vm.close_iterators_for_return(&mut records, 0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(records.len(), 1);
}

#[cfg_attr(test, test)]
fn runtime_error_inputs_thrown_primitive_cleanup_has_one_owned_buffer() {
    fn sample(code: &Bytecode, text: &str) -> usize {
        let mut vm = page();
        let sample = install(&mut vm, "unused");
        let text = JsString::from(text);
        vm.install_host_function("reason", 0, move |_: &[HostValue]| {
            Ok(HostValue::String(text.clone()))
        })
        .unwrap();
        vm.execute_script(code).unwrap();
        let inner = vm.lookup_global_name("inner").unwrap().unwrap();
        let outer = vm.lookup_global_name("outer").unwrap().unwrap();
        let outer = vm.direct_iterator_record(&outer).unwrap();
        let inner = vm.direct_iterator_record(&inner).unwrap();
        let mut records = vm.iterator_records(vec![outer, inner]);
        assert!(matches!(
            vm.close_iterators_for_return(&mut records, 0),
            Err(RuntimeError::Thrown(Value::String(_)))
        ));
        sample.get().unwrap()
    }
    let source = "var inner={next(){return {done:true}},return(){throw reason()}};var outer={next(){return {done:true}},return(){observe();return {done:true}}};0;";
    let code = crate::compile(&crate::parse(source).unwrap()).unwrap();
    assert_eq!(
        sample(&code, "extended") - sample(&code, "tiny"),
        2 * ("extended".len() - "tiny".len())
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_runtime_error_input_contracts() {
        runtime_error_inputs_completion_copy_retains_each_message();
        runtime_error_inputs_generator_cleanup_keeps_native_error_copy();
        runtime_error_inputs_iterator_close_keeps_first_message_across_outer_callback();
        runtime_error_inputs_completed_generator_result_keeps_message_after_vm_drop();
        runtime_error_inputs_public_catch_finally_and_normal_gc_preserve_message();
        runtime_error_inputs_original_throw_wins_over_close_error();
        runtime_error_inputs_neutral_cleanup_uses_the_original_borrowed_account();
        runtime_error_inputs_suspended_finally_returns_the_original_message_credit();
        runtime_error_inputs_failed_owner_preserves_the_unattempted_generator();
        runtime_error_inputs_foreign_owner_refuses_before_generator_state_changes();
        runtime_error_inputs_terminal_close_keeps_outer_cleanup_unattempted();
        runtime_error_inputs_thrown_primitive_cleanup_has_one_owned_buffer();
    }
}
