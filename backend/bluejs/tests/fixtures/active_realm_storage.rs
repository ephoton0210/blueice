// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny cross-realm registrations own metadata without retaining thread caches.

use super::*;

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

fn enter(vm: &mut Vm) -> ActiveGuard {
    register_active(vm).unwrap()
}

#[cfg_attr(test, test)]
fn active_realm_storage_registration_retains_document_metadata_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let guard = enter(&mut vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= before + std::mem::size_of::<ActiveVm>(),
        "a live cross-realm registration must own its native metadata"
    );
    drop(guard);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn active_realm_storage_empty_registry_releases_backing() {
    let mut vm = page();
    let guard = enter(&mut vm);
    assert!(active_backing_bytes() > 0);
    drop(guard);
    assert_eq!(
        active_backing_bytes(),
        0,
        "an empty thread registry must not retain an unowned allocation"
    );
}

#[cfg_attr(test, test)]
fn active_realm_storage_nested_documents_keep_independent_credit() {
    let mut first = page();
    let mut second = page();
    let first_view = observer(&first);
    let second_view = observer(&second);
    let before_first = first_view.host_scheduling_snapshot().payload_bytes;
    let before_second = second_view.host_scheduling_snapshot().payload_bytes;
    let outer = enter(&mut first);
    let first_live = first_view.host_scheduling_snapshot().payload_bytes;
    assert!(first_live > before_first);
    let inner = enter(&mut second);
    assert!(second_view.host_scheduling_snapshot().payload_bytes > before_second);
    assert_eq!(
        first_view.host_scheduling_snapshot().payload_bytes,
        first_live
    );
    drop(inner);
    assert_eq!(
        second_view.host_scheduling_snapshot().payload_bytes,
        before_second
    );
    assert_eq!(
        first_view.host_scheduling_snapshot().payload_bytes,
        first_live
    );
    drop(outer);
    assert_eq!(
        first_view.host_scheduling_snapshot().payload_bytes,
        before_first
    );
}

fn script(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
}

