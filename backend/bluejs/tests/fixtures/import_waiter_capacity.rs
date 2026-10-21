// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real import jobs, small waiting graphs and scalar shared-owner models.

use super::*;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[derive(Clone, Copy)]
enum Mode {
    EagerSame,
    EagerDistinct,
    Deferred,
}

fn fixture(mode: Mode, registered: usize, rejected: bool) -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    execute(
        &mut vm,
        "var gate=Promise.withResolvers(), targets=[]; function callback() {} 0;",
    );
    let count = if matches!(mode, Mode::EagerDistinct) {
        registered + 1
    } else {
        1
    };
    let mut registry = HashMap::new();
    for index in 0..count {
        let end = if rejected {
            "throw 7;"
        } else {
            "export const value=1;"
        };
        let code = crate::compile_module(
            &crate::parse_module(&format!("await gate.promise; {end}")).unwrap(),
        )
        .unwrap();
        registry.insert(format!("test/m{index}.mjs"), code);
    }
    if matches!(mode, Mode::Deferred) {
        registry.insert(
            "test/entry.mjs".into(),
            crate::compile_module(
                &crate::parse_module("import './m0.mjs'; export const value=1;").unwrap(),
            )
            .unwrap(),
        );
    }
    for index in 0..count {
        vm.execute_module_graph(&format!("test/m{index}.mjs"), &registry)
            .unwrap();
    }
    vm.set_module_loader_context("test/main.js", registry);
    let request = match mode {
        Mode::EagerSame => "import('./m0.mjs')",
        Mode::EagerDistinct => "import('./m'+i+'.mjs')",
        Mode::Deferred => "import.defer('./entry.mjs')",
    };
    execute(
        &mut vm,
        &format!(
            "for(var i=0;i<{};i++) targets.push({request}); 0;",
            registered + 1
        ),
    );
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    // Keep one ordinary job behind the import under test so popping it does
    // not refund the queue's slot storage and hide missing admission.
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback });
    for _ in 0..registered {
        assert_eq!(vm.run_next_promise_job(), Ok(true));
    }
    assert_eq!(vm.promise_jobs.len(), 2);
    // This fixture exercises waiter admission after job entry. Prepare its
    // separate root storage before filling the scalar budget, so an earlier
    // root-capacity refusal does not hide the waiter boundary under test.
    let mut references = 0;
    vm.visit_promise_job_references(vm.promise_jobs.front().unwrap(), &mut |_| references += 1);
    vm.prepare_async_frame_root_slots(references).unwrap();
    vm
}

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    // These bytes are represented numerically, not allocated by the fixture.
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

/// The job entry roots the queued job's edges before it pops it, and those
/// temporary roots are charged to the same account.
fn roots_bytes(vm: &Vm) -> usize {
    let mut references = 0;
    vm.visit_promise_job_references(vm.promise_jobs.front().unwrap(), &mut |_| references += 1);
    crate::vm::realm_call_storage::RealmCallRoots::new(references, None)
        .unwrap()
        .retained_bytes()
}

fn shape(vm: &Vm) -> (usize, usize, usize, usize) {
    (
        vm.module_import_waiters.len(),
        vm.module_import_waiters.capacity(),
        vm.module_import_waiters
            .values()
            .map(|waiters| waiters.len())
            .sum(),
        vm.deferred_import_waiters.len(),
    )
}

fn module_import_snapshot(vm: &Vm) -> HashMap<String, Vec<ObjectId>> {
    vm.module_import_waiters
        .iter()
        .map(|(name, waiters)| (name.clone(), waiters.as_slice().to_vec()))
        .collect()
}

