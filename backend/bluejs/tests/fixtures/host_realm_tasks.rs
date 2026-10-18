// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny mixed-realm document task cases without service or quota replay.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn owned(vm: &mut Vm) -> ObjectId {
    vm.install_test262_harness().unwrap();
    run(vm, "var realm=$262.createRealm();0;");
    *vm.test262_realms.keys().next().unwrap()
}
fn shadow(vm: &mut Vm) -> ShadowRealmRecord {
    let value = run(vm, "new ShadowRealm();");
    let id = value.object_id().unwrap();
    vm.heap.root(id).unwrap();
    vm.shadow_realms[&id].clone()
}
fn browser_timers(vm: &mut Vm) {
    // Replace the conformance harness names only in this owner-profile fixture.
    // Its independent Test262 timer queue is not a browser task source.
    run(vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
}
fn work(vm: &mut Vm) {
    browser_timers(vm);
    run(
        vm,
        "var seen=0;setTimeout(()=>{seen+=1;Promise.resolve().then(()=>{seen+=2;});},1);0;",
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_owned_child_contributes_its_next_wake() {
    let mut root = page();
    let id = owned(&mut root);
    work(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    assert_eq!(root.next_host_task_wake(0).unwrap(), Some(1));
}

#[cfg_attr(test, test)]
fn host_realm_tasks_owned_child_timer_and_promise_complete_in_document_turn() {
    let mut root = page();
    let id = owned(&mut root);
    work(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(3.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_shadow_below_owned_child_receives_its_turn() {
    let mut root = page();
    let id = owned(&mut root);
    let record = shadow(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    work(&mut record.vm.borrow_mut());
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(&mut record.vm.borrow_mut(), "seen;"),
        Value::Number(3.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_owned_child_below_shadow_receives_its_turn() {
    let mut root = page();
    let record = shadow(&mut root);
    let id = owned(&mut record.vm.borrow_mut());
    work(
        &mut record
            .vm
            .borrow_mut()
            .test262_realms
            .get_mut(&id)
            .unwrap()
            .vm
            .borrow_mut(),
    );
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(
            &mut record
                .vm
                .borrow_mut()
                .test262_realms
                .get_mut(&id)
                .unwrap()
                .vm
                .borrow_mut(),
            "seen;"
        ),
        Value::Number(3.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_owned_microtasks_drain_without_a_task_delivery() {
    let mut root = page();
    let id = owned(&mut root);
    run(
        &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
        "var seen=0;Promise.resolve().then(()=>seen=7);0;",
    );
    assert_eq!(root.run_host_task_turn(0, 0).unwrap().tasks_run, 0);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(7.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_owned_timer_can_return_to_its_parent() {
    let mut root = page();
    let id = owned(&mut root);
    run(&mut root,"var events=[];realm.global.parentCallback=()=>{events.push('parent');Promise.resolve().then(()=>events.push('job'));};0;");
    {
        let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
        browser_timers(child);
        run(child, "setTimeout(()=>parentCallback(),1);0;");
    }
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("parent,job".into())
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_mixed_path_keeps_each_callback_ancestor_active() {
    let mut root = page();
    let id = owned(&mut root);
    run(&mut root,"var events=[];realm.global.parentCallback=()=>{events.push('parent');Promise.resolve().then(()=>events.push('job'));};0;");
    // Enter through the real membrane while exporting the parent's callable.
    run(
        &mut root,
        r#"realm.evalScript("var nested=new ShadowRealm();var save=nested.evaluate('callback=>{globalThis.parentCallback=callback;return 0;}');save(parentCallback);0;");0;"#,
    );
    let record = root
        .test262_realms
        .get(&id)
        .unwrap()
        .vm
        .borrow_mut()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    {
        let mut leaf = record.vm.borrow_mut();
        browser_timers(&mut leaf);
        run(&mut leaf, "setTimeout(()=>parentCallback(),1);0;");
    }
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("parent,job".into())
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_checkpoint_revisits_ancestor_jobs_until_quiescent() {
    let mut root = page();
    let id = owned(&mut root);
    run(&mut root,"var events=[];realm.global.parentCallback=()=>{events.push('child');Promise.resolve().then(()=>events.push('parent-job'));};0;");
    run(
        &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
        "globalThis.trigger=()=>{Promise.resolve().then(()=>parentCallback());return 0;};0;",
    );
    run(
        &mut root,
        "Promise.resolve().then(()=>realm.global.trigger());0;",
    );
    assert_eq!(root.run_host_task_turn(0, 0).unwrap().tasks_run, 0);
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("child,parent-job".into())
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_shared_aliases_and_cycles_have_one_snapshot_entry() {
    let mut root = page();
    let first = shadow(&mut root);
    let second = shadow(&mut root);
    root.adopt_shadow_realm(root.object_prototype, first.clone())
        .unwrap();
    {
        let mut vm = first.vm.borrow_mut();
        let key = vm.object_prototype;
        vm.adopt_shadow_realm(key, second.clone()).unwrap();
    }
    {
        let mut vm = second.vm.borrow_mut();
        let key = vm.object_prototype;
        vm.adopt_shadow_realm(key, first.clone()).unwrap();
    }
    let snapshot = root.host_task_realms().unwrap();
    assert_eq!(
        snapshot
            .iter()
            .map(|member| member.heap)
            .collect::<Vec<_>>(),
        [first.heap, second.heap]
    );
    first.vm.borrow_mut().shadow_realms.clear();
    second.vm.borrow_mut().shadow_realms.clear();
}

#[cfg_attr(test, test)]
fn host_realm_tasks_busy_shared_route_is_skipped_then_becomes_runnable() {
    let mut root = page();
    let record = shadow(&mut root);
    work(&mut record.vm.borrow_mut());
    let borrowed = record.vm.borrow();
    assert_eq!(root.next_host_task_wake(0).unwrap(), None);
    drop(borrowed);
    assert_eq!(root.next_host_task_wake(0).unwrap(), Some(1));
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
}

#[cfg_attr(test, test)]
fn host_realm_tasks_foreign_owned_member_stays_outside_document_delivery() {
    let mut root = page();
    let mut foreign = page();
    let id = owned(&mut foreign);
    work(&mut foreign.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    let child = foreign.test262_realms.remove(&id).unwrap();
    let key = root.object_prototype;
    let prepared = root
        .test262_realms
        .prepare_insert(root.host_scheduling.as_ref())
        .unwrap();
    root.test262_realms.commit(prepared);
    root.test262_realms.insert(key, child);
    assert_eq!(root.next_host_task_wake(0).unwrap(), None);
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 0);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&key).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(0.0)
    );
}

fn record(vm: &mut Vm, log: &Rc<RefCell<Vec<HostValue>>>) {
    let log = log.clone();
    vm.install_host_function("record", 1, move |args: &[HostValue]| {
        log.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn host_realm_tasks_timer_source_order_spans_root_owned_and_shared_members() {
    let mut root = page();
    let id = owned(&mut root);
    let shadow = shadow(&mut root);
    let log = Rc::new(RefCell::new(Vec::new()));
    browser_timers(&mut root);
    record(&mut root, &log);
    {
        {
            let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
            browser_timers(child);
            record(child, &log);
            run(child, "setTimeout(()=>record('owned'),0);0;");
        }
        run(&mut root, "setTimeout(()=>record('root'),0);0;");
        {
            let mut child = shadow.vm.borrow_mut();
            browser_timers(&mut child);
            record(&mut child, &log);
            run(&mut child, "setTimeout(()=>record('shared'),0);0;");
        }
    }
    assert_eq!(root.run_host_task_turn(0, 3).unwrap().tasks_run, 3);
    assert_eq!(
        *log.borrow(),
        [
            HostValue::String("owned".into()),
            HostValue::String("root".into()),
            HostValue::String("shared".into())
        ]
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_interval_and_new_timer_keep_initial_turn_eligibility() {
    let mut root = page();
    let id = owned(&mut root);
    {
        let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
        browser_timers(child);
        run(
            child,
            "var seen=0;var interval=setInterval(()=>{seen++;setTimeout(()=>seen+=10,0);},1);0;",
        );
    }
    assert_eq!(root.run_host_task_turn(1, 3).unwrap().tasks_run, 1);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(1.0)
    );
    assert_eq!(root.run_host_task_turn(1, 3).unwrap().tasks_run, 1);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "clearInterval(interval);seen;"
        ),
        Value::Number(11.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_browser_only_turn_defers_owned_callback_timers() {
    let mut root = page();
    let id = owned(&mut root);
    {
        let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
        browser_timers(child);
        child.install_web_file_api().unwrap();
        run(child,"var seen=0;var signal=AbortSignal.timeout(1);signal.onabort=()=>seen++;setTimeout(()=>seen+=10,1);0;");
    }
    assert_eq!(
        root.run_host_task_turn_inner(1, 3, false)
            .unwrap()
            .tasks_run,
        1
    );
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(1.0)
    );
    assert_eq!(root.run_host_task_turn(1, 3).unwrap().tasks_run, 1);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(11.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_owned_finalization_cleanup_is_document_work() {
    let mut root = page();
    let id = owned(&mut root);
    {
        let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
        run(child,"var seen=0;var registry=new FinalizationRegistry(()=>seen++);(()=>{var object={};registry.register(object,0);})();0;");
        child.collect_realm_garbage().unwrap();
    }
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(
            &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut(),
            "seen;"
        ),
        Value::Number(1.0)
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_conformance_timers_remain_independent() {
    let mut root = page();
    let id = owned(&mut root);
    let timer;
    {
        let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
        let global = child.global("globalThis").unwrap().object_id().unwrap();
        timer = child
            .heap
            .get_own(global, "setTimeout")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        assert_eq!(
            child.heap.native_function(timer).unwrap(),
            Some(NativeFunction::Test262("setTimeout"))
        );
    }
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 0);
    let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
    assert_eq!(
        child.heap.native_function(timer).unwrap(),
        Some(NativeFunction::Test262("setTimeout"))
    );
    assert!(child.host_timers.is_empty());
}

#[cfg_attr(test, test)]
fn host_realm_tasks_snapshot_releases_owned_vm_and_all_path_credit() {
    let mut root = page();
    let id = owned(&mut root);
    let child_bytes = root.test262_realms[&id]
        .vm
        .borrow_mut()
        .heap
        .stats()
        .managed_bytes;
    let managed = root.heap.document_heap_budget().unwrap().0;
    let snapshot = root.host_task_realms().unwrap();
    let bytes = snapshot.retained_bytes()
        + snapshot
            .iter()
            .map(|realm| realm.retained_bytes())
            .sum::<usize>();
    root.test262_realms.clear();
    assert_eq!(
        root.heap.document_heap_budget().unwrap().0,
        managed - child_bytes
    );
    assert!(snapshot[0].enter(&mut root, |_| Ok(())).unwrap().is_none());
    let native = root.host_scheduling_snapshot().payload_bytes;
    drop(snapshot);
    assert_eq!(
        root.host_scheduling_snapshot().payload_bytes,
        native - bytes
    );
}

#[cfg_attr(test, test)]
fn host_realm_tasks_terminal_path_entry_preserves_queued_work() {
    let mut root = page();
    let id = owned(&mut root);
    work(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    let snapshot = root.host_task_realms().unwrap();
    let fuel = root.test262_realms[&id]
        .vm
        .borrow_mut()
        .remaining_instructions;
    root.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        snapshot[0].enter(&mut root, |_| Ok(())),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(
        root.test262_realms[&id].vm.borrow_mut().host_timers.len(),
        1
    );
    assert_eq!(
        root.test262_realms[&id]
            .vm
            .borrow_mut()
            .remaining_instructions,
        fuel
    );
    drop(snapshot);
    let mut healthy = page();
    assert_eq!(healthy.run_host_task_turn(0, 1).unwrap().tasks_run, 0);
}

#[cfg_attr(test, test)]
fn host_realm_tasks_wake_lookup_preserves_fuel_and_releases_registrations() {
    let mut root = page();
    let id = owned(&mut root);
    work(&mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut());
    root.remaining_instructions = 7;
    root.test262_realms
        .get_mut(&id)
        .unwrap()
        .vm
        .borrow_mut()
        .remaining_instructions = 11;
    let before = root.host_scheduling_usage().unwrap();
    assert_eq!(root.next_host_task_wake(0).unwrap(), Some(1));
    assert_eq!(root.remaining_instructions, 7);
    assert_eq!(
        root.test262_realms[&id]
            .vm
            .borrow_mut()
            .remaining_instructions,
        11
    );
    assert!(realm_reentrancy::resolve_active(root.object_prototype.heap).is_none());
    assert!(realm_reentrancy::resolve_active(
        root.test262_realms[&id]
            .vm
            .borrow_mut()
            .object_prototype
            .heap
    )
    .is_none());
    assert_eq!(root.host_scheduling_usage().unwrap(), before);
}

#[cfg_attr(test, test)]
fn host_realm_tasks_nested_owned_path_delivers_and_releases_each_registration() {
    let mut root = page();
    let id = owned(&mut root);
    let leaf_id;
    {
        let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
        run(child, "var nestedRealm=$262.createRealm();0;");
        leaf_id = *child.test262_realms.keys().next().unwrap();
        work(
            &mut child
                .test262_realms
                .get_mut(&leaf_id)
                .unwrap()
                .vm
                .borrow_mut(),
        );
    }
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    let child = &mut root.test262_realms.get_mut(&id).unwrap().vm.borrow_mut();
    assert_eq!(
        run(
            &mut child
                .test262_realms
                .get_mut(&leaf_id)
                .unwrap()
                .vm
                .borrow_mut(),
            "seen;"
        ),
        Value::Number(3.0)
    );
    assert!(realm_reentrancy::resolve_active(root.object_prototype.heap).is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_realm_task_contracts() {
        host_realm_tasks_owned_child_contributes_its_next_wake();
        host_realm_tasks_owned_child_timer_and_promise_complete_in_document_turn();
        host_realm_tasks_shadow_below_owned_child_receives_its_turn();
        host_realm_tasks_owned_child_below_shadow_receives_its_turn();
        host_realm_tasks_owned_microtasks_drain_without_a_task_delivery();
        host_realm_tasks_owned_timer_can_return_to_its_parent();
        host_realm_tasks_mixed_path_keeps_each_callback_ancestor_active();
        host_realm_tasks_checkpoint_revisits_ancestor_jobs_until_quiescent();
        host_realm_tasks_shared_aliases_and_cycles_have_one_snapshot_entry();
        host_realm_tasks_busy_shared_route_is_skipped_then_becomes_runnable();
        host_realm_tasks_foreign_owned_member_stays_outside_document_delivery();
        host_realm_tasks_timer_source_order_spans_root_owned_and_shared_members();
        host_realm_tasks_interval_and_new_timer_keep_initial_turn_eligibility();
        host_realm_tasks_browser_only_turn_defers_owned_callback_timers();
        host_realm_tasks_owned_finalization_cleanup_is_document_work();
        host_realm_tasks_conformance_timers_remain_independent();
        host_realm_tasks_snapshot_releases_owned_vm_and_all_path_credit();
        host_realm_tasks_terminal_path_entry_preserves_queued_work();
        host_realm_tasks_wake_lookup_preserves_fuel_and_releases_registrations();
        host_realm_tasks_nested_owned_path_delivers_and_releases_each_registration();
    }
}