#[cfg_attr(test, test)]
fn active_realm_storage_preparation_charges_before_publication() {
    let mut vm = page();
    let view = observer(&vm);
    let tag = vm.object_prototype.heap;
    let before = view.host_scheduling_snapshot().payload_bytes;
    let prepared = prepare_active_registration(&mut vm, None).unwrap();
    assert!(prepared.node.ptr.get().is_null());
    assert!(prepared.node.previous.get().is_none());
    assert!(resolve_active(tag).is_none());
    assert_eq!(active_backing_bytes(), 0);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + std::mem::size_of::<ActiveVm>()
    );
    let guard = prepared.publish(&mut vm);
    assert_eq!(resolve_active(tag), Some(&mut vm as *mut Vm));
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + std::mem::size_of::<ActiveVm>()
    );
    drop(guard);
    assert!(resolve_active(tag).is_none());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn active_realm_storage_unpublished_metadata_can_outlive_its_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let prepared = prepare_active_registration(&mut vm, None).unwrap();
    drop(vm);
    assert!(prepared.node.ptr.get().is_null());
    assert_eq!(active_backing_bytes(), 0);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        std::mem::size_of::<ActiveVm>()
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_nested_tags_restore_in_stack_order() {
    let mut first = page();
    let mut second = page();
    let first_tag = first.object_prototype.heap;
    let second_tag = second.object_prototype.heap;
    let outer = enter(&mut first);
    assert!(!has_foreign_caller(first_tag));
    let inner = enter(&mut second);
    assert!(has_foreign_caller(first_tag));
    assert!(has_foreign_caller(second_tag));
    assert_eq!(resolve_active(first_tag), Some(&mut first as *mut Vm));
    assert_eq!(resolve_active(second_tag), Some(&mut second as *mut Vm));
    drop(inner);
    assert!(resolve_active(second_tag).is_none());
    assert!(!has_foreign_caller(first_tag));
    assert_eq!(resolve_active(first_tag), Some(&mut first as *mut Vm));
    drop(outer);
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_duplicate_tags_keep_zero_owning_policy_views() {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let tag = vm.object_prototype.heap;
    let outer = enter(&mut vm);
    let inner = enter(&mut vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let captured = active_scheduling_scope(tag).unwrap();
    assert!(captured.same_document(&scope));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    ACTIVE.with(|active| assert!(Rc::ptr_eq(active.borrow().as_ref().unwrap(), &inner.node)));
    drop(captured);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(inner);
    ACTIVE.with(|active| assert!(Rc::ptr_eq(active.borrow().as_ref().unwrap(), &outer.node)));
    assert_eq!(resolve_active(tag), Some(&mut vm as *mut Vm));
    drop(outer);
    assert!(active_scheduling_scope(tag).is_none());
}

#[cfg_attr(test, test)]
fn active_realm_storage_peer_hint_preserves_neutral_policy() {
    let owner = page();
    let scope = owner.host_scheduling.as_ref().unwrap();
    let view = observer(&owner);
    let mut neutral = Vm::default();
    let tag = neutral.object_prototype.heap;
    let before = view.host_scheduling_snapshot().payload_bytes;
    let prepared = prepare_active_registration(&mut neutral, Some(scope)).unwrap();
    assert!(prepared.node._storage.as_ref().unwrap().belongs_to(scope));
    let guard = prepared.publish(&mut neutral);
    assert!(neutral.host_scheduling.is_none());
    assert!(active_scheduling_scope(tag).is_none());
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + std::mem::size_of::<ActiveVm>()
    );
    drop(guard);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn active_realm_storage_neutral_nesting_uses_nearest_metadata_owner() {
    let mut first = page();
    let mut second = page();
    let first_view = observer(&first);
    let second_view = observer(&second);
    let mut neutral = Vm::default();
    let outer = enter(&mut first);
    let middle = enter(&mut second);
    let first_before = first_view.host_scheduling_snapshot().payload_bytes;
    let second_before = second_view.host_scheduling_snapshot().payload_bytes;
    let inner = enter(&mut neutral);
    assert!(active_scheduling_scope(neutral.object_prototype.heap).is_none());
    assert_eq!(
        first_view.host_scheduling_snapshot().payload_bytes,
        first_before
    );
    assert_eq!(
        second_view.host_scheduling_snapshot().payload_bytes,
        second_before + std::mem::size_of::<ActiveVm>()
    );
    drop(inner);
    assert_eq!(
        second_view.host_scheduling_snapshot().payload_bytes,
        second_before
    );
    drop(middle);
    drop(outer);
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_standalone_neutral_calls_release_metadata() {
    let mut vm = Vm::default();
    let guard = enter(&mut vm);
    assert!(guard.node._storage.is_none());
    assert!(active_scheduling_scope(vm.object_prototype.heap).is_none());
    assert_eq!(active_backing_bytes(), std::mem::size_of::<ActiveVm>());
    drop(guard);
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_terminal_preparation_preserves_execution_state() {
    let mut vm = page();
    script(&mut vm, "let kept=7;0;").unwrap();
    let tag = vm.object_prototype.heap;
    let fuel = vm.remaining_instructions;
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        prepare_active_registration(&mut vm, None),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(resolve_active(tag).is_none());
    assert_eq!(active_backing_bytes(), 0);
    assert_eq!(vm.remaining_instructions, fuel);
    assert_eq!(
        vm.lookup_global_name("kept").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn active_realm_storage_terminal_neutral_nesting_keeps_outer_registration() {
    let mut owner = page();
    let view = observer(&owner);
    let mut neutral = Vm::default();
    let outer = enter(&mut owner);
    let before = view.host_scheduling_snapshot().payload_bytes;
    owner.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        prepare_active_registration(&mut neutral, None),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(resolve_active(neutral.object_prototype.heap).is_none());
    assert_eq!(
        resolve_active(owner.object_prototype.heap),
        Some(&mut owner as *mut Vm)
    );
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(outer);
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_failed_ancestor_does_not_poison_another_document() {
    let mut first = page();
    let mut second = page();
    let outer = enter(&mut first);
    first.host_scheduling.as_ref().unwrap().fail();
    let inner = enter(&mut second);
    assert!(second.check_scheduling_failure().is_ok());
    assert!(inner
        .node
        ._storage
        .as_ref()
        .unwrap()
        .belongs_to(second.host_scheduling.as_ref().unwrap()));
    drop(inner);
    drop(outer);
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_terminal_drop_refunds_live_metadata() {
    let mut vm = page();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let guard = enter(&mut vm);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(guard);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        view.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn active_realm_storage_finite_repeated_calls_release_each_node() {
    let mut vm = page();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    for _ in 0..3 {
        let guard = enter(&mut vm);
        assert_eq!(
            view.host_scheduling_snapshot().payload_bytes,
            before + std::mem::size_of::<ActiveVm>()
        );
        drop(guard);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(active_backing_bytes(), 0);
    }
}

fn observe_metadata(vm: &mut Vm) -> Rc<RefCell<Vec<usize>>> {
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        let bytes = ACTIVE.with(|active| {
            let active = active.borrow();
            let mut node = active.as_deref();
            let mut bytes = 0;
            while let Some(current) = node {
                assert!(current._storage.as_ref().unwrap().belongs_to(&scope));
                bytes += std::mem::size_of::<ActiveVm>();
                node = current.previous.get().and_then(Option::as_deref);
            }
            bytes
        });
        assert!(bytes >= 2 * std::mem::size_of::<ActiveVm>());
        output.borrow_mut().push(bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    seen
}

#[cfg_attr(test, test)]
fn active_realm_storage_shadow_callbacks_and_throws_release_the_chain() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let seen = observe_metadata(&mut vm);
    let view = observer(&vm);
    assert_eq!(script(&mut vm,
        "var r=new ShadowRealm();var call=r.evaluate('(callback)=>callback()+1');call(()=>{$262.gc();observe();return 7;});").unwrap(),
        Value::Number(8.0));
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(active_backing_bytes(), 0);
    assert!(matches!(
        script(&mut vm, "r.evaluate('throw 3;');"),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(active_backing_bytes(), 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn queued_callback(timer: bool) {
    let mut vm = page();
    if timer {
        vm.install_host_timer_functions().unwrap();
    }
    let seen = observe_metadata(&mut vm);
    let view = observer(&vm);
    script(&mut vm,
        "var result=0;var realm=new ShadowRealm();var wrapped=realm.evaluate('(callback)=>callback()+1');var handler=(function(fn){let kept=7;return function(){result=fn(()=>{observe();return kept;});};})(wrapped);").unwrap();
    script(&mut vm, if timer {
        "setTimeout(handler,1);handler=undefined;wrapped=undefined;realm=undefined;0;"
    } else {
        "var pending=Promise.withResolvers();pending.promise.then(handler);handler=undefined;wrapped=undefined;realm=undefined;0;"
    }).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(active_backing_bytes(), 0);
    if timer {
        assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    } else {
        script(&mut vm, "pending.resolve();pending=undefined;0;").unwrap();
        vm.run_promise_jobs_bounded(4).unwrap();
    }
    assert_eq!(script(&mut vm, "result;").unwrap(), Value::Number(8.0));
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(active_backing_bytes(), 0);
    vm.collect_realm_garbage().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn active_realm_storage_promise_callback_keeps_realm_and_credit_through_gc() {
    queued_callback(false);
}

#[cfg_attr(test, test)]
fn active_realm_storage_timer_callback_keeps_realm_and_credit_through_gc() {
    queued_callback(true);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_active_realm_storage_contracts() {
        active_realm_storage_registration_retains_document_metadata_credit();
        active_realm_storage_empty_registry_releases_backing();
        active_realm_storage_nested_documents_keep_independent_credit();
        active_realm_storage_preparation_charges_before_publication();
        active_realm_storage_unpublished_metadata_can_outlive_its_vm();
        active_realm_storage_nested_tags_restore_in_stack_order();
        active_realm_storage_duplicate_tags_keep_zero_owning_policy_views();
        active_realm_storage_peer_hint_preserves_neutral_policy();
        active_realm_storage_neutral_nesting_uses_nearest_metadata_owner();
        active_realm_storage_standalone_neutral_calls_release_metadata();
        active_realm_storage_terminal_preparation_preserves_execution_state();
        active_realm_storage_terminal_neutral_nesting_keeps_outer_registration();
        active_realm_storage_failed_ancestor_does_not_poison_another_document();
        active_realm_storage_terminal_drop_refunds_live_metadata();
        active_realm_storage_finite_repeated_calls_release_each_node();
        active_realm_storage_shadow_callbacks_and_throws_release_the_chain();
        active_realm_storage_promise_callback_keeps_realm_and_credit_through_gc();
        active_realm_storage_timer_callback_keeps_realm_and_credit_through_gc();
    }
}
