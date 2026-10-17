// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny aliases must not change the home realm of a queued callable.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "var realm=$262.createRealm();0;");
    vm
}
fn owned_home() -> (Vm, ShadowRealmRecord) {
    let mut root = page();
    run(&mut root,"var events=[];realm.global.parentCallback=()=>{events.push('parent');Promise.resolve().then(()=>events.push('job'));};0;");
    run(
        &mut root,
        r#"realm.evalScript("var nested=new ShadowRealm();var save=nested.evaluate('callback=>{globalThis.parentCallback=callback;return 0;}');save(parentCallback);0;");0;"#,
    );
    let leaf = root
        .test262_realms
        .values()
        .next()
        .unwrap()
        .vm
        .borrow_mut()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    // Discovery sees this direct shared alias before the owned creator route.
    root.adopt_shadow_realm(root.object_prototype, leaf.clone());
    (root, leaf)
}

#[cfg_attr(test, test)]
fn realm_alias_home_owned_callback_survives_direct_timer_alias() {
    let (mut root, leaf) = owned_home();
    {
        let mut vm = leaf.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "setTimeout(()=>parentCallback(),1);0;");
    }
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("parent,job".into())
    );
}

#[cfg_attr(test, test)]
fn realm_alias_home_owned_callback_survives_direct_microtask_alias() {
    let (mut root, leaf) = owned_home();
    {
        let mut vm = leaf.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "queueMicrotask(()=>parentCallback());0;");
    }
    assert_eq!(root.run_host_microtask_checkpoint().unwrap(), 0);
    assert_eq!(
        run(&mut root, "events.join(',');"),
        Value::String("parent,job".into())
    );
}

#[cfg_attr(test, test)]
fn realm_alias_home_shared_callback_survives_shorter_owner_route() {
    let mut root = page();
    run(
        &mut root,
        r#"realm.evalScript("var outer=new ShadowRealm();outer.evaluate(\"globalThis.seen=0;var nested=new ShadowRealm();var save=nested.evaluate('callback=>{globalThis.callback=callback;return 0;}');save(()=>{seen++;return 0;});0;\");0;");0;"#,
    );
    let home = root
        .test262_realms
        .values()
        .next()
        .unwrap()
        .vm
        .borrow_mut()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    let leaf = home
        .vm
        .borrow()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    root.adopt_shadow_realm(root.object_prototype, leaf.clone());
    {
        let mut vm = leaf.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "setTimeout(()=>callback(),1);0;");
    }
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut home.vm.borrow_mut(), "seen;"), Value::Number(1.0));
}

fn view(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Default::default()
    }
}