fn assert_refused(vm: &mut Vm) {
    let before = shape(vm);
    let references = module_import_snapshot(vm);
    let deferred_references: Vec<_> = vm
        .deferred_import_waiters
        .iter()
        .map(|waiter| (waiter.promise, waiter.namespace, waiter.pending.clone()))
        .collect();
    let capacities: Vec<_> = vm
        .module_import_waiters
        .values()
        .map(|waiters| waiters.capacity())
        .collect();
    let deferred_capacity = vm.deferred_import_waiters.capacity();
    let count = vm.host_scheduling_snapshot().pending_reactions;
    assert_eq!(
        vm.run_next_promise_job(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(shape(vm), before);
    assert_eq!(
        vm.module_import_waiters
            .values()
            .map(|waiters| waiters.capacity())
            .collect::<Vec<_>>(),
        capacities
    );
    assert_eq!(vm.deferred_import_waiters.capacity(), deferred_capacity);
    assert_eq!(vm.host_scheduling_snapshot().pending_reactions, count);
    assert_eq!(module_import_snapshot(vm), references);
    assert_eq!(
        vm.deferred_import_waiters
            .iter()
            .map(|waiter| (waiter.promise, waiter.namespace, waiter.pending.clone()))
            .collect::<Vec<_>>(),
        deferred_references
    );
    assert_eq!(vm.promise_jobs.len(), 1);
}

#[cfg_attr(test, test)]
fn first_import_waiter_is_refused_before_a_new_key_or_vector_is_published() {
    let mut vm = fixture(Mode::EagerSame, 0, false);
    let roots = roots_bytes(&vm);
    let _owner = leave_room(&mut vm, roots);
    assert_refused(&mut vm);
}

#[cfg_attr(test, test)]
fn import_waiter_map_growth_is_refused_before_publication() {
    let mut vm = fixture(Mode::EagerDistinct, 7, false);
    assert_eq!(
        vm.module_import_waiters.len(),
        vm.module_import_waiters.capacity()
    );
    let roots = roots_bytes(&vm);
    let _owner = leave_room(&mut vm, roots);
    assert_refused(&mut vm);
}

#[cfg_attr(test, test)]
fn import_waiter_vector_growth_is_refused_before_publication() {
    let mut vm = fixture(Mode::EagerSame, 8, false);
    let waiters = &vm.module_import_waiters["test/m0.mjs"];
    assert_eq!(waiters.len(), waiters.capacity());
    let roots = roots_bytes(&vm);
    let _owner = leave_room(&mut vm, roots);
    assert_refused(&mut vm);
}

#[cfg_attr(test, test)]
fn import_waiter_existing_slots_do_not_recharge_the_module_name() {
    let mut vm = fixture(Mode::EagerSame, 5, false);
    let capacity = vm.module_import_waiters["test/m0.mjs"].capacity();
    assert!(capacity > 5);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (preview, peak) = scope.measure_payload_peak(|| {
        vm.dynamic_import_job(
            "test/main.js",
            "./m0.mjs",
            ModuleType::JavaScript,
            ImportPhase::Evaluation,
        )
    });
    assert!(matches!(preview, Ok(DynamicImportResult::Waiting(_))));
    drop(preview);
    let job_bytes = vm.promise_jobs.front().unwrap().scheduling_payload_bytes();
    let roots = roots_bytes(&vm);
    let _owner = leave_room(&mut vm, peak + roots);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(vm.run_next_promise_job(), Ok(true));
    assert_eq!(vm.module_import_waiters["test/m0.mjs"].len(), 6);
    assert_eq!(vm.module_import_waiters["test/m0.mjs"].capacity(), capacity);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - job_bytes
    );
}

fn assert_count_refusal(mode: Mode) {
    let mut vm = fixture(mode, 5, false);
    let used = vm.host_scheduling_usage().unwrap().pending_reactions;
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_reactions: MAX_PENDING_HOST_REACTIONS - used,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
    .unwrap();
    assert_refused(&mut vm);
}

#[cfg_attr(test, test)]
fn import_waiter_shared_count_refusal_precedes_publication() {
    assert_count_refusal(Mode::EagerSame);
}

#[cfg_attr(test, test)]
fn deferred_waiter_shared_count_refusal_precedes_publication() {
    assert_count_refusal(Mode::Deferred);
}

#[cfg_attr(test, test)]
fn deferred_waiter_growth_is_refused_before_dependencies_or_entries_are_retained() {
    let mut vm = fixture(Mode::Deferred, 8, false);
    assert_eq!(
        vm.deferred_import_waiters.len(),
        vm.deferred_import_waiters.capacity()
    );
    let roots = roots_bytes(&vm);
    let _owner = leave_room(&mut vm, roots);
    assert_refused(&mut vm);
}

#[cfg_attr(test, test)]
fn deferred_waiter_existing_slots_charge_complete_dependency_staging() {
    let mut vm = fixture(Mode::Deferred, 5, false);
    let capacity = vm.deferred_import_waiters.capacity();
    assert!(capacity > 5);
    // Inspect one fixed loader result before applying the scalar allowance.
    // The queued import still runs through the ordinary job entry below.
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (preview, producer_peak) = scope.measure_payload_peak(|| {
        vm.dynamic_import_job(
            "test/main.js",
            "./entry.mjs",
            ModuleType::JavaScript,
            ImportPhase::Defer,
        )
    });
    let DynamicImportResult::WaitingDeferred { modules, .. } = preview.unwrap() else {
        unreachable!()
    };
    let bytes = host_scheduling::slots(&modules)
        + modules.iter().map(String::capacity).sum::<usize>()
        + HashSet::<String>::with_capacity(modules.len()).capacity()
            * std::mem::size_of::<String>();
    drop(modules);
    let job_bytes = vm.promise_jobs.front().unwrap().scheduling_payload_bytes();
    let roots = roots_bytes(&vm);
    let _owner = leave_room(&mut vm, bytes.max(producer_peak) + roots);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(vm.run_next_promise_job(), Ok(true));
    assert_eq!(vm.deferred_import_waiters.len(), 6);
    assert_eq!(vm.deferred_import_waiters.capacity(), capacity);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - job_bytes + vm.deferred_import_waiters[5].pending.retained_bytes(),
    );
}

