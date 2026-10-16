// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Document timer ownership, using small live queues and numeric admission.

use super::*;

fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_host_timer_functions().unwrap();
    vm
}

fn child_with_timers(root: &mut Vm, source: &str) {
    execute(root, "var realm = new ShadowRealm(); 0;");
    let record = root.shadow_realms.values().next().unwrap().clone();
    let mut child = record.vm.borrow_mut();
    child.install_host_timer_functions().unwrap();
    execute(&mut child, source);
}

#[cfg_attr(test, test)]
fn document_timer_counts_share_live_realms_and_refund_delivery_and_cancellation() {
    let mut root = page_vm();
    execute(&mut root, "var root_id = setTimeout(() => {}, 100); 0;");
    child_with_timers(
        &mut root,
        "var once = setTimeout(() => {}, 10); var interval = setInterval(() => {}, 10); 0;",
    );
    let mut other = page_vm();
    execute(&mut other, "setTimeout(() => {}, 100); 0;");
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 3);
    assert_eq!(other.host_scheduling_usage().unwrap().pending_timers, 1);
    let record = root.shadow_realms.values().next().unwrap().clone();
    {
        let mut child = record.vm.borrow_mut();
        assert_eq!(child.host_scheduling_usage().unwrap().pending_timers, 3);
        assert_eq!(child.run_due_host_timers(10).unwrap().tasks_run, 2);
        assert_eq!(child.host_scheduling_usage().unwrap().pending_timers, 2);
        execute(&mut child, "clearInterval(interval); 0;");
    }
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 1);
    execute(&mut root, "clearTimeout(root_id); 0;");
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 0);
    assert_eq!(other.host_scheduling_usage().unwrap().pending_timers, 1);
}

#[cfg_attr(test, test)]
fn document_timer_counts_publish_registration_and_self_cancellation_before_jobs() {
    for repeat in [false, true] {
        let mut vm = page_vm();
        let account = vm.host_scheduling.as_ref().unwrap().account.clone();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let output = observed.clone();
        vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
            output
                .borrow_mut()
                .push(account.borrow().usage.pending_timers);
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let timer = if repeat { "setInterval" } else { "setTimeout" };
        execute(&mut vm, &format!("var id = {timer}(function() {{ observe(); clearInterval(id); observe(); Promise.resolve().then(() => observe()); }}, 1); 0;"));
        assert_eq!(vm.host_scheduling_usage().unwrap().pending_timers, 1);
        assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
        assert_eq!(*observed.borrow(), vec![usize::from(repeat), 0, 0]);
        assert_eq!(vm.host_scheduling_usage().unwrap().pending_timers, 0);
    }
}

#[cfg_attr(test, test)]
fn document_timer_counts_keep_discarded_realms_alive_until_queued_work_finishes() {
    let mut root = page_vm();
    child_with_timers(
        &mut root,
        "setTimeout(() => {}, 10); setTimeout(() => {}, 20); 0;",
    );
    execute(&mut root, "realm = undefined; 0;");
    root.collect_realm_garbage().unwrap();
    assert_eq!(root.shadow_realms.len(), 1);
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 2);
    assert_eq!(root.run_host_task_turn(20, 2).unwrap().tasks_run, 2);
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 0);
    root.collect_realm_garbage().unwrap();
    assert!(root.shadow_realms.is_empty());
}

#[cfg_attr(test, test)]
fn document_timer_counts_refund_the_whole_document_tree_on_drop() {
    let mut root = page_vm();
    execute(&mut root, "setTimeout(() => {}, 100); 0;");
    child_with_timers(
        &mut root,
        "setTimeout(() => {}, 100); setInterval(() => {}, 100); 0;",
    );
    let account = root.host_scheduling.as_ref().unwrap().account.clone();
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 3);
    drop(root);
    assert_eq!(account.borrow().usage.pending_timers, 0);
}

