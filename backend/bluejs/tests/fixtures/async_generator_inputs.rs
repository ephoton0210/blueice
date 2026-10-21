// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed scheduler inputs observed through delegate callbacks and normal GC.

use super::*;
use crate::Bytecode;
use std::cell::Cell;
use std::rc::Rc;

const DELEGATE: &str = "var delegated=false;var delegate={ [Symbol.asyncIterator](){return this},next(){if(delegated)return {done:true};delegated=true;return {value:7,done:false}},get return(){observe();return function(value){return {value,done:true}}}};var iterator=(async function*(){return yield* delegate})();var first=iterator.next();0;";
const NEXT_BODY: &str = "var iterator=(async function*(){var sent=yield 7;observe();return sent})();var first=iterator.next();0;";
const FINALLY_BODY: &str = "var iterator=(async function*(){try{yield 7}finally{observe()}})();var first=iterator.next();0;";
const ITERATOR_CLOSE: &str = "var seen=false;var values={ [Symbol.iterator](){return {next(){if(seen)return {done:true};seen=true;return {value:7,done:false}},get return(){observe();return function(){return {done:true}}}}}};var iterator=(async function*(){for(var value of values)yield value})();var first=iterator.next();0;";

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn input_sample(code: &Bytecode, value: &str, neutral: bool, kind: NativeFunction) -> usize {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let sample = Rc::new(Cell::new(None));
    let output = sample.clone();
    let view = observer(&vm);
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        assert!(output.get().is_none());
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    vm.execute_script(code).unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    vm.collect_realm_garbage().unwrap();
    if neutral {
        drop(vm.host_scheduling.take());
    }
    let target = vm
        .async_generator_request(&iterator, Value::String(value.into()), kind)
        .unwrap()
        .object_id()
        .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    if kind == NativeFunction::AsyncGeneratorThrow {
        assert!(
            matches!(&vm.promises[&target].status, PromiseStatus::Rejected(Value::String(reason)) if reason == &JsString::from(value))
        );
    } else {
        let PromiseStatus::Fulfilled(Value::Object(result)) = vm.promises[&target].status else {
            panic!("the completed request must fulfill its original target");
        };
        assert_eq!(
            vm.heap.get_own(result, "value").unwrap(),
            Some(Value::String(value.into()))
        );
        assert_eq!(
            vm.heap.get_own(result, "done").unwrap(),
            Some(Value::Bool(true))
        );
    }
    sample.get().unwrap()
}

