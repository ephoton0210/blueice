// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny source-phase imports return document refusal without host panics.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn fixture(with_reaction: bool) -> (Vm, ObjectId) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let source = if with_reaction {
        "var pending=new Promise(()=>{}); pending.then(()=>0); pending;"
    } else {
        "var pending=new Promise(()=>{}); pending;"
    };
    let promise = execute(&mut vm, source).object_id().unwrap();
    vm.active_module_name = Some("test/main.mjs".into());
    vm.set_module_source_loader_context(vec!["test/object.wasm".into()]);
    (vm, promise)
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg(test)]
#[test]
fn import_source_admission_refuses_before_resolver_allocation() {
    let (mut vm, promise) = fixture(false);
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript)
    });
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), before);
    assert!(vm.module_source_cache.is_empty());
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
}

#[cfg_attr(test, test)]
fn import_source_admission_rejection_fanout_refusal_is_returned_without_panicking() {
    let (mut vm, promise) = fixture(true);
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
    .unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::Json)
    }));
    assert!(
        outcome.is_ok(),
        "source-phase rejection must return admission errors"
    );
    assert_eq!(outcome.unwrap(), Err(RuntimeError::SchedulingLimit));
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert_eq!(vm.promises[&promise].reactions.len(), 1);
    assert!(vm.promise_jobs.is_empty());
}

#[cfg(test)]
#[test]
fn import_source_admission_terminal_scope_precedes_resolver_and_heap_work() {
    let (mut vm, promise) = fixture(false);
    let before = vm.heap.stats();
    vm.host_scheduling.as_ref().unwrap().fail();
    let (result, allocations) = allocation_observer::count(|| {
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript)
    });
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), before);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
}

fn full_microtask_peer(vm: &Vm) -> SchedulingScope {
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
    .unwrap();
    peer
}
fn source_object(vm: &mut Vm) -> ObjectId {
    vm.module_source_object("test/object.wasm", &HashMap::new())
        .unwrap()
}
fn publish_source(vm: &mut Vm, source: ObjectId) {
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.define_data(
        global,
        "sourceObject",
        Value::Object(source),
        true,
        true,
        true,
    )
    .unwrap();
}

#[cfg_attr(test, test)]
fn import_source_admission_fulfillment_fanout_refusal_is_returned() {
    let (mut vm, promise) = fixture(true);
    let source = source_object(&mut vm);
    let _peer = full_microtask_peer(&vm);
    assert_eq!(
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert_eq!(vm.promises[&promise].reactions.len(), 1);
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(vm.module_source_cache["test/object.wasm"], source);
}

#[cfg_attr(test, test)]
fn import_source_admission_thenable_job_refusal_is_returned_without_calling_then() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    publish_source(&mut vm, source);
    let calls = Rc::new(std::cell::Cell::new(0));
    let output = calls.clone();
    vm.install_host_function("started", 0, move |_args: &[HostValue]| {
        output.set(output.get() + 1);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    execute(
        &mut vm,
        "sourceObject.then=function(resolve){started(); resolve(7);}; 0;",
    );
    let _peer = full_microtask_peer(&vm);
    assert_eq!(
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert!(vm.promise_jobs.is_empty());
}

#[cfg_attr(test, test)]
fn import_source_admission_no_job_fulfillment_accepts_a_full_shared_count() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    let _peer = full_microtask_peer(&vm);
    assert_eq!(
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript),
        Ok(())
    );
    assert!(
        matches!(vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == source)
    );
    assert!(vm.promise_jobs.is_empty());
    vm.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn import_source_admission_no_job_rejection_accepts_a_full_shared_count() {
    let (mut vm, promise) = fixture(false);
    let _peer = full_microtask_peer(&vm);
    assert_eq!(
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::Text),
        Ok(())
    );
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Rejected(Value::Object(_))
    ));
    assert!(vm.promise_jobs.is_empty());
    vm.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn import_source_admission_cached_lookup_refusal_preserves_identity_and_roots() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    let root = vm.module_source_roots["test/object.wasm"];
    let capacities = (
        vm.module_source_cache.capacity(),
        vm.module_source_roots.capacity(),
    );
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_source_cache["test/object.wasm"], source);
    assert_eq!(vm.module_source_roots["test/object.wasm"], root);
    assert_eq!(
        (
            vm.module_source_cache.capacity(),
            vm.module_source_roots.capacity()
        ),
        capacities
    );
    assert_eq!(vm.heap.stats(), before);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
}

