// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny host-code transfers preserve cross-document reuse and native ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 10_000;
    vm
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn returned_code(vm: &mut Vm, code: &Bytecode) -> SharedBytecode {
    let object = vm.execute_script(code).unwrap().object_id().unwrap();
    vm.heap.closure_code(object).unwrap().unwrap()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn host_code_storage_returned_function_has_native_source_ownership() {
    let mut vm = page();
    let original = script("(function kept(){return 1;})");
    let code = returned_code(&mut vm, &original);
    assert!(code.runtime_source_bytes() > 0);
    assert_eq!(original.functions[0].runtime_source_bytes(), 0);
}
#[cfg_attr(test, test)]
fn host_code_storage_shared_child_keeps_lease_after_vm_and_input_drop() {
    let mut vm = page();
    let view = observer(&vm);
    let original = script("(function kept(){return 1;})");
    let code = returned_code(&mut vm, &original);
    let minimum = code.retained_bytes() + code.source_text.allocation().unwrap().len();
    drop(original);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn host_code_storage_independent_documents_keep_distinct_code_owners() {
    let original = script("(function kept(){return 1;})");
    let mut first = page();
    let mut second = page();
    let a = returned_code(&mut first, &original);
    let b = returned_code(&mut second, &original);
    assert!(!std::ptr::eq(a.as_ref(), b.as_ref()));
}
#[cfg(test)]
#[test]
fn host_code_storage_refusal_precedes_code_copy_and_heap_publication() {
    let mut vm = page();
    let original = script("(function kept(){return 1;})");
    let code = original.functions[0].clone();
    let prototype = vm.object_prototype;
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.heap
            .alloc_closure(code, Vec::new(), Value::Undefined, prototype)
    });
    assert_eq!(result, Err(HeapError::CodeSourceStorageLimit));
    assert_eq!(allocations, 0);
}

#[cfg_attr(test, test)]
fn host_code_storage_weak_cache_keeps_the_dead_code_header_charged() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let budget = vm.host_scheduling.as_ref().unwrap().heap_native_budget();
    let cache = HostCodeCache::shared(&budget).unwrap();
    let original = script("(function kept(){return 1;})");
    let code = HostCodeCache::prepare(&cache, &original.functions[0], &budget).unwrap();
    drop(code);
    let backing = cache.borrow().charge.bytes();
    assert!(
        vm.host_scheduling_snapshot().payload_bytes > before + backing,
        "dead code's allocation header is retained by a weak cache entry"
    );
    cache.borrow_mut().prune();
    assert!(cache.borrow().is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + header_bytes()
    );
    drop(cache);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

fn setup_cache(vm: &Vm) -> (HeapNativeBudget, Rc<RefCell<HostCodeCache>>) {
    let budget = vm.host_scheduling.as_ref().unwrap().heap_native_budget();
    let cache = HostCodeCache::shared(&budget).unwrap();
    (budget, cache)
}

#[cfg_attr(test, test)]
fn host_code_storage_repeated_execution_reuses_one_document_copy() {
    let mut vm = page();
    let original = script("(function kept(){return 1;})");
    let first = returned_code(&mut vm, &original);
    let second = returned_code(&mut vm, &original);
    assert!(std::ptr::eq(first.as_ref(), second.as_ref()));
    assert!(Arc::ptr_eq(
        first.source_text.allocation().unwrap(),
        original.functions[0].source_text.allocation().unwrap()
    ));
}

#[cfg_attr(test, test)]
fn host_code_storage_related_heaps_share_one_document_copy() {
    let mut first = page();
    let original = script("(function kept(){return 1;})");
    let a = returned_code(&mut first, &original);
    let mut second = Vm {
        host_scheduling: first.host_scheduling.as_ref().map(SchedulingScope::child),
        bounded_host_microtask_checkpoints: true,
        ..Vm::default()
    };
    second
        .heap
        .inherit_document_heap_budget(&first.heap)
        .unwrap();
    second.use_bounded_host_microtask_checkpoints().unwrap();
    let b = returned_code(&mut second, &original);
    assert!(std::ptr::eq(a.as_ref(), b.as_ref()));
}

#[cfg_attr(test, test)]
fn host_code_storage_nested_code_owns_one_shared_source() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let (budget, cache) = setup_cache(&vm);
    let original = script("(function outer(){return function inner(){return 1;};})");
    let code = HostCodeCache::prepare(&cache, &original.functions[0], &budget).unwrap();
    assert_eq!(code.functions.len(), 1);
    assert_eq!(cache.borrow().sources.len(), 1);
    assert!(code.functions[0].is_runtime());
    assert_eq!(
        code.runtime_source_bytes(),
        code.functions[0].runtime_source_bytes()
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before
            + cache.borrow().charge.bytes()
            + code.retained_bytes()
            + code.functions[0].retained_bytes()
            + code.runtime_source_bytes()
    );
}

