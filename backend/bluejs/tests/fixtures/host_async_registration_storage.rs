// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed async registration, cancellation and completion ownership contracts.

use super::*;
use std::cell::Cell;

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
fn loan<T>(
    vm: &mut Vm,
    source: &Vm,
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}
fn object_count(vm: &Vm) -> usize {
    let stats = vm.heap.stats();
    stats.nursery_objects + stats.tenured_objects
}
fn registry_bytes(vm: &Vm) -> usize {
    vm.host_promises.functions.capacity() * std::mem::size_of::<Registration>()
        + vm.host_promises
            .functions
            .iter()
            .map(|record| match &record.operation {
                Operation::Function(callback) => std::mem::size_of_val(&**callback),
                Operation::Method { method, .. } => std::mem::size_of_val(&**method),
            })
            .sum::<usize>()
}
fn install_both(vm: &mut Vm) {
    let family = vm.create_host_object_family().unwrap();
    let data = [7u64, 4];
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        move |_: HostPromiseToken, _: &[HostValue]| {
            assert_eq!(data[0] + data[1], 11);
            Ok(())
        },
    )
    .unwrap();
    let data = [7u64];
    vm.install_host_async_object_method(
        family,
        "wait",
        0,
        HostTaskSource::UserInteraction,
        move |_: HostPromiseToken, key: HostObjectKey, _: &[HostValue]| {
            assert!(key.matches_owner(data[0], 4));
            Ok(())
        },
    )
    .unwrap();
}