#[cfg_attr(test, test)]
fn document_timer_counts_apply_numeric_admission_without_allocating_timer_queues() {
    let mut root = SchedulingScope::new();
    let mut child = root.child();
    let account = root.account.clone();
    let root_usage = HostSchedulingUsage {
        pending_timers: MAX_PENDING_HOST_TIMERS - 1,
        ..HostSchedulingUsage::default()
    };
    let child_usage = HostSchedulingUsage {
        pending_timers: 1,
        ..HostSchedulingUsage::default()
    };
    root.check(root_usage, HostSchedulingUsage::default())
        .unwrap();
    child
        .check(child_usage, HostSchedulingUsage::default())
        .unwrap();
    assert_eq!(
        account.borrow().usage.pending_timers,
        MAX_PENDING_HOST_TIMERS
    );
    assert_eq!(
        child.check(
            child_usage,
            HostSchedulingUsage {
                pending_timers: 1,
                ..HostSchedulingUsage::default()
            }
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        account.borrow().usage.pending_timers,
        MAX_PENDING_HOST_TIMERS
    );
    assert_eq!(
        child.check(
            HostSchedulingUsage::default(),
            HostSchedulingUsage::default()
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        account.borrow().usage.pending_timers,
        MAX_PENDING_HOST_TIMERS - 1
    );
    drop(root);
    assert_eq!(account.borrow().usage.pending_timers, 0);
    assert!(account.borrow().failed);
}

#[cfg_attr(test, test)]
fn document_timer_counts_observe_clock_enqueues_before_registering_the_outer_timer() {
    let mut root = Vm::default();
    root.use_bounded_host_microtask_checkpoints().unwrap();
    child_with_timers(&mut root, "0;");
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    root.install_host_function("record", 0, move |_args: &[HostValue]| {
        output.borrow_mut().push("root");
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let record = root.shadow_realms.values().next().unwrap().clone();
    let output = log.clone();
    record
        .vm
        .borrow_mut()
        .install_host_function("record", 0, move |_args: &[HostValue]| {
            output.borrow_mut().push("child");
            Ok(HostValue::Undefined)
        })
        .unwrap();
    let child_vm = record.vm.clone();
    let scheduled = std::cell::Cell::new(false);
    root.install_host_timer_functions_with_clock(move || {
        if !scheduled.replace(true) {
            execute(
                &mut child_vm.borrow_mut(),
                "setTimeout(() => record(), 0); 0;",
            );
        }
        0
    })
    .unwrap();
    execute(&mut root, "setTimeout(() => record(), 0); 0;");
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 2);
    assert_eq!(root.run_host_task_turn(0, 2).unwrap().tasks_run, 2);
    assert_eq!(*log.borrow(), vec!["child", "root"]);
    assert_eq!(root.host_scheduling_usage().unwrap().pending_timers, 0);
}

#[cfg_attr(test, test)]
fn document_timer_counts_preserve_host_neutral_range_error_recovery() {
    let mut vm = Vm::default();
    vm.install_host_timer_functions().unwrap();
    execute(
        &mut vm,
        &format!(
            "for (var i = 0; i < {}; i++) setTimeout(() => {{}}, 100); 0;",
            crate::MAX_HOST_TIMERS_PER_REALM
        ),
    );
    let code = crate::compile(&crate::parse("setTimeout(() => {}, 100);").unwrap()).unwrap();
    assert!(matches!(
        vm.execute_script(&code),
        Err(RuntimeError::RangeError(_))
    ));
    execute(&mut vm, "clearTimeout(1); setTimeout(() => {}, 100); 0;");
    assert_eq!(
        vm.host_scheduling_usage().unwrap().pending_timers,
        crate::MAX_HOST_TIMERS_PER_REALM
    );
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.execute_script(&code), Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.host_scheduling_snapshot().pending_timers,
        MAX_PENDING_HOST_TIMERS
    );
}

impl crate::Vm {
    /// Runs document timer ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_document_timer_count_contracts() {
        document_timer_counts_share_live_realms_and_refund_delivery_and_cancellation();
        document_timer_counts_publish_registration_and_self_cancellation_before_jobs();
        document_timer_counts_keep_discarded_realms_alive_until_queued_work_finishes();
        document_timer_counts_refund_the_whole_document_tree_on_drop();
        document_timer_counts_apply_numeric_admission_without_allocating_timer_queues();
        document_timer_counts_observe_clock_enqueues_before_registering_the_outer_timer();
        document_timer_counts_preserve_host_neutral_range_error_recovery();
    }
}
