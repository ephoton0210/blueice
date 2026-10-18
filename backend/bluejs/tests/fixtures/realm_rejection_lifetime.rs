// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed small document notification lifetimes, without services or pressure.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_test262_harness().unwrap();
    vm
}
fn member(root: &mut Vm, owned: bool) -> realm_cells::RealmHandle {
    if owned {
        run(root, "var member=$262.createRealm();0;");
        root.test262_realms.values().next().unwrap().vm.clone()
    } else {
        run(root, "var member=new ShadowRealm();0;");
        root.shadow_realms.values().next().unwrap().vm.clone()
    }
}
fn install(vm: &mut Vm, log: &Rc<RefCell<Vec<HostValue>>>) {
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_rejection_tracking().unwrap();
    let values = log.clone();
    vm.install_host_function("record", 1, move |args: &[HostValue]| {
        values.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        vm,
        "onunhandledrejection=event=>{record(event.reason);event.preventDefault();};0;",
    );
}
fn collect(vm: &mut Vm) {
    vm.collect_realm_garbage().unwrap();
}
fn orphan(root: &mut Vm, handle: realm_cells::RealmHandle) -> realm_cells::RealmWeak {
    let weak = handle.downgrade();
    drop(handle);
    run(root, "member=null;0;");
    weak
}
fn pending_member(owned: bool) {
    let mut root = page();
    let handle = member(&mut root, owned);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "var pending=Promise.reject('orphan');0;");
    }
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    assert!(log.borrow().is_empty());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("orphan".into())]);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_owned_pending_notification_survives_discard() {
    pending_member(true);
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_shared_pending_notification_survives_discard() {
    pending_member(false);
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_nested_pending_notification_retains_the_access_route() {
    let mut root = page();
    let parent = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    let inner;
    {
        let mut vm = parent.borrow_mut();
        run(&mut vm, "var inner=new ShadowRealm();0;");
        let record = vm.shadow_realms.values().next().unwrap();
        inner = record.vm.downgrade();
        let mut child = record.vm.borrow_mut();
        install(&mut child, &log);
        run(&mut child, "Promise.reject('nested');0;");
    }
    let outer = orphan(&mut root, parent);
    collect(&mut root);
    assert!(outer.upgrade().is_some() && inner.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("nested".into())]);
    collect(&mut root);
    assert!(outer.upgrade().is_none() && inner.upgrade().is_none());
}

fn empty_profile(owned: bool) {
    let mut root = page();
    let handle = member(&mut root, owned);
    let log = Rc::new(RefCell::new(Vec::new()));
    install(&mut handle.borrow_mut(), &log);
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
    assert!(log.borrow().is_empty());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_empty_owned_profile_does_not_retain_its_vm() {
    empty_profile(true);
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_empty_shared_profile_does_not_retain_its_vm() {
    empty_profile(false);
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_untracked_rejection_does_not_retain_its_vm() {
    let mut root = page();
    let handle = member(&mut root, true);
    run(
        &mut handle.borrow_mut(),
        "var pending=Promise.reject('unobserved');0;",
    );
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_early_handler_preserves_only_its_reaction_job() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(
            &mut vm,
            "var pending=Promise.reject('ignored');pending.catch(()=>record('reaction'));0;",
        );
        assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    }
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 0);
    assert_eq!(*log.borrow(), [HostValue::String("reaction".into())]);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_published_notification_survives_discard() {
    let mut root = page();
    let handle = member(&mut root, false);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "Promise.reject('queued');0;");
    }
    root.run_host_microtask_checkpoint().unwrap();
    assert!(handle
        .borrow()
        .host_rejections
        .as_ref()
        .unwrap()
        .pending
        .is_empty());
    assert_eq!(handle.borrow().browser_tasks.len(), 1);
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("queued".into())]);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_handler_can_publish_one_followup_notification() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "onunhandledrejection=event=>{record(event.reason);event.preventDefault();if(event.reason==='first')Promise.reject('second');};Promise.reject('first');0;");
    }
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("first".into())]);
    assert!(weak.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(
        *log.borrow(),
        [
            HostValue::String("first".into()),
            HostValue::String("second".into())
        ]
    );
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_another_document_cannot_consume_pending_delivery() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "Promise.reject('first-document');0;");
    }
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    let mut other = page();
    assert_eq!(other.run_host_task_turn(0, 1).unwrap().tasks_run, 0);
    assert_eq!(run(&mut other, "21+21;"), Value::Number(42.0));
    assert!(log.borrow().is_empty());
    assert!(weak.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("first-document".into())]);
    assert!(other.check_scheduling_failure().is_ok());
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_document_drop_releases_pending_owners_and_credit() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "Promise.reject('discarded');0;");
    }
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    let observer = Vm {
        host_scheduling: root.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Default::default()
    };
    drop(root);
    assert!(weak.upgrade().is_none());
    assert!(log.borrow().is_empty());
    drop(weak);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_reported_bit_is_not_a_vm_root() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "var pending=Promise.reject('reported');0;");
    }
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    {
        let mut vm = handle.borrow_mut();
        let pending = vm
            .lookup_global_name("pending")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        assert!(vm.promises[&pending].rejection_notified);
        assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    }
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_late_handler_task_keeps_the_vm_until_delivery() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "onunhandledrejection=event=>{record('unhandled');event.preventDefault();};onrejectionhandled=()=>record('handled');var pending=Promise.reject('reason');0;");
    }
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    run(
        &mut handle.borrow_mut(),
        "pending.catch(()=>record('reaction'));0;",
    );
    let weak = orphan(&mut root, handle);
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(
        *log.borrow(),
        [
            HostValue::String("unhandled".into()),
            HostValue::String("reaction".into()),
            HostValue::String("handled".into())
        ]
    );
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_rejection_lifetime_busy_member_requests_retry_without_consuming_delivery() {
    let mut root = page();
    let handle = member(&mut root, true);
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let mut vm = handle.borrow_mut();
        install(&mut vm, &log);
        run(&mut vm, "Promise.reject('retry');0;");
    }
    let weak = orphan(&mut root, handle);
    let handle = weak.upgrade().unwrap();
    {
        let _busy = handle.borrow_mut();
        collect(&mut root);
        let turn = root.run_host_task_turn(0, 1).unwrap();
        assert_eq!(turn.tasks_run, 0);
        assert_eq!(turn.next_wake_in_ms, Some(0));
        assert!(log.borrow().is_empty());
    }
    drop(handle);
    assert_eq!(root.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("retry".into())]);
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_rejection_lifetime_contracts() {
        realm_rejection_lifetime_owned_pending_notification_survives_discard();
        realm_rejection_lifetime_shared_pending_notification_survives_discard();
        realm_rejection_lifetime_nested_pending_notification_retains_the_access_route();
        realm_rejection_lifetime_empty_owned_profile_does_not_retain_its_vm();
        realm_rejection_lifetime_empty_shared_profile_does_not_retain_its_vm();
        realm_rejection_lifetime_untracked_rejection_does_not_retain_its_vm();
        realm_rejection_lifetime_early_handler_preserves_only_its_reaction_job();
        realm_rejection_lifetime_published_notification_survives_discard();
        realm_rejection_lifetime_handler_can_publish_one_followup_notification();
        realm_rejection_lifetime_another_document_cannot_consume_pending_delivery();
        realm_rejection_lifetime_document_drop_releases_pending_owners_and_credit();
        realm_rejection_lifetime_reported_bit_is_not_a_vm_root();
        realm_rejection_lifetime_late_handler_task_keeps_the_vm_until_delivery();
        realm_rejection_lifetime_busy_member_requests_retry_without_consuming_delivery();
    }
}
