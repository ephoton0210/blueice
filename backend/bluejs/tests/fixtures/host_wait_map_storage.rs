// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny suspended/collected maps retain actual backing and original ownership.

use super::*;
use std::cell::Cell;

type Calls = Rc<RefCell<Vec<HostPromiseToken>>>;
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
fn install(vm: &mut Vm) -> Calls {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        move |token: HostPromiseToken, _: &[HostValue]| {
            output.borrow_mut().push(token);
            Ok(())
        },
    )
    .unwrap();
    vm.global("Promise").unwrap();
    calls
}
fn call(vm: &mut Vm) -> Result<Value, RuntimeError> {
    vm.host_async_function_call(0, Value::Undefined, &[], false)
}
fn bytes(vm: &Vm) -> usize {
    vm.promises.retained_bytes() + vm.host_promises.pending.retained_bytes()
}

#[cfg_attr(test, test)]
fn host_wait_map_storage_scoped_tables_keep_credit_after_vm_destruction() {
    let mut vm = page();
    let _calls = install(&mut vm);
    call(&mut vm).unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm);
    let maps = (
        std::mem::take(&mut vm.host_promises.pending),
        std::mem::take(&mut vm.promises),
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(maps);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_borrowed_tables_keep_the_registration_account() {
    let source = page();
    let caller = page();
    let view = observer(&source);
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let _calls = loan(&mut vm, &source, |vm| Ok(install(vm))).unwrap();
    loan(&mut vm, &caller, call).unwrap();
    let retained = bytes(&vm);
    let maps = (
        std::mem::take(&mut vm.host_promises.pending),
        std::mem::take(&mut vm.promises),
    );
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + retained
    );
    drop(maps);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_native_start_observes_backing_credit_in_the_home_account() {
    let source = page();
    let caller = page();
    let view = Rc::new(observer(&source));
    let baseline = Rc::new(Cell::new(0));
    let observed = Rc::new(Cell::new(0));
    let (output, before, seen) = (view.clone(), baseline.clone(), observed.clone());
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| {
        vm.install_host_async_function(
            "wait",
            0,
            HostTaskSource::UserInteraction,
            move |_: HostPromiseToken, _: &[HostValue]| {
                seen.set(output.host_scheduling_snapshot().payload_bytes - before.get());
                Ok(())
            },
        )
    })
    .unwrap();
    vm.global("Promise").unwrap();
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    loan(&mut vm, &caller, call).unwrap();
    let minimum = std::mem::size_of::<(HostPromiseToken, PendingHostPromise)>()
        + std::mem::size_of::<(ObjectId, PromiseRecord)>();
    assert!(
        observed.get() >= minimum,
        "home backing credit: {}",
        observed.get()
    );
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_ordinary_scoped_promise_table_outlives_the_vm() {
    let mut vm = page();
    vm.new_promise().unwrap();
    let view = observer(&vm);
    let retained = vm.promises.retained_bytes();
    let table = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(table);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_ordinary_borrowed_promise_table_outlives_the_vm() {
    let source = page();
    let view = observer(&source);
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    loan(&mut vm, &source, |vm| vm.new_promise().map(Value::Object)).unwrap();
    let retained = vm.promises.retained_bytes();
    let table = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + retained
    );
    drop(table);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_gc_reclaims_backing_without_losing_survivor_credit() {
    let source = page();
    let caller = page();
    let view = observer(&source);
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let calls = loan(&mut vm, &source, |vm| Ok(install(vm))).unwrap();
    for _ in 0..5 {
        loan(&mut vm, &caller, call).unwrap();
    }
    let tokens = calls.borrow().clone();
    for token in tokens.into_iter().skip(1) {
        assert!(vm
            .queue_host_promise_completion(
                token,
                HostPromiseCompletion::Resolve(HostValue::Undefined)
            )
            .unwrap());
    }
    vm.run_host_task_turn(0, 4).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert_eq!(vm.promises.len(), 1);
    assert_eq!(vm.host_promises.pending.len(), 1);
    let retained = bytes(&vm);
    let maps = (
        std::mem::take(&mut vm.host_promises.pending),
        std::mem::take(&mut vm.promises),
    );
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + retained
    );
    drop(maps);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_terminal_cleanup_frees_backing_after_its_data() {
    let source = page();
    let view = observer(&source);
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let _calls = loan(&mut vm, &source, |vm| Ok(install(vm))).unwrap();
    loan(&mut vm, &source, call).unwrap();
    let retained = bytes(&vm);
    let mut waits = std::mem::take(&mut vm.host_promises.pending);
    let mut promises = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + retained
    );
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    waits.retain(|_, _| false);
    promises.retain(|_, _| false);
    waits.release_empty();
    promises.release_empty();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_policy_discovery_checks_tables_without_a_live_registry() {
    let source = page();
    let mut vm = Vm::default();
    let _calls = loan(&mut vm, &source, |vm| Ok(install(vm))).unwrap();
    loan(&mut vm, &source, call).unwrap();
    let _waits = std::mem::take(&mut vm.host_promises.pending);
    let _functions = std::mem::take(&mut vm.host_promises.functions);
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_one_failed_entry_preserves_another_healthy_completion() {
    let source = page();
    let first = page();
    let second = page();
    let mut vm = Vm::default();
    let calls = loan(&mut vm, &source, |vm| Ok(install(vm))).unwrap();
    loan(&mut vm, &first, call).unwrap();
    let promise = loan(&mut vm, &second, call).unwrap().object_id().unwrap();
    let tokens = calls.borrow().clone();
    let _ = first.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.queue_host_promise_completion(
            tokens[0],
            HostPromiseCompletion::Resolve(HostValue::Undefined)
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .queue_host_promise_completion(
            tokens[1],
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    vm.run_host_task_turn(0, 1).unwrap();
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Fulfilled(Value::Number(7.0))
    ));
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_ordinary_promise_values_and_reactions_are_preserved() {
    let mut vm = page();
    vm.execute_script(
        &crate::compile(
            &crate::parse("var result;Promise.resolve(7).then(value=>result=value);0;").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        vm.execute_script(&crate::compile(&crate::parse("result;").unwrap()).unwrap()),
        Ok(Value::Number(7.0))
    );
}

#[cfg_attr(test, test)]
fn host_wait_map_storage_existing_field_credit_moves_without_a_duplicate_peak() {
    let mut vm = page();
    let key = vm.new_promise().unwrap();
    let record = vm.promises.remove(&key).unwrap();
    let mut legacy = retained_control_map::RetainedControlMap::default();
    let prepared =
        promise_queue_storage::PreparedMap::for_insert(&legacy, vm.host_scheduling.as_ref())
            .unwrap();
    prepared.commit(&mut legacy);
    legacy.insert(key, record);
    vm.promises = legacy;
    vm.synchronize_scheduling_usage().unwrap();
    assert!(!vm.promises.has_owner());
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let owner = vm.argument_list_owner().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) =
        scope.measure_payload_peak(|| vm.retain_host_wait_map_storage(owner.as_ref()));
    result.unwrap();
    assert_eq!(peak, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.promises.unowned_bytes(), 0);
    assert!(vm.promises.has_owner() && vm.host_promises.pending.has_owner());
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_foreign_scope_refusal_preserves_original_backing() {
    let source = page();
    let other = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    loan(&mut vm, &source, |vm| vm.new_promise().map(Value::Object)).unwrap();
    let before = vm.promises.retained_bytes();
    let result = promise_queue_storage::PreparedMap::for_insert(
        &vm.promises,
        other.host_scheduling.as_ref(),
    );
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert_eq!(vm.promises.retained_bytes(), before);
    assert!(vm
        .promises
        .owner_view()
        .unwrap()
        .unwrap()
        .belongs_to(source.host_scheduling.as_ref().unwrap()));
    assert!(source.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_spare_capacity_still_retains_terminal_identity() {
    let source = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    loan(&mut vm, &source, |vm| vm.new_promise().map(Value::Object)).unwrap();
    let prepared = promise_queue_storage::PreparedMap::for_insert(&vm.promises, None).unwrap();
    assert_eq!(prepared.staging_bytes(), 0);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        prepared.validate_owner(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promises.len(), 1);
}

struct DropStamp {
    view: Rc<Vm>,
    seen: Rc<Cell<usize>>,
}
impl Drop for DropStamp {
    fn drop(&mut self) {
        self.seen
            .set(self.view.host_scheduling_snapshot().payload_bytes);
    }
}
fn native_map<K: Eq + std::hash::Hash, V>(
    source: &Vm,
) -> retained_control_map::RetainedControlMap<K, V> {
    let mut map = retained_control_map::RetainedControlMap::default();
    map.install_storage(source.argument_list_owner().unwrap().unwrap());
    map
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_value_destructors_observe_complete_backing_credit() {
    let source = page();
    let view = Rc::new(observer(&source));
    let before = view.host_scheduling_snapshot().payload_bytes;
    let seen = Rc::new(Cell::new(0));
    let mut map = native_map::<u64, DropStamp>(&source);
    for key in 0..5 {
        let prepared = promise_queue_storage::PreparedMap::for_insert(&map, None).unwrap();
        prepared.commit(&mut map);
        map.insert(
            key,
            DropStamp {
                view: view.clone(),
                seen: seen.clone(),
            },
        );
    }
    let backing = map.retained_bytes();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + backing
    );
    drop(map);
    assert_eq!(seen.get(), before + backing);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_owned_compaction_keeps_survivors_and_terminal_cleanup() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut map = native_map::<u64, u64>(&source);
    for key in 0..5 {
        promise_queue_storage::PreparedMap::for_insert(&map, None)
            .unwrap()
            .commit(&mut map);
        map.insert(key, key + 1);
    }
    let original = map.retained_bytes();
    map.retain(|key, _| *key == 0);
    assert_eq!(map.retained_bytes(), original);
    assert!(map.reclaim(None).is_none());
    assert!(map.retained_bytes() < original);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + map.retained_bytes()
    );
    assert_eq!(map.get(&0), Some(&1));
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert!(map.prepare_compaction(None).is_none());
    map.retain(|_, _| false);
    map.release_empty();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert!(matches!(
        map.owner_view(),
        Err(RuntimeError::SchedulingLimit)
    ));
}

struct HashFault {
    id: u64,
    fail: Rc<Cell<bool>>,
}
impl PartialEq for HashFault {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for HashFault {}
impl std::hash::Hash for HashFault {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        assert!(!self.fail.get(), "fixed hash fault");
        std::hash::Hash::hash(&self.id, state);
    }
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_hash_unwind_destroys_candidate_data_before_its_credit() {
    let source = page();
    let view = Rc::new(observer(&source));
    let before = view.host_scheduling_snapshot().payload_bytes;
    let seen = Rc::new(Cell::new(0));
    let fail = Rc::new(Cell::new(false));
    let mut map = native_map::<HashFault, DropStamp>(&source);
    for id in 0..3 {
        promise_queue_storage::PreparedMap::for_insert(&map, None)
            .unwrap()
            .commit(&mut map);
        map.insert(
            HashFault {
                id,
                fail: fail.clone(),
            },
            DropStamp {
                view: view.clone(),
                seen: seen.clone(),
            },
        );
    }
    let old = map.retained_bytes();
    let prepared = promise_queue_storage::PreparedMap::for_insert(&map, None).unwrap();
    let incoming = prepared.staging_bytes();
    fail.set(true);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepared.commit(&mut map)))
            .is_err()
    );
    assert_eq!(seen.get(), before + old + incoming);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before + old);
    assert!(map.is_empty());
    map.release_empty();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert!(source.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_existing_map_owner_is_checked_after_native_start() {
    let original = page();
    let home = page();
    let caller = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    loan(&mut vm, &original, |vm| vm.new_promise().map(Value::Object)).unwrap();
    let terminate = original.host_scheduling.as_ref().unwrap().child();
    loan(&mut vm, &home, |vm| {
        vm.install_host_async_function(
            "wait",
            0,
            HostTaskSource::UserInteraction,
            move |_: HostPromiseToken, _: &[HostValue]| {
                let _ = terminate.fail();
                Ok(())
            },
        )
    })
    .unwrap();
    let roots = vm.heap.root_storage_state().0;
    assert_eq!(
        loan(&mut vm, &caller, call),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state().0, roots);
    assert!(home.argument_list_owner().is_ok() && caller.argument_list_owner().is_ok());
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_replacement_candidate_retains_credit_after_vm_destruction() {
    let mut vm = page();
    for _ in 0..3 {
        vm.new_promise().unwrap();
    }
    assert_eq!(vm.promises.len(), vm.promises.capacity());
    let view = observer(&vm);
    let prepared =
        promise_queue_storage::PreparedMap::for_insert(&vm.promises, vm.host_scheduling.as_ref())
            .unwrap();
    let incoming = prepared.staging_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, incoming);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_empty_collected_table_keeps_its_original_owner() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    vm.global("Promise").unwrap();
    loan(&mut vm, &source, |vm| vm.new_promise().map(Value::Object)).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.promises.is_empty());
    assert_eq!(vm.promises.retained_bytes(), 0);
    assert!(vm
        .promises
        .owner_view()
        .unwrap()
        .unwrap()
        .belongs_to(source.host_scheduling.as_ref().unwrap()));
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        loan(&mut vm, &caller, |vm| vm.new_promise().map(Value::Object)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(caller.argument_list_owner().is_ok());
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
#[cfg_attr(test, test)]
fn host_wait_map_storage_manual_timer_completion_and_major_gc_keep_backing_owned() {
    let mut vm = page();
    let calls = install(&mut vm);
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(
        &mut vm,
        "var result=false;setTimeout(()=>wait().then(value=>{$262.gc();result=value===7;}),1);0;",
    );
    assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    assert!(vm
        .queue_host_promise_completion(
            calls.borrow()[0],
            HostPromiseCompletion::Resolve(HostValue::Number(7.0))
        )
        .unwrap());
    assert_eq!(vm.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut vm, "result;"), Value::Bool(true));
    let view = observer(&vm);
    let retained = vm.promises.retained_bytes();
    let table = std::mem::take(&mut vm.promises);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(table);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_wait_map_storage_contracts() {
        host_wait_map_storage_scoped_tables_keep_credit_after_vm_destruction();
        host_wait_map_storage_borrowed_tables_keep_the_registration_account();
        host_wait_map_storage_native_start_observes_backing_credit_in_the_home_account();
        host_wait_map_storage_ordinary_scoped_promise_table_outlives_the_vm();
        host_wait_map_storage_ordinary_borrowed_promise_table_outlives_the_vm();
        host_wait_map_storage_gc_reclaims_backing_without_losing_survivor_credit();
        host_wait_map_storage_terminal_cleanup_frees_backing_after_its_data();
        host_wait_map_storage_policy_discovery_checks_tables_without_a_live_registry();
        host_wait_map_storage_one_failed_entry_preserves_another_healthy_completion();
        host_wait_map_storage_ordinary_promise_values_and_reactions_are_preserved();
        host_wait_map_storage_existing_field_credit_moves_without_a_duplicate_peak();
        host_wait_map_storage_foreign_scope_refusal_preserves_original_backing();
        host_wait_map_storage_spare_capacity_still_retains_terminal_identity();
        host_wait_map_storage_value_destructors_observe_complete_backing_credit();
        host_wait_map_storage_owned_compaction_keeps_survivors_and_terminal_cleanup();
        host_wait_map_storage_hash_unwind_destroys_candidate_data_before_its_credit();
        host_wait_map_storage_existing_map_owner_is_checked_after_native_start();
        host_wait_map_storage_replacement_candidate_retains_credit_after_vm_destruction();
        host_wait_map_storage_empty_collected_table_keeps_its_original_owner();
        host_wait_map_storage_manual_timer_completion_and_major_gc_keep_backing_owned();
    }
}