#[cfg_attr(test, test)]
fn host_code_storage_external_weak_source_keeps_its_header_after_cache_drop() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let (budget, cache) = setup_cache(&vm);
    let text = Arc::<str>::from("tiny");
    let source = HostCodeCache::source(&cache, &text, &budget).unwrap();
    let weak = source.weak();
    drop(source);
    cache.borrow_mut().prune();
    assert!(weak.upgrade().is_none());
    drop(cache);
    assert!(vm.host_scheduling_snapshot().payload_bytes > before);
    drop(weak);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_code_storage_copy_peak_has_exact_and_one_short_admission() {
    let vm = page();
    let (budget, cache) = setup_cache(&vm);
    let original = script("(function kept(){return 1;})");
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope
        .measure_payload_peak(|| HostCodeCache::prepare(&cache, &original.functions[0], &budget));
    drop(result.unwrap());
    drop(cache);
    for accepts in [false, true] {
        let mut vm = page();
        let (budget, cache) = setup_cache(&vm);
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = HostCodeCache::prepare(&cache, &original.functions[0], &budget);
        if accepts {
            drop(result.unwrap());
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        cache.borrow_mut().prune();
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn host_code_storage_cached_hit_needs_no_new_native_headroom() {
    let mut vm = page();
    let original = script("(function kept(){return 1;})");
    let _kept = returned_code(&mut vm, &original);
    let prototype = vm.object_prototype;
    let peer = leave_room(&mut vm, 0);
    let object = vm
        .heap
        .alloc_closure(
            original.functions[0].clone(),
            Vec::new(),
            Value::Undefined,
            prototype,
        )
        .unwrap();
    assert!(vm.heap.contains(object));
    assert!(vm.check_scheduling_failure().is_ok());
    drop(peer);
}

#[cfg_attr(test, test)]
fn host_code_storage_terminal_cached_hit_preserves_existing_entries() {
    let vm = page();
    let (budget, cache) = setup_cache(&vm);
    let original = script("(function kept(){return 1;})");
    let code = HostCodeCache::prepare(&cache, &original.functions[0], &budget).unwrap();
    budget.fail();
    assert_eq!(
        HostCodeCache::prepare(&cache, &original.functions[0], &budget).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert_eq!(cache.borrow().code.len(), 1);
    drop(code);
    cache.borrow_mut().prune();
    assert!(cache.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn host_code_storage_foreign_cache_refuses_without_publication() {
    let vm = page();
    let (_, cache) = setup_cache(&vm);
    let other = page();
    let budget = other.host_scheduling.as_ref().unwrap().heap_native_budget();
    let original = script("(function kept(){return 1;})");
    assert_eq!(
        HostCodeCache::prepare(&cache, &original.functions[0], &budget).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert!(cache.borrow().is_empty());
    assert!(vm.check_scheduling_failure().is_ok());
}

#[cfg_attr(test, test)]
fn host_code_storage_runtime_owned_code_keeps_its_existing_owner() {
    let vm = page();
    let (budget, cache) = setup_cache(&vm);
    let original = script("(function kept(){return 1;})");
    let code = HostCodeCache::prepare(&cache, &original.functions[0], &budget).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let same = HostCodeCache::prepare(&cache, &code, &budget).unwrap();
    assert!(std::ptr::eq(code.as_ref(), same.as_ref()));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_code_storage_host_edit_invalidates_its_cached_identity() {
    let vm = page();
    let (budget, cache) = setup_cache(&vm);
    let original = script("(function kept(){return 1;})");
    let mut host = original.functions[0].clone();
    drop(original);
    let first = HostCodeCache::prepare(&cache, &host, &budget).unwrap();
    let identity = host.host_identity();
    host.edit(|code| code.function_length = 2).unwrap();
    assert_ne!(host.host_identity(), identity);
    let second = HostCodeCache::prepare(&cache, &host, &budget).unwrap();
    assert_eq!(first.function_length, 0);
    assert_eq!(second.function_length, 2);
    assert!(!std::ptr::eq(first.as_ref(), second.as_ref()));
    assert_eq!(cache.borrow().sources.len(), 1);
}

#[cfg_attr(test, test)]
fn host_code_storage_cached_edit_has_exact_and_one_short_admission() {
    let vm = page();
    let (budget, cache) = setup_cache(&vm);
    let original = script("(function kept(){return 1;})");
    let mut code = HostCodeCache::prepare(&cache, &original.functions[0], &budget).unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| code.edit(|code| code.function_length = 2));
    result.unwrap();
    drop(code);
    drop(cache);
    for accepts in [false, true] {
        let mut vm = page();
        let (budget, cache) = setup_cache(&vm);
        let mut code = HostCodeCache::prepare(&cache, &original.functions[0], &budget).unwrap();
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = code.edit(|code| code.function_length = 2);
        if accepts {
            result.unwrap();
            assert_eq!(code.function_length, 2);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(code.function_length, 0);
        }
        drop(peer);
        drop(code);
        cache.borrow_mut().prune();
        assert!(cache.borrow().is_empty());
    }
}

#[cfg_attr(test, test)]
fn host_code_storage_sparse_reclamation_can_decline_without_terminal_failure() {
    let mut vm = page();
    let (budget, cache) = setup_cache(&vm);
    let originals: Vec<_> = (0..4)
        .map(|i| script(&format!("(function kept(){{return {i};}})")))
        .collect();
    let mut copies: Vec<_> = originals
        .iter()
        .map(|code| HostCodeCache::prepare(&cache, &code.functions[0], &budget).unwrap())
        .collect();
    copies.truncate(1);
    {
        // A sparse state after dead weak entries are removed but compaction
        // was deferred. Every retained backing slot still has its old charge.
        let mut cache = cache.borrow_mut();
        cache.code.retain(|_, entry| entry.compiled.live());
        cache.sources.retain(|_, entry| entry.live());
    }
    assert_eq!(cache.borrow().code.capacity(), 7);
    let peer = leave_room(&mut vm, 0);
    cache.borrow_mut().prune();
    assert_eq!(cache.borrow().code.capacity(), 7);
    assert!(vm.check_scheduling_failure().is_ok());
    drop(peer);
    cache.borrow_mut().prune();
    assert_eq!(cache.borrow().code.capacity(), 3);
    assert_eq!(cache.borrow().sources.capacity(), 3);
}

/// A table's `capacity()` drops when removals leave tombstones and rises again
/// when later insertions reuse them, with no reallocation. The ledger follows
/// the real allocation, so pruning never asks for more than it holds. Each
/// iteration builds fresh tables, hence a fresh random hash seed.
#[cfg_attr(test, test)]
fn host_code_storage_prune_ignores_tombstone_capacity_drift() {
    for round in 0..200 {
        let vm = page();
        let (budget, cache) = setup_cache(&vm);
        let originals: Vec<_> = (0..40)
            .map(|i| script(&format!("(function kept(){{return {round}+{i};}})")))
            .collect();
        let prepare = |range: std::ops::Range<usize>| -> Vec<SharedBytecode> {
            range
                .map(|i| {
                    HostCodeCache::prepare(&cache, &originals[i].functions[0], &budget).unwrap()
                })
                .collect()
        };
        let mut live = prepare(0..22);
        live.drain(..16);
        cache.borrow_mut().prune();
        live.extend(prepare(22..40));
        cache.borrow_mut().prune();
        live.clear();
        cache.borrow_mut().prune();
        assert!(vm.check_scheduling_failure().is_ok());
    }
}

fn queued(timer: bool) {
    let mut vm = page();
    if timer {
        vm.install_host_timer_functions().unwrap();
    }
    let function = vm
        .execute_script(&script(
            "var result=0;var callback=function(){result+=1;};callback;",
        ))
        .unwrap()
        .object_id()
        .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    assert!(code.runtime_source_bytes() > 0);
    let source = Arc::downgrade(code.source_text.allocation().unwrap());
    drop(code);
    vm.execute_script(&script(if timer {
        "setTimeout(callback,1);callback=undefined;0;"
    } else {
        "var pending=Promise.withResolvers();pending.promise.then(callback);callback=undefined;0;"
    }))
    .unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(function));
    if timer {
        assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    } else {
        vm.execute_script(&script("pending.resolve();pending=undefined;0;"))
            .unwrap();
        vm.run_promise_jobs_bounded(4).unwrap();
    }
    assert_eq!(
        vm.execute_script(&script("result;")).unwrap(),
        Value::Number(1.0)
    );
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(function));
    assert!(source.upgrade().is_none());
}
#[cfg_attr(test, test)]
fn host_code_storage_timer_retains_code_until_delivery_and_gc() {
    queued(true);
}
#[cfg_attr(test, test)]
fn host_code_storage_promise_retains_code_until_delivery_and_gc() {
    queued(false);
}

#[cfg_attr(test, test)]
fn host_code_storage_late_activation_copies_without_binding_host_input() {
    let mut vm = Vm::default();
    let original = script("(function kept(){return 1;})");
    let object = vm.execute_script(&original).unwrap().object_id().unwrap();
    assert!(std::ptr::eq(
        vm.heap.closure_code(object).unwrap().unwrap().as_ref(),
        original.functions[0].as_ref()
    ));
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let code = vm.heap.closure_code(object).unwrap().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(!std::ptr::eq(code.as_ref(), original.functions[0].as_ref()));
    let view = observer(&vm);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        code.retained_bytes() + code.runtime_source_bytes()
    );
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn start(code: SharedBytecode) -> crate::heap::GeneratorState {
    crate::heap::GeneratorState::Start {
        code,
        captures: Vec::new(),
        callee: Value::Undefined,
        receiver: Value::Undefined,
        args: Vec::new(),
        home: None,
        with_objects: Vec::new(),
    }
}
#[cfg_attr(test, test)]
fn host_code_storage_generator_creation_and_state_restoration_adopt_code() {
    let mut vm = page();
    let original = script("(function* kept(){yield 1;})");
    let prototype = vm.object_prototype;
    let object = vm
        .heap
        .alloc_generator(start(original.functions[0].clone()), prototype)
        .unwrap();
    let root = vm.heap.root(object).unwrap();
    let first = vm.heap.take_generator_state(object).unwrap();
    let crate::heap::GeneratorState::Start { code, .. } = first else {
        panic!("start state");
    };
    assert!(code.runtime_source_bytes() > 0);
    let replacement = script("(function* replaced(){yield 2;})");
    vm.heap
        .set_generator_state(object, start(replacement.functions[0].clone()))
        .unwrap();
    let crate::heap::GeneratorState::Start { code, .. } =
        vm.heap.take_generator_state(object).unwrap()
    else {
        panic!("restored start");
    };
    assert!(code.runtime_source_bytes() > 0);
    assert!(!std::ptr::eq(
        code.as_ref(),
        replacement.functions[0].as_ref()
    ));
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn host_code_storage_async_saved_code_keeps_lease_after_vm_drop() {
    let mut vm = page();
    let view = observer(&vm);
    vm.execute_script(&script(
        "var hold=Promise.withResolvers();(async function(){await hold.promise;return 1;})();0;",
    ))
    .unwrap();
    let code = vm.async_continuations.values().next().unwrap().code.clone();
    assert!(code.runtime_source_bytes() > 0);
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_code_storage_template_identity_and_function_text_survive_copying() {
    let mut vm = page();
    assert_eq!(vm.execute_script(&script("var first;function tag(s){if(first===undefined)first=s;return first===s;}function f(){return tag`x`;} f()&&f()&&f.toString()==='function f(){return tag`x`;}';")).unwrap(),Value::Bool(true));
}

#[cfg_attr(test, test)]
fn host_code_storage_wrapped_runtime_sources_do_not_escape_to_another_document() {
    let first = page();
    let input = crate::vm::eval_compilation_storage::ParsedEvalInput::parse_script(
        "(function(){return 1;})",
        first.host_scheduling.as_ref(),
    )
    .unwrap();
    let code = input
        .compile_with(first.host_scheduling.as_ref(), crate::compile)
        .unwrap();
    let wrapper = SharedBytecode::new((*code).clone());
    let mut other = page();
    let prototype = other.object_prototype;
    assert_eq!(
        other
            .heap
            .alloc_closure(wrapper, Vec::new(), Value::Undefined, prototype),
        Err(HeapError::CodeSourceStorageLimit)
    );
    assert!(first.check_scheduling_failure().is_ok());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_code_storage_contracts() {
        host_code_storage_returned_function_has_native_source_ownership();
        host_code_storage_shared_child_keeps_lease_after_vm_and_input_drop();
        host_code_storage_independent_documents_keep_distinct_code_owners();
        host_code_storage_weak_cache_keeps_the_dead_code_header_charged();
        host_code_storage_repeated_execution_reuses_one_document_copy();
        host_code_storage_related_heaps_share_one_document_copy();
        host_code_storage_nested_code_owns_one_shared_source();
        host_code_storage_external_weak_source_keeps_its_header_after_cache_drop();
        host_code_storage_copy_peak_has_exact_and_one_short_admission();
        host_code_storage_cached_hit_needs_no_new_native_headroom();
        host_code_storage_terminal_cached_hit_preserves_existing_entries();
        host_code_storage_foreign_cache_refuses_without_publication();
        host_code_storage_runtime_owned_code_keeps_its_existing_owner();
        host_code_storage_host_edit_invalidates_its_cached_identity();
        host_code_storage_cached_edit_has_exact_and_one_short_admission();
        host_code_storage_sparse_reclamation_can_decline_without_terminal_failure();
        host_code_storage_timer_retains_code_until_delivery_and_gc();
        host_code_storage_promise_retains_code_until_delivery_and_gc();
        host_code_storage_late_activation_copies_without_binding_host_input();
        host_code_storage_generator_creation_and_state_restoration_adopt_code();
        host_code_storage_async_saved_code_keeps_lease_after_vm_drop();
        host_code_storage_template_identity_and_function_text_survive_copying();
        host_code_storage_wrapped_runtime_sources_do_not_escape_to_another_document();
    }
}