fn assert_cleanup(mode: Mode) {
    for rejected in [false, true] {
        let mut vm = fixture(mode, 1, rejected);
        execute(
            &mut vm,
            "var weak = new WeakRef(targets[0]); targets=null; 0;",
        );
        vm.run_promise_jobs_bounded(32).unwrap();
        vm.collect_realm_garbage().unwrap();
        assert_eq!(
            execute(&mut vm, "weak.deref() !== undefined;"),
            Value::Bool(true)
        );
        execute(&mut vm, "gate.resolve(); 0;");
        vm.run_promise_jobs_bounded(32).unwrap();
        assert!(vm.module_import_waiters.is_empty());
        assert!(vm.deferred_import_waiters.is_empty());
        assert_eq!(vm.module_import_waiters.capacity(), 0);
        assert_eq!(vm.deferred_import_waiters.capacity(), 0);
        assert_eq!(vm.host_scheduling_usage().unwrap().pending_reactions, 0);
        // `weak.deref()` keeps its target until the job's checkpoint ends.
        vm.run_host_microtask_checkpoint().unwrap();
        vm.collect_realm_garbage().unwrap();
        assert_eq!(
            execute(&mut vm, "weak.deref() === undefined;"),
            Value::Bool(true)
        );
    }
}

#[cfg_attr(test, test)]
fn completed_and_rejected_imports_release_wait_storage_and_unreferenced_promises() {
    assert_cleanup(Mode::EagerSame);
}

#[cfg_attr(test, test)]
fn completed_and_rejected_deferred_imports_release_wait_storage_and_unreferenced_promises() {
    assert_cleanup(Mode::Deferred);
}