#[cfg_attr(test, test)]
fn realm_alias_home_weak_cell_releases_vm_before_its_allocation_credit() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let observer = view(&vm);
    let before = observer.host_scheduling_snapshot().payload_bytes;
    let handle = realm_cells::RealmHandle::new(vm).unwrap();
    let bytes = observer.host_scheduling_snapshot().payload_bytes - before;
    assert!(bytes >= std::mem::size_of::<RefCell<Vm>>());
    let weak = handle.downgrade();
    let other = weak.clone();
    let strong = handle.clone();
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    drop(handle);
    assert!(weak.upgrade().is_some());
    drop(strong);
    assert!(weak.upgrade().is_none());
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(weak);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(other);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn realm_alias_home_neutral_cell_attaches_once_on_document_activation() {
    let handle = realm_cells::RealmHandle::new(Vm::default()).unwrap();
    assert!(handle.borrow().host_scheduling.is_none());
    let weak = handle.downgrade();
    handle
        .borrow_mut()
        .use_bounded_host_microtask_checkpoints()
        .unwrap();
    let observer = view(&handle.borrow());
    let before = observer.host_scheduling_snapshot().payload_bytes;
    assert!(before >= std::mem::size_of::<RefCell<Vm>>());
    handle
        .borrow_mut()
        .use_bounded_host_microtask_checkpoints()
        .unwrap();
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, before);
    drop(handle);
    assert!(weak.upgrade().is_none());
    assert!(observer.host_scheduling_snapshot().payload_bytes > 0);
    drop(weak);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn realm_alias_home_foreign_reattachment_preserves_the_original_document() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let handle = realm_cells::RealmHandle::new(vm).unwrap();
    let weak = handle.downgrade();
    let original = view(&handle.borrow());
    let before = original.host_scheduling_snapshot().payload_bytes;
    let mut foreign = Vm::default();
    foreign.use_bounded_host_microtask_checkpoints().unwrap();
    assert!(matches!(
        weak.attach(foreign.host_scheduling.as_ref().unwrap()),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(foreign.check_scheduling_failure().is_err());
    assert!(handle.borrow().check_scheduling_failure().is_ok());
    assert_eq!(original.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(run(&mut handle.borrow_mut(), "21+21;"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn realm_alias_home_terminal_account_refuses_cell_creation() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let observer = view(&vm);
    vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        realm_cells::RealmHandle::new(vm),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn realm_alias_home_busy_inactive_cell_never_enters_the_callable() {
    let handle = realm_cells::RealmHandle::new(Vm::default()).unwrap();
    let caller = Vm::default().object_prototype.heap;
    let _held = handle.borrow_mut();
    let called = std::cell::Cell::new(false);
    assert!(matches!(
        handle.edit(caller, |_| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        handle.read(caller, |_| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(!called.get());
}

#[cfg_attr(test, test)]
fn realm_alias_home_self_identity_is_rejected_before_borrowing() {
    let handle = realm_cells::RealmHandle::new(Vm::default()).unwrap();
    let called = std::cell::Cell::new(false);
    assert!(matches!(
        handle.edit(handle.heap(), |_| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        handle.read(handle.heap(), |_| {
            called.set(true);
            Ok(())
        }),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(!called.get());
}

#[cfg_attr(test, test)]
fn realm_alias_home_registered_cell_reenters_and_releases_its_registration() {
    let handle = realm_cells::RealmHandle::new(Vm::default()).unwrap();
    let caller = Vm::default().object_prototype.heap;
    handle
        .edit(caller, |vm| {
            let registration = realm_reentrancy::prepare_active_registration(vm, None)?;
            let guard = registration.publish(vm);
            assert_eq!(
                handle.edit(caller, |inner| Ok(inner.object_prototype.heap))?,
                handle.heap()
            );
            assert_eq!(
                handle.read(caller, |inner| Ok(inner.object_prototype.heap))?,
                handle.heap()
            );
            drop(guard);
            Ok(())
        })
        .unwrap();
    assert!(realm_reentrancy::resolve_active(handle.heap()).is_none());
    assert_eq!(run(&mut handle.borrow_mut(), "21+21;"), Value::Number(42.0));
}

fn local_owned_home() -> (Vm, realm_cells::RealmHandle, ShadowRealmRecord) {
    let mut root = page();
    run(
        &mut root,
        r#"realm.evalScript("var seen=0;var nested=new ShadowRealm();var save=nested.evaluate('callback=>{globalThis.callback=callback;return 0;}');save(()=>{seen++;Promise.resolve().then(()=>seen+=10);return 0;});0;");0;"#,
    );
    let home = root.test262_realms.values().next().unwrap().vm.clone();
    let leaf = home.borrow().shadow_realms.values().next().unwrap().clone();
    root.adopt_shadow_realm(root.object_prototype, leaf.clone());
    (root, home, leaf)
}

#[cfg_attr(test, test)]
fn realm_alias_home_owned_local_callable_survives_gc_before_timer_delivery() {
    let (mut root, home, leaf) = local_owned_home();
    {
        let mut vm = leaf.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "setTimeout(()=>callback(),1);0;");
    }
    root.collect_realm_garbage().unwrap();
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(run(&mut home.borrow_mut(), "seen;"), Value::Number(11.0));
}

#[cfg_attr(test, test)]
fn realm_alias_home_owned_local_callable_survives_gc_before_microtask_delivery() {
    let (mut root, home, leaf) = local_owned_home();
    {
        let mut vm = leaf.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "queueMicrotask(()=>callback());0;");
    }
    root.collect_realm_garbage().unwrap();
    assert_eq!(root.run_host_microtask_checkpoint().unwrap(), 0);
    assert_eq!(run(&mut home.borrow_mut(), "seen;"), Value::Number(11.0));
}

#[cfg_attr(test, test)]
fn realm_alias_home_live_wrapper_retains_shared_home_then_releases_its_cycle() {
    let mut root = page();
    run(
        &mut root,
        r#"realm.evalScript("var outer=new ShadowRealm();outer.evaluate(\"globalThis.seen=0;var nested=new ShadowRealm();var save=nested.evaluate('callback=>{globalThis.callback=callback;return 0;}');save(()=>{seen++;return 0;});0;\");0;");0;"#,
    );
    let owner = root.test262_realms.values().next().unwrap().vm.clone();
    let home = owner
        .borrow()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    let leaf = home
        .vm
        .borrow()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .clone();
    let weak = home.vm.downgrade();
    root.adopt_shadow_realm(root.object_prototype, leaf.clone());
    run(&mut owner.borrow_mut(), "outer=null;0;");
    drop(home);
    root.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_some());
    {
        let mut vm = leaf.vm.borrow_mut();
        vm.install_host_timer_functions().unwrap();
        run(&mut vm, "setTimeout(()=>callback(),1);0;");
    }
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(
        run(&mut weak.upgrade().unwrap().borrow_mut(), "seen;"),
        Value::Number(1.0)
    );
    run(&mut leaf.vm.borrow_mut(), "callback=undefined;0;");
    root.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(run(&mut root, "21+21;"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn realm_alias_home_callback_throw_stays_opaque_and_leaves_other_documents_healthy() {
    let (mut root, home, leaf) = local_owned_home();
    run(
        &mut home.borrow_mut(),
        "save(()=>{throw new Error('private');});0;",
    );
    run(&mut leaf.vm.borrow_mut(), "globalThis.caught=false;try{callback();}catch(error){caught=error instanceof TypeError&&!String(error).includes('private');}0;");
    assert_eq!(run(&mut leaf.vm.borrow_mut(), "caught;"), Value::Bool(true));
    assert!(realm_reentrancy::resolve_active(home.heap()).is_none());
    let mut independent = page();
    assert_eq!(run(&mut independent, "21+21;"), Value::Number(42.0));
    assert!(root.run_host_microtask_checkpoint().is_ok());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_alias_home_contracts() {
        realm_alias_home_owned_callback_survives_direct_timer_alias();
        realm_alias_home_owned_callback_survives_direct_microtask_alias();
        realm_alias_home_shared_callback_survives_shorter_owner_route();
        realm_alias_home_weak_cell_releases_vm_before_its_allocation_credit();
        realm_alias_home_neutral_cell_attaches_once_on_document_activation();
        realm_alias_home_foreign_reattachment_preserves_the_original_document();
        realm_alias_home_terminal_account_refuses_cell_creation();
        realm_alias_home_busy_inactive_cell_never_enters_the_callable();
        realm_alias_home_self_identity_is_rejected_before_borrowing();
        realm_alias_home_registered_cell_reenters_and_releases_its_registration();
        realm_alias_home_owned_local_callable_survives_gc_before_timer_delivery();
        realm_alias_home_owned_local_callable_survives_gc_before_microtask_delivery();
        realm_alias_home_live_wrapper_retains_shared_home_then_releases_its_cycle();
        realm_alias_home_callback_throw_stays_opaque_and_leaves_other_documents_healthy();
    }
}