#[cfg_attr(test, test)]
fn async_generator_inputs_delegate_getter_retains_one_independent_head_copy() {
    let code = crate::compile(&crate::parse(DELEGATE).unwrap()).unwrap();
    let small = input_sample(&code, "tiny", false, NativeFunction::AsyncGeneratorReturn);
    let large = input_sample(
        &code,
        "extended",
        false,
        NativeFunction::AsyncGeneratorReturn,
    );
    // At GetMethod: queued input, fulfilled await input and active head copy.
    let difference = 2 * ("extended".len() - "tiny".len());
    assert_eq!(large - small, 3 * difference);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_neutral_delegate_getter_keeps_the_original_head_account() {
    let code = crate::compile(&crate::parse(DELEGATE).unwrap()).unwrap();
    let small = input_sample(&code, "tiny", true, NativeFunction::AsyncGeneratorReturn);
    let large = input_sample(
        &code,
        "extended",
        true,
        NativeFunction::AsyncGeneratorReturn,
    );
    let difference = 2 * ("extended".len() - "tiny".len());
    assert_eq!(large - small, 3 * difference);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_next_handoff_charges_the_stack_buffer_once() {
    let code = crate::compile(&crate::parse(NEXT_BODY).unwrap()).unwrap();
    let small = input_sample(&code, "tiny", false, NativeFunction::AsyncGeneratorNext);
    let large = input_sample(&code, "extended", false, NativeFunction::AsyncGeneratorNext);
    // Queued input and the resumed frame binding; the moved head is no longer native input.
    let difference = 2 * ("extended".len() - "tiny".len());
    assert_eq!(large - small, 2 * difference);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_return_keeps_the_native_completion_during_iterator_close() {
    let code = crate::compile(&crate::parse(ITERATOR_CLOSE).unwrap()).unwrap();
    let small = input_sample(&code, "tiny", false, NativeFunction::AsyncGeneratorReturn);
    let large = input_sample(
        &code,
        "extended",
        false,
        NativeFunction::AsyncGeneratorReturn,
    );
    // Queue, awaited operand, native completion and independent temporary VM root.
    let difference = 2 * ("extended".len() - "tiny".len());
    assert_eq!(large - small, 4 * difference);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_throw_keeps_the_native_completion_during_iterator_close() {
    let code = crate::compile(&crate::parse(ITERATOR_CLOSE).unwrap()).unwrap();
    let small = input_sample(&code, "tiny", false, NativeFunction::AsyncGeneratorThrow);
    let large = input_sample(
        &code,
        "extended",
        false,
        NativeFunction::AsyncGeneratorThrow,
    );
    let difference = 2 * ("extended".len() - "tiny".len());
    // Queue, native throw and the pending-completion/close temporary copies.
    assert_eq!(large - small, 4 * difference);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_finally_handoff_keeps_both_vm_copies_without_a_native_duplicate() {
    let code = crate::compile(&crate::parse(FINALLY_BODY).unwrap()).unwrap();
    let small = input_sample(&code, "tiny", false, NativeFunction::AsyncGeneratorReturn);
    let large = input_sample(
        &code,
        "extended",
        false,
        NativeFunction::AsyncGeneratorReturn,
    );
    // Queue, awaited operand and the two VM completion fields; native input has moved.
    let difference = 2 * ("extended".len() - "tiny".len());
    assert_eq!(large - small, 4 * difference);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_failed_original_owner_keeps_the_unattempted_request() {
    for neutral in [false, true] {
        let mut vm = Vm::default();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let calls = Rc::new(Cell::new(0));
        let output = calls.clone();
        vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
            output.set(output.get() + 1);
            Ok(crate::HostValue::Undefined)
        })
        .unwrap();
        vm.execute_script(&crate::compile(&crate::parse(NEXT_BODY).unwrap()).unwrap())
            .unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
        let generator = iterator.object_id().unwrap();
        vm.set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)
            .unwrap();
        let target = vm
            .async_generator_request(
                &iterator,
                Value::String("tiny".into()),
                NativeFunction::AsyncGeneratorReturn,
            )
            .unwrap()
            .object_id()
            .unwrap();
        vm.set_async_generator_status(generator, AsyncGeneratorStatus::SuspendedYield)
            .unwrap();
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        let view = observer(&vm);
        if neutral {
            drop(vm.host_scheduling.take());
        }
        let _ = scope.fail();
        assert_eq!(
            vm.resume_async_generator_next(generator),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(calls.get(), 0);
        let queue = vm
            .heap
            .async_generator_control_ref(generator)
            .unwrap()
            .unwrap();
        assert_eq!(queue.requests.len(), 1);
        assert_eq!(queue.requests.front().unwrap().target, target);
        assert!(matches!(
            vm.promises[&target].status,
            PromiseStatus::Pending
        ));
        assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
        drop(vm);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
        assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    }
}

#[cfg_attr(test, test)]
fn async_generator_inputs_owned_context_roots_an_object_and_releases_it_after_gc() {
    let mut source = Vm::default();
    source.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = neutral.heap.alloc_object(None).unwrap();
    let input = NativeValue::adopt(Value::Object(object), owner.as_ref()).unwrap();
    input
        .with_owned_context(&mut neutral, |vm, input| {
            assert!(vm.argument_list_owner()?.unwrap().belongs_to(&scope));
            vm.collect_realm_garbage()?;
            assert!(vm.heap.contains(input.value().object_id().unwrap()));
            Ok(())
        })
        .unwrap();
    assert!(neutral.current_realm_root_owner().unwrap().is_none());
    neutral.collect_realm_garbage().unwrap();
    assert!(!neutral.heap.contains(object));
}

#[cfg_attr(test, test)]
fn async_generator_inputs_foreign_scoped_consumer_refuses_before_callback() {
    let mut source = Vm::default();
    source.use_bounded_host_microtask_checkpoints().unwrap();
    let before = source.host_scheduling_snapshot().payload_bytes;
    let owner = source.argument_list_owner().unwrap();
    let input = NativeValue::adopt(Value::String("tiny".into()), owner.as_ref()).unwrap();
    let mut target = Vm::default();
    target.use_bounded_host_microtask_checkpoints().unwrap();
    let called = Cell::new(false);
    assert_eq!(
        input.with_owned_context(&mut target, |_, _| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!called.get());
    assert!(source.check_scheduling_failure().is_ok());
    assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_neutral_promise_keeps_transferred_data_after_vm_drop() {
    let mut source = Vm::default();
    source.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let input = NativeValue::adopt(Value::String("tiny".into()), owner.as_ref()).unwrap();
    let mut neutral = Vm::default();
    let promise = neutral
        .promise_resolve_retained(input)
        .unwrap()
        .object_id()
        .unwrap();
    assert!(
        matches!(&neutral.promises[&promise].status, PromiseStatus::Fulfilled(Value::String(value)) if value == &JsString::from("tiny"))
    );
    let records = std::mem::take(&mut neutral.promises);
    let bytes = records.retained_bytes()
        + records
            .values()
            .map(PromiseRecord::status_payload_bytes)
            .sum::<usize>();
    drop(neutral);
    drop(source);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_scalar_completion_copy_admits_its_own_buffer_before_cloning() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let original = Completion::Return(Value::String("tiny".into()));
    let bytes = host_scheduling::value_payload_bytes(&Value::String("tiny".into()));
    let storage = Some(
        vm.host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge(bytes)
            .unwrap(),
    );
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (copy, peak) =
        scope.measure_payload_peak(|| generator_input::copy_scalar_completion(&original, &storage));
    let (copy, copied_storage) = copy.unwrap();
    assert_eq!(peak, bytes);
    drop(original);
    drop(storage);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copy);
    drop(copied_storage);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn async_generator_inputs_fifo_gc_and_completed_throw_keep_their_public_results() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let code = crate::compile(&crate::parse("var iterator=(async function*(){var sent=yield 7;return sent})();var first=iterator.next();0;").unwrap()).unwrap();
    vm.execute_script(&code).unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    vm.collect_realm_garbage().unwrap();
    let iterator = vm.lookup_global_name("iterator").unwrap().unwrap();
    let target = vm
        .async_generator_request(
            &iterator,
            Value::String("tiny".into()),
            NativeFunction::AsyncGeneratorNext,
        )
        .unwrap()
        .object_id()
        .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    let PromiseStatus::Fulfilled(Value::Object(result)) = vm.promises[&target].status else {
        panic!("the resumed generator must fulfill its target");
    };
    assert_eq!(
        vm.heap.get_own(result, "value").unwrap(),
        Some(Value::String("tiny".into()))
    );
    let rejected = vm
        .async_generator_request(
            &iterator,
            Value::String("reason".into()),
            NativeFunction::AsyncGeneratorThrow,
        )
        .unwrap()
        .object_id()
        .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(
        matches!(&vm.promises[&rejected].status, PromiseStatus::Rejected(Value::String(value)) if value == &JsString::from("reason"))
    );
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_async_generator_input_contracts() {
        async_generator_inputs_delegate_getter_retains_one_independent_head_copy();
        async_generator_inputs_neutral_delegate_getter_keeps_the_original_head_account();
        async_generator_inputs_fifo_gc_and_completed_throw_keep_their_public_results();
        async_generator_inputs_next_handoff_charges_the_stack_buffer_once();
        async_generator_inputs_return_keeps_the_native_completion_during_iterator_close();
        async_generator_inputs_throw_keeps_the_native_completion_during_iterator_close();
        async_generator_inputs_finally_handoff_keeps_both_vm_copies_without_a_native_duplicate();
        async_generator_inputs_failed_original_owner_keeps_the_unattempted_request();
        async_generator_inputs_owned_context_roots_an_object_and_releases_it_after_gc();
        async_generator_inputs_foreign_scoped_consumer_refuses_before_callback();
        async_generator_inputs_neutral_promise_keeps_transferred_data_after_vm_drop();
        async_generator_inputs_scalar_completion_copy_admits_its_own_buffer_before_cloning();
    }
}