#[cfg_attr(test, test)]
fn import_source_admission_normalized_requests_share_the_static_source_identity() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    assert_eq!(
        vm.dynamic_import_source(promise, "./part/../object.wasm", ModuleType::JavaScript),
        Ok(())
    );
    assert!(
        matches!(vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == source)
    );
    assert_eq!(vm.module_source_cache.len(), 1);
}

#[cfg_attr(test, test)]
fn import_source_admission_cached_resolution_refunds_its_temporary_name() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript)
        .unwrap();
    let next = vm.new_promise().unwrap();
    let root = vm.heap.root(next).unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.dynamic_import_source(next, "./object.wasm", ModuleType::JavaScript)
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(
        matches!(vm.promises[&next].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == source)
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn import_source_admission_then_getter_throw_preserves_the_original_reason() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    publish_source(&mut vm, source);
    let reason = execute(
        &mut vm,
        "var reason={}; Object.defineProperty(sourceObject,'then',{get(){throw reason;}}); reason;",
    );
    vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript)
        .unwrap();
    assert!(
        matches!(&vm.promises[&promise].status, PromiseStatus::Rejected(value) if *value == reason)
    );
    assert!(vm.promise_jobs.is_empty());
}

#[cfg_attr(test, test)]
fn import_source_admission_then_getter_collection_keeps_source_and_promise_live() {
    let (mut vm, promise) = fixture(false);
    let source = source_object(&mut vm);
    publish_source(&mut vm, source);
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let prototype = vm.function_prototype().unwrap();
    vm.install_native(
        global,
        prototype,
        "collectNow",
        0,
        NativeFunction::Test262("gc"),
    )
    .unwrap();
    execute(
        &mut vm,
        "Object.defineProperty(sourceObject,'then',{get(){collectNow(); return undefined;}}); 0;",
    );
    vm.dynamic_import_source(promise, "./object.wasm", ModuleType::JavaScript)
        .unwrap();
    assert!(
        matches!(vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == source)
    );
    assert_eq!(vm.module_source_cache["test/object.wasm"], source);
}

#[cfg_attr(test, test)]
fn import_source_admission_host_neutral_calls_keep_their_existing_behavior() {
    let mut vm = Vm::default();
    let promise = execute(&mut vm, "var pending=new Promise(()=>{}); pending;")
        .object_id()
        .unwrap();
    vm.set_module_source_loader_context(vec!["object.wasm".into()]);
    assert!(vm.host_scheduling.is_none());
    vm.dynamic_import_source(promise, "object.wasm", ModuleType::JavaScript)
        .unwrap();
    assert!(
        matches!(vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Object(id)) if id == vm.module_source_cache["object.wasm"])
    );
}

#[cfg_attr(test, test)]
fn import_source_admission_public_source_promise_keeps_normal_reaction_delivery() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.set_module_source_loader_context(vec!["test/object.wasm".into()]);
    vm.set_module_loader_context("test/main.js", HashMap::new());
    execute(&mut vm, "var delivered=false, imported; import.source('./object.wasm').then(value=>{delivered=true; imported=value;}); 0;");
    assert_eq!(
        vm.lookup_global_name("delivered").unwrap(),
        Some(Value::Bool(false))
    );
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        vm.lookup_global_name("delivered").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(
        vm.lookup_global_name("imported").unwrap(),
        Some(Value::Object(vm.module_source_cache["test/object.wasm"]))
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_import_source_admission_contracts() {
        import_source_admission_rejection_fanout_refusal_is_returned_without_panicking();
        import_source_admission_fulfillment_fanout_refusal_is_returned();
        import_source_admission_thenable_job_refusal_is_returned_without_calling_then();
        import_source_admission_no_job_fulfillment_accepts_a_full_shared_count();
        import_source_admission_no_job_rejection_accepts_a_full_shared_count();
        import_source_admission_cached_lookup_refusal_preserves_identity_and_roots();
        import_source_admission_normalized_requests_share_the_static_source_identity();
        import_source_admission_cached_resolution_refunds_its_temporary_name();
        import_source_admission_then_getter_throw_preserves_the_original_reason();
        import_source_admission_then_getter_collection_keeps_source_and_promise_live();
        import_source_admission_host_neutral_calls_keep_their_existing_behavior();
        import_source_admission_public_source_promise_keeps_normal_reaction_delivery();
    }
}