#[derive(Clone)]
struct Calls {
    starts: Rc<Cell<usize>>,
    cancels: Rc<Cell<usize>>,
    token: Rc<Cell<Option<HostPromiseToken>>>,
}
impl Calls {
    fn new() -> Self {
        Self {
            starts: Rc::new(Cell::new(0)),
            cancels: Rc::new(Cell::new(0)),
            token: Rc::new(Cell::new(None)),
        }
    }
    fn probe(&self, terminate: Option<SchedulingScope>, error: bool) -> Probe {
        Probe {
            calls: self.clone(),
            terminate,
            error,
        }
    }
    fn token(&self) -> HostPromiseToken {
        self.token.get().unwrap()
    }
}
struct Probe {
    calls: Calls,
    terminate: Option<SchedulingScope>,
    error: bool,
}
impl HostAsyncFunction for Probe {
    fn start(&mut self, token: HostPromiseToken, _: &[HostValue]) -> Result<(), HostFunctionError> {
        self.calls.starts.set(self.calls.starts.get() + 1);
        self.calls.token.set(Some(token));
        if let Some(scope) = &self.terminate {
            let _ = scope.fail();
        }
        if self.error {
            Err(HostFunctionError::new("tiny error"))
        } else {
            Ok(())
        }
    }
    fn cancel(&mut self, token: HostPromiseToken) {
        assert_eq!(token.heap, self.calls.token().heap);
        self.calls.cancels.set(self.calls.cancels.get() + 1);
    }
}
fn install(
    vm: &mut Vm,
    source: &Vm,
    calls: &Calls,
    terminate: Option<SchedulingScope>,
    error: bool,
) {
    loan(vm, source, |vm| {
        vm.install_host_async_function(
            "wait",
            0,
            HostTaskSource::UserInteraction,
            calls.probe(terminate, error),
        )
    })
    .unwrap();
    // Measure request roots after the intrinsic's permanent root exists.
    vm.global("Promise").unwrap();
}
fn call(vm: &mut Vm) -> Result<Value, RuntimeError> {
    vm.host_async_function_call(0, Value::Undefined, &[], false)
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_callbacks_and_slots_outlive_the_vm() {
    let mut vm = page();
    install_both(&mut vm);
    let view = observer(&vm);
    let bytes = registry_bytes(&vm);
    let functions = std::mem::take(&mut vm.host_promises.functions);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_neutral_records_adopt_backing_and_boxes() {
    let mut vm = Vm::default();
    install_both(&mut vm);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = registry_bytes(&vm);
    let functions = std::mem::take(&mut vm.host_promises.functions);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_terminal_caller_precedes_cold_global_allocation() {
    let source = page();
    let mut vm = Vm::default();
    let objects = object_count(&vm);
    let result = loan(&mut vm, &source, |vm| {
        let _ = source.host_scheduling.as_ref().unwrap().fail();
        vm.install_host_async_function(
            "wait",
            0,
            HostTaskSource::UserInteraction,
            |_: HostPromiseToken, _: &[HostValue]| Ok(()),
        )
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(object_count(&vm), objects);
    assert!(vm.host_promises.functions.is_empty());
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_original_owner_stops_global_publication() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let objects = object_count(&vm);
    let result = loan(&mut vm, &caller, |vm| {
        vm.install_host_async_function(
            "next",
            0,
            HostTaskSource::UserInteraction,
            |_: HostPromiseToken, _: &[HostValue]| Ok(()),
        )
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.host_promises.functions.len(), 1);
    assert_eq!(object_count(&vm), objects);
    assert!(caller.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_original_registry_stops_method_publication() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let family = vm.create_host_object_family().unwrap();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let result = loan(&mut vm, &caller, |vm| {
        vm.install_host_async_object_method(
            family,
            "next",
            0,
            HostTaskSource::UserInteraction,
            |_: HostPromiseToken, _: HostObjectKey, _: &[HostValue]| Ok(()),
        )
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.host_promises.functions.len(), 1);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_terminal_registry_stops_start_before_promise_allocation() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let objects = object_count(&vm);
    let roots = vm.heap.root_storage_state().0;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        loan(&mut vm, &caller, call),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.starts.get(), 0);
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert_eq!(object_count(&vm), objects);
}
fn cancellation_return(original: bool, error: bool) {
    let source = page();
    let caller = page();
    let terminate = if original { &source } else { &caller }
        .host_scheduling
        .as_ref()
        .unwrap()
        .child();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, Some(terminate), error);
    let roots = vm.heap.root_storage_state().0;
    assert_eq!(
        loan(&mut vm, &caller, call),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.starts.get(), 1);
    assert_eq!(calls.cancels.get(), usize::from(!error));
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert!(if original { &caller } else { &source }
        .argument_list_owner()
        .is_ok());
    drop(vm);
    assert_eq!(calls.cancels.get(), usize::from(!error));
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_original_cancellation_discards_success_and_cancels_started_work()
{
    cancellation_return(true, false);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_original_cancellation_precedes_native_error_processing() {
    cancellation_return(true, true);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_caller_cancellation_cancels_successfully_started_work() {
    cancellation_return(false, false);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_caller_cancellation_preserves_start_error_cleanup() {
    cancellation_return(false, true);
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_queue_refusal_cancels_original_document_work() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let roots = vm.heap.root_storage_state().0;
    loan(&mut vm, &caller, call).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.cancels.get(), 1);
    assert!(vm.host_promises.pending.is_empty() && vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert!(caller.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_queue_refusal_only_cancels_the_terminal_entry_owner() {
    let source = page();
    let first = page();
    let second = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    loan(&mut vm, &first, call).unwrap();
    let first_token = calls.token();
    let second_promise = loan(&mut vm, &second, call).unwrap().object_id().unwrap();
    let second_token = calls.token();
    let _ = first.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.queue_host_promise_completion(
            first_token,
            HostPromiseCompletion::Resolve(HostValue::Number(1.0))
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.cancels.get(), 1);
    assert_eq!(vm.host_promises.pending.len(), 1);
    assert!(vm
        .queue_host_promise_completion(
            second_token,
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert!(matches!(
        vm.promises[&second_promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(calls.cancels.get(), 1);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_queued_delivery_observes_original_cancellation() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let roots = vm.heap.root_storage_state().0;
    loan(&mut vm, &caller, call).unwrap();
    assert!(vm
        .queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::String("tiny".into()))
        )
        .unwrap());
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        vm.run_host_task_turn(0, 1),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(calls.cancels.get(), 1);
    assert!(vm.host_promises.pending.is_empty() && vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    drop(vm);
    assert_eq!(calls.cancels.get(), 1);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_normal_completion_rejects_duplicates_and_preserves_values() {
    let source = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let promise = loan(&mut vm, &source, call).unwrap().object_id().unwrap();
    let token = calls.token();
    assert!(vm
        .queue_host_promise_completion(
            token,
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    assert!(!vm
        .queue_host_promise_completion(
            token,
            HostPromiseCompletion::Resolve(HostValue::Number(1.0))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
    assert!(!vm
        .queue_host_promise_completion(
            token,
            HostPromiseCompletion::Resolve(HostValue::Number(1.0))
        )
        .unwrap());
    assert_eq!(calls.cancels.get(), 0);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_vm_drop_cancels_terminal_pending_work_once() {
    let source = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    loan(&mut vm, &source, call).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    drop(vm);
    assert_eq!(calls.starts.get(), 1);
    assert_eq!(calls.cancels.get(), 1);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_policy_cannot_reassign_the_original_document() {
    let source = page();
    let mut vm = Vm::default();
    install(&mut vm, &source, &Calls::new(), None, false);
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_growth_retains_all_indices_and_callback_boxes() {
    let mut vm = page();
    for (index, name) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        let value = [index as u64];
        vm.install_host_async_function(
            name,
            0,
            HostTaskSource::UserInteraction,
            move |_: HostPromiseToken, _: &[HostValue]| {
                assert!(value[0] <= 4);
                Ok(())
            },
        )
        .unwrap();
    }
    assert_eq!(
        run(
            &mut vm,
            "a.name==='a'&&e.name==='e'&&a.length===0&&e.length===0;"
        ),
        Value::Bool(true)
    );
    let view = observer(&vm);
    let bytes = registry_bytes(&vm);
    let functions = std::mem::take(&mut vm.host_promises.functions);
    assert_eq!(functions.len(), 5);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

struct DropProbe {
    view: Rc<Vm>,
    seen: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}
impl HostAsyncFunction for DropProbe {
    fn start(&mut self, _: HostPromiseToken, _: &[HostValue]) -> Result<(), HostFunctionError> {
        Ok(())
    }
}
impl HostAsyncObjectMethod for DropProbe {
    fn start(
        &mut self,
        _: HostPromiseToken,
        _: HostObjectKey,
        _: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        Ok(())
    }
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.seen
            .set(self.view.host_scheduling_snapshot().payload_bytes);
        self.drops.set(self.drops.get() + 1);
    }
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_destructor_observes_live_callback_and_backing_credit() {
    let mut vm = page();
    let view = Rc::new(observer(&vm));
    let seen = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        DropProbe {
            view: view.clone(),
            seen: seen.clone(),
            drops: drops.clone(),
        },
    )
    .unwrap();
    let bytes = registry_bytes(&vm);
    let functions = std::mem::take(&mut vm.host_promises.functions);
    drop(vm);
    assert_eq!(drops.get(), 0);
    drop(functions);
    assert_eq!(drops.get(), 1);
    assert_eq!(seen.get(), bytes);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_failed_publication_releases_the_prepared_callback() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 4, 1)))
    })
    .unwrap();
    run(
        &mut vm,
        "Object.preventExtensions(Object.getPrototypeOf(target()));0;",
    );
    let view = Rc::new(observer(&vm));
    let drops = Rc::new(Cell::new(0));
    let result = vm.install_host_async_object_method(
        family,
        "wait",
        0,
        HostTaskSource::UserInteraction,
        DropProbe {
            view: view.clone(),
            seen: Rc::new(Cell::new(0)),
            drops: drops.clone(),
        },
    );
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert_eq!(drops.get(), 1);
    assert!(vm.host_promises.functions.is_empty());
    assert_eq!(
        run(
            &mut vm,
            "Object.hasOwn(Object.getPrototypeOf(target()),'wait');"
        ),
        Value::Bool(false)
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_original_cancellation_retires_all_queued_waits() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let roots = vm.heap.root_storage_state().0;
    loan(&mut vm, &caller, call).unwrap();
    let first = calls.token();
    loan(&mut vm, &caller, call).unwrap();
    let second = calls.token();
    for token in [first, second] {
        assert!(vm
            .queue_host_promise_completion(
                token,
                HostPromiseCompletion::Resolve(HostValue::String("tiny".into()))
            )
            .unwrap());
    }
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.queue_host_promise_completion(
            first,
            HostPromiseCompletion::Resolve(HostValue::Undefined)
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.cancels.get(), 2);
    assert!(vm.host_promises.pending.is_empty() && vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    drop(vm);
    assert_eq!(calls.cancels.get(), 2);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_terminal_current_caller_keeps_a_healthy_wait_pending() {
    let source = page();
    let owner = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let promise = loan(&mut vm, &owner, call).unwrap().object_id().unwrap();
    let token = calls.token();
    let result = loan(&mut vm, &caller, |vm| {
        let _ = caller.host_scheduling.as_ref().unwrap().fail();
        vm.queue_host_promise_completion(
            token,
            HostPromiseCompletion::Resolve(HostValue::Number(1.0)),
        )
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.host_promises.pending.len(), 1);
    assert!(!vm.host_promises.pending[&token].queued);
    assert_eq!(calls.cancels.get(), 0);
    assert!(vm
        .queue_host_promise_completion(
            token,
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_delivery_only_retires_the_terminal_entry_owner() {
    let source = page();
    let first = page();
    let second = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    let roots = vm.heap.root_storage_state().0;
    loan(&mut vm, &first, call).unwrap();
    let first_token = calls.token();
    let promise = loan(&mut vm, &second, call).unwrap().object_id().unwrap();
    let second_token = calls.token();
    for token in [first_token, second_token] {
        assert!(vm
            .queue_host_promise_completion(
                token,
                HostPromiseCompletion::Resolve(HostValue::Number(7.0))
            )
            .unwrap());
    }
    let _ = first.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        vm.run_host_task_turn(0, 1),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(calls.cancels.get(), 1);
    assert_eq!(vm.host_promises.pending.len(), 1);
    assert!(vm.host_promises.pending.contains_key(&second_token));
    assert_eq!(vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
    assert_eq!(vm.heap.root_storage_state().0, roots);
    drop(vm);
    assert_eq!(calls.cancels.get(), 1);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_neutral_entry_adopts_the_existing_registry() {
    let source = page();
    let view = observer(&source);
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let calls = Calls::new();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        calls.probe(None, false),
    )
    .unwrap();
    let bytes = registry_bytes(&vm);
    loan(&mut vm, &source, call).unwrap();
    assert!(vm
        .queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::Undefined)
        )
        .unwrap());
    vm.run_host_task_turn(0, 1).unwrap();
    let functions = std::mem::take(&mut vm.host_promises.functions);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + bytes
    );
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_terminal_preparation_stops_zero_sized_box_construction() {
    let vm = page();
    let view = observer(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let prepared = vm
        .host_promises
        .functions
        .prepare(1, owner.as_ref())
        .unwrap();
    let slots = prepared.staging_bytes();
    let built = Cell::new(false);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    let result = prepared.callbacks(&[0], || {
        built.set(true);
        [Registration {
            source: HostTaskSource::UserInteraction,
            operation: Operation::Function(Box::new(|_: HostPromiseToken, _: &[HostValue]| Ok(()))),
        }]
    });
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(!built.get());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_manual_timer_completion_and_promise_gc_preserve_values() {
    let mut vm = page();
    let calls = Calls::new();
    vm.install_host_async_function(
        "wait",
        1,
        HostTaskSource::UserInteraction,
        calls.probe(None, false),
    )
    .unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm,"var result=false;setTimeout(()=>{wait('tiny').then(value=>{$262.gc();result=value==='tiny';});},1);0;");
    assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    assert!(vm
        .queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::String("tiny".into()))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut vm, "result;"), Value::Bool(true));
    assert_eq!(calls.starts.get(), 1);
    assert_eq!(calls.cancels.get(), 0);
    assert!(vm.host_promises.pending.is_empty() && vm.realm_call_roots.is_none());
}
struct MethodProbe(Probe);
impl HostAsyncObjectMethod for MethodProbe {
    fn start(
        &mut self,
        token: HostPromiseToken,
        key: HostObjectKey,
        args: &[HostValue],
    ) -> Result<(), HostFunctionError> {
        assert!(key.matches_owner(7, 4));
        assert!(matches!(args, [HostValue::String(text)] if text == "tiny"));
        self.0.start(token, args)
    }
    fn cancel(&mut self, token: HostPromiseToken) {
        self.0.cancel(token);
    }
}
#[cfg_attr(test, test)]
fn host_async_registration_storage_public_object_method_preserves_brand_and_result() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 4, 1)))
    })
    .unwrap();
    let calls = Calls::new();
    vm.install_host_async_object_method(
        family,
        "wait",
        1,
        HostTaskSource::UserInteraction,
        MethodProbe(calls.probe(None, false)),
    )
    .unwrap();
    run(
        &mut vm,
        "var received;target().wait('tiny').then(value=>received=value);0;",
    );
    assert!(vm
        .queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut vm, "received;"), Value::Number(7.0));
    assert_eq!(calls.starts.get(), 1);
    assert_eq!(calls.cancels.get(), 0);
    assert!(vm.host_promises.pending.is_empty());
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_pending_count_belongs_to_registration_document() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    loan(&mut vm, &caller, call).unwrap();
    assert_eq!(source.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(caller.host_scheduling_snapshot().pending_reactions, 0);
    drop(vm);
    assert_eq!(source.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(calls.cancels.get(), 1);
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_pending_count_borrowed_state_outlives_vm() {
    let source = page();
    let mut vm = Vm::default();
    let calls = Calls::new();
    install(&mut vm, &source, &calls, None, false);
    call(&mut vm).unwrap();
    let state = std::mem::take(&mut vm.host_promises);
    drop(vm);
    assert_eq!(source.host_scheduling_snapshot().pending_reactions, 1);
    assert_eq!(calls.cancels.get(), 0);
    drop(state);
    assert_eq!(source.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(calls.cancels.get(), 1);
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_pending_count_scoped_state_outlives_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let calls = Calls::new();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        calls.probe(None, false),
    )
    .unwrap();
    call(&mut vm).unwrap();
    let state = std::mem::take(&mut vm.host_promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(state);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(calls.cancels.get(), 1);
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_pending_count_legacy_adoption_retains_count() {
    let mut vm = Vm::default();
    let calls = Calls::new();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        calls.probe(None, false),
    )
    .unwrap();
    call(&mut vm).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let state = std::mem::take(&mut vm.host_promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 1);
    drop(state);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(calls.cancels.get(), 1);
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_pending_count_delivery_releases_wait() {
    let mut vm = page();
    let calls = Calls::new();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        calls.probe(None, false),
    )
    .unwrap();
    let promise = call(&mut vm).unwrap().object_id().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    assert!(vm
        .queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    vm.run_browser_tasks_bounded(1).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 0);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
    assert_eq!(calls.cancels.get(), 0);
}

#[cfg_attr(test, test)]
fn host_async_registration_storage_pending_count_cancellation_releases_wait() {
    let mut vm = page();
    let calls = Calls::new();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        calls.probe(None, false),
    )
    .unwrap();
    call(&mut vm).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, 1);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.queue_host_promise_completion(
            calls.token(),
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    // The terminal VM boundary refuses publication; its owning state still
    // cancels and refunds the wait when it is destroyed.
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().pending_reactions, 0);
    assert_eq!(calls.cancels.get(), 1);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_async_registration_storage_contracts() {
        host_async_registration_storage_pending_count_belongs_to_registration_document();
        host_async_registration_storage_pending_count_borrowed_state_outlives_vm();
        host_async_registration_storage_pending_count_scoped_state_outlives_vm();
        host_async_registration_storage_pending_count_legacy_adoption_retains_count();
        host_async_registration_storage_pending_count_delivery_releases_wait();
        host_async_registration_storage_pending_count_cancellation_releases_wait();
        host_async_registration_storage_callbacks_and_slots_outlive_the_vm();
        host_async_registration_storage_neutral_records_adopt_backing_and_boxes();
        host_async_registration_storage_terminal_caller_precedes_cold_global_allocation();
        host_async_registration_storage_original_owner_stops_global_publication();
        host_async_registration_storage_original_registry_stops_method_publication();
        host_async_registration_storage_terminal_registry_stops_start_before_promise_allocation();
        host_async_registration_storage_original_cancellation_discards_success_and_cancels_started_work();
        host_async_registration_storage_original_cancellation_precedes_native_error_processing();
        host_async_registration_storage_caller_cancellation_cancels_successfully_started_work();
        host_async_registration_storage_caller_cancellation_preserves_start_error_cleanup();
        host_async_registration_storage_queue_refusal_cancels_original_document_work();
        host_async_registration_storage_queue_refusal_only_cancels_the_terminal_entry_owner();
        host_async_registration_storage_queued_delivery_observes_original_cancellation();
        host_async_registration_storage_normal_completion_rejects_duplicates_and_preserves_values();
        host_async_registration_storage_vm_drop_cancels_terminal_pending_work_once();
        host_async_registration_storage_policy_cannot_reassign_the_original_document();
        host_async_registration_storage_growth_retains_all_indices_and_callback_boxes();
        host_async_registration_storage_destructor_observes_live_callback_and_backing_credit();
        host_async_registration_storage_failed_publication_releases_the_prepared_callback();
        host_async_registration_storage_original_cancellation_retires_all_queued_waits();
        host_async_registration_storage_terminal_current_caller_keeps_a_healthy_wait_pending();
        host_async_registration_storage_delivery_only_retires_the_terminal_entry_owner();
        host_async_registration_storage_neutral_entry_adopts_the_existing_registry();
        host_async_registration_storage_terminal_preparation_stops_zero_sized_box_construction();
        host_async_registration_storage_manual_timer_completion_and_promise_gc_preserve_values();
        host_async_registration_storage_public_object_method_preserves_brand_and_result();
    }
}