#[cfg_attr(test, test)]
fn deferred_partial_completion_releases_obsolete_dependency_storage() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    execute(
        &mut vm,
        "var gates=[]; for(var i=0;i<7;i++) gates.push(Promise.withResolvers()); 0;",
    );
    let mut registry = HashMap::new();
    let mut entry = String::new();
    for index in 0..7 {
        let code = crate::compile_module(
            &crate::parse_module(&format!(
                "await gates[{index}].promise; export const value=1;"
            ))
            .unwrap(),
        )
        .unwrap();
        registry.insert(format!("test/m{index}.mjs"), code);
        entry.push_str(&format!("import './m{index}.mjs';"));
    }
    registry.insert(
        "test/entry.mjs".into(),
        crate::compile_module(&crate::parse_module(&entry).unwrap()).unwrap(),
    );
    for index in 0..7 {
        vm.execute_module_graph(&format!("test/m{index}.mjs"), &registry)
            .unwrap();
    }
    vm.set_module_loader_context("test/main.js", registry);
    let target = execute(&mut vm, "var target=import.defer('./entry.mjs'); target;")
        .object_id()
        .unwrap();
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(vm.deferred_import_waiters.len(), 1);
    assert_eq!(vm.deferred_import_waiters[0].pending.len(), 7);
    let capacity = vm.deferred_import_waiters[0].pending.capacity();
    execute(&mut vm, "for(var i=0;i<6;i++) gates[i].resolve(); 0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    let pending = &vm.deferred_import_waiters[0].pending;
    assert_eq!(pending.len(), 1);
    assert!(pending.contains("test/m6.mjs"));
    assert!(pending.capacity() < capacity);
    assert!(matches!(
        vm.promises[&target].status,
        PromiseStatus::Pending
    ));
    assert_eq!(vm.host_scheduling_usage().unwrap().pending_reactions, 2);
    execute(&mut vm, "gates[6].resolve(); 0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(vm.deferred_import_waiters.capacity(), 0);
    assert!(matches!(
        vm.promises[&target].status,
        PromiseStatus::Fulfilled(_)
    ));
}

#[cfg_attr(test, test)]
fn namespace_root_refusal_preserves_pending_import_owners() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    execute(
        &mut vm,
        "var gate=Promise.withResolvers(), hold=Promise.withResolvers(); 0;",
    );
    let registry: HashMap<_, _> = [
        ("test/root.mjs", "import './m0.mjs'; await hold.promise;"),
        ("test/m0.mjs", "await gate.promise; export const value=1;"),
    ]
    .into_iter()
    .map(|(name, source)| {
        (
            name.into(),
            crate::compile_module(&crate::parse_module(source).unwrap()).unwrap(),
        )
    })
    .collect();
    vm.execute_module_graph("test/root.mjs", &registry).unwrap();
    vm.set_module_loader_context("test/main.js", registry);
    execute(&mut vm, "var target=import('./m0.mjs'); 0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    execute(&mut vm, "gate.resolve(); 0;");
    let references = module_import_snapshot(&vm);
    let capacity = vm.module_import_waiters.capacity();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.run_next_promise_job(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(module_import_snapshot(&vm), references);
    assert_eq!(vm.module_import_waiters.capacity(), capacity);
}

impl crate::Vm {
    /// Runs import waiter admission/lifetime contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_import_waiter_capacity_contracts() {
        first_import_waiter_is_refused_before_a_new_key_or_vector_is_published();
        import_waiter_map_growth_is_refused_before_publication();
        import_waiter_vector_growth_is_refused_before_publication();
        import_waiter_existing_slots_do_not_recharge_the_module_name();
        import_waiter_shared_count_refusal_precedes_publication();
        deferred_waiter_shared_count_refusal_precedes_publication();
        deferred_waiter_growth_is_refused_before_dependencies_or_entries_are_retained();
        deferred_waiter_existing_slots_charge_complete_dependency_staging();
        completed_and_rejected_imports_release_wait_storage_and_unreferenced_promises();
        completed_and_rejected_deferred_imports_release_wait_storage_and_unreferenced_promises();
        deferred_partial_completion_releases_obsolete_dependency_storage();
        namespace_root_refusal_preserves_pending_import_owners();
    }
}
