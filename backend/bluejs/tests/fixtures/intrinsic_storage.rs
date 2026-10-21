// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny late-intrinsic preparation, ownership and deletion contracts.

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

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn base(vm: &mut Vm) -> ObjectId {
    run(vm, "Iterator.prototype;0;");
    vm.iterator_base.unwrap()
}

fn loaned_install(neutral: &mut Vm, source: &Vm) -> ObjectId {
    let object = base(neutral);
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    neutral
        .with_realm_call_roots(roots, |vm| {
            vm.materialize_iterator_helper_property(object, &"flatMap".into())
        })
        .unwrap();
    assert!(neutral.host_scheduling.is_none() && neutral.realm_call_roots.is_none());
    object
}

#[cfg_attr(test, test)]
fn intrinsic_storage_inline_flags_retain_origin_without_a_heap_backing_charge() {
    let mut vm = page();
    let view = observer(&vm);
    run(&mut vm, "Iterator.prototype.flatMap;0;");
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let flags = std::mem::take(&mut vm.iterator_helpers_installed);
    assert!(flags.contains(LateIteratorHelper::FlatMap));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    let _ = scope.fail();
    assert_eq!(flags.validate_owner(), Err(RuntimeError::SchedulingLimit));
    drop(flags);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_prepares_original_identity_before_publishing_a_property() {
    let mut vm = page();
    let object = base(&mut vm);
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let prepared = vm.prepare_iterator_helper_installation().unwrap();
    assert!(vm.iterator_helpers_installed.is_empty());
    assert!(vm
        .heap
        .get_own_property_descriptor(object, "flatMap")
        .unwrap()
        .is_none());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        prepared.validate_origin(),
        Err(RuntimeError::SchedulingLimit)
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_neutral_flags_adopt_document_identity_without_backing_charge() {
    let mut vm = Vm::default();
    run(&mut vm, "Iterator.prototype.flatMap;0;");
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert!(vm
        .iterator_helpers_installed
        .owner
        .as_ref()
        .unwrap()
        .belongs_to(&scope));
    let view = observer(&vm);
    let flags = std::mem::take(&mut vm.iterator_helpers_installed);
    assert!(flags.can_join(&scope));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    let _ = scope.fail();
    assert_eq!(flags.validate_owner(), Err(RuntimeError::SchedulingLimit));
    drop(flags);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_original_terminal_loan_prevents_another_helper_publication() {
    let source = page();
    let mut neutral = Vm::default();
    let object = loaned_install(&mut neutral, &source);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        neutral.materialize_iterator_helper_property(object, &"chunks".into()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral
        .heap
        .get_own_property_descriptor(object, "chunks")
        .unwrap()
        .is_none());
    assert_eq!(neutral.iterator_helpers_installed.len(), 1);
    assert!(neutral
        .iterator_helpers_installed
        .contains(LateIteratorHelper::FlatMap));
}

#[cfg_attr(test, test)]
fn intrinsic_storage_foreign_document_cannot_reassign_existing_flag_origin() {
    let source = page();
    let mut neutral = Vm::default();
    loaned_install(&mut neutral, &source);
    assert_eq!(
        neutral.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!neutral.document_policy_ready);
    assert_eq!(
        neutral
            .host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge(0)
            .err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert!(neutral
        .iterator_helpers_installed
        .can_join(source.host_scheduling.as_ref().unwrap()));
    assert!(source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

#[cfg_attr(test, test)]
fn intrinsic_storage_public_descriptors_and_deleted_helpers_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var proto=Iterator.prototype;
        var good=['flatMap','chunks','windows'].every(name=>{var desc=Object.getOwnPropertyDescriptor(proto,name);return desc.value.name===name&&desc.value.length===1&&desc.writable&&!desc.enumerable&&desc.configurable;});
        delete proto.flatMap;delete proto.chunks;delete proto.windows;
        good&&proto.flatMap===undefined&&Object.getOwnPropertyDescriptor(proto,'chunks')===undefined&&!Reflect.has(proto,'windows');
    "#
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.iterator_helpers_installed.len(), 3);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_deleting_unread_helpers_preserves_their_absence() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var proto=Iterator.prototype;
        delete proto.flatMap;delete proto.chunks;delete proto.windows;
        proto.flatMap===undefined&&Object.getOwnPropertyDescriptor(proto,'chunks')===undefined&&!Reflect.has(proto,'windows');
    "#,
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.iterator_helpers_installed.len(), 3);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_deleted_user_replacements_are_not_reinstalled() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var proto=Iterator.prototype;
        Object.defineProperty(proto,'flatMap',{value:17,configurable:true});
        Object.defineProperty(proto,'chunks',{get(){return 23;},configurable:true});
        var replaced=proto.flatMap===17&&proto.chunks===23;
        delete proto.flatMap;delete proto.chunks;
        replaced&&proto.flatMap===undefined&&Object.getOwnPropertyDescriptor(proto,'chunks')===undefined;
    "#,
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.iterator_helpers_installed.len(), 2);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_failed_property_installation_does_not_publish_a_flag() {
    let mut vm = page();
    let object = base(&mut vm);
    run(&mut vm, "Object.preventExtensions(Iterator.prototype);0;");
    assert!(matches!(
        vm.materialize_iterator_helper_property(object, &"flatMap".into()),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(vm.iterator_helpers_installed.is_empty());
    assert!(vm.iterator_helpers_installed.owner.is_none());
    assert!(vm
        .heap
        .get_own_property_descriptor(object, "flatMap")
        .unwrap()
        .is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn intrinsic_storage_unrelated_keys_and_receivers_leave_flags_empty() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            "var proto=Iterator.prototype; Object.getOwnPropertyDescriptor(proto,Symbol('flatMap'))===undefined&&Object.getOwnPropertyDescriptor({},'flatMap')===undefined&&proto.unrelated===undefined;",
        ),
        Value::Bool(true)
    );
    assert!(vm.iterator_helpers_installed.is_empty());
    assert!(vm.iterator_helpers_installed.owner.is_none());
    let other = vm.heap.alloc_object(None).unwrap();
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.materialize_iterator_helper_property(other, &"flatMap".into()),
        Ok(())
    );
    assert_eq!(
        vm.materialize_iterator_helper_property(vm.iterator_base.unwrap(), &"unrelated".into()),
        Ok(())
    );
    assert!(vm.iterator_helpers_installed.is_empty());
}

#[cfg_attr(test, test)]
fn intrinsic_storage_reflection_and_repeated_reads_preserve_function_identity() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var proto=Iterator.prototype,first=proto.flatMap;
        first===proto.flatMap&&first===Reflect.get(proto,'flatMap')&&first===Object.getOwnPropertyDescriptor(proto,'flatMap').value;
    "#,
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.iterator_helpers_installed.len(), 1);
    assert_eq!(
        vm.iterator_helpers_installed
            .owner
            .as_ref()
            .unwrap()
            .bytes(),
        0
    );
}

#[cfg_attr(test, test)]
fn intrinsic_storage_all_three_helpers_execute_across_explicit_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var values=[{n:1},{n:2},{n:3}];
        var flat=values.values().flatMap(value=>{$262.gc();return [value.n,value.n+10];});
        var chunks=values.values().chunks(2),windows=values.values().windows(2);
        $262.gc();
        flat.toArray().join()==='1,11,2,12,3,13'&&
        chunks.map(group=>group.map(value=>value.n).join()).toArray().join('|')==='1,2|3'&&
        windows.map(group=>group.map(value=>value.n).join()).toArray().join('|')==='1,2|2,3';
    "#,
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.iterator_helpers_installed.len(), 3);
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn intrinsic_storage_terminal_first_install_preserves_property_and_outer_stack() {
    let mut vm = page();
    let object = base(&mut vm);
    vm.stack.push(Value::Number(9.0));
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.materialize_iterator_helper_property(object, &"flatMap".into()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .heap
        .get_own_property_descriptor(object, "flatMap")
        .unwrap()
        .is_none());
    assert!(vm.iterator_helpers_installed.is_empty());
    assert!(vm.iterator_helpers_installed.owner.is_none());
    assert_eq!(vm.stack, [Value::Number(9.0)]);
}

#[cfg_attr(test, test)]
fn intrinsic_storage_original_termination_does_not_poison_another_borrower() {
    let source = page();
    let other = page();
    let mut neutral = Vm::default();
    let object = loaned_install(&mut neutral, &source);
    let owner = other.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let result = neutral.with_realm_call_roots(roots, |vm| {
        vm.materialize_iterator_helper_property(object, &"chunks".into())
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(neutral.realm_call_roots.is_none());
    assert!(neutral
        .iterator_helpers_installed
        .can_join(source.host_scheduling.as_ref().unwrap()));
    assert!(!neutral
        .iterator_helpers_installed
        .can_join(other.host_scheduling.as_ref().unwrap()));
    assert!(other
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
    assert!(neutral
        .heap
        .get_own_property_descriptor(object, "chunks")
        .unwrap()
        .is_none());
}

#[cfg_attr(test, test)]
fn intrinsic_storage_abandoned_preparation_does_not_bind_empty_flags() {
    let source = page();
    let mut neutral = Vm::default();
    let object = base(&mut neutral);
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    let prepared = neutral
        .with_realm_call_roots(roots, |vm| vm.prepare_iterator_helper_installation())
        .unwrap();
    assert!(neutral.iterator_helpers_installed.owner.is_none());
    drop(prepared);
    neutral.use_bounded_host_microtask_checkpoints().unwrap();
    assert!(!neutral
        .host_scheduling
        .as_ref()
        .unwrap()
        .same_document(source.host_scheduling.as_ref().unwrap()));
    assert!(neutral.iterator_helpers_installed.is_empty());
    assert!(neutral.iterator_helpers_installed.owner.is_none());
    neutral
        .materialize_iterator_helper_property(object, &"chunks".into())
        .unwrap();
    assert!(neutral
        .iterator_helpers_installed
        .can_join(neutral.host_scheduling.as_ref().unwrap()));
    assert!(source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_intrinsic_storage_contracts() {
        intrinsic_storage_inline_flags_retain_origin_without_a_heap_backing_charge();
        intrinsic_storage_prepares_original_identity_before_publishing_a_property();
        intrinsic_storage_neutral_flags_adopt_document_identity_without_backing_charge();
        intrinsic_storage_original_terminal_loan_prevents_another_helper_publication();
        intrinsic_storage_foreign_document_cannot_reassign_existing_flag_origin();
        intrinsic_storage_public_descriptors_and_deleted_helpers_are_preserved();
        intrinsic_storage_deleting_unread_helpers_preserves_their_absence();
        intrinsic_storage_deleted_user_replacements_are_not_reinstalled();
        intrinsic_storage_failed_property_installation_does_not_publish_a_flag();
        intrinsic_storage_unrelated_keys_and_receivers_leave_flags_empty();
        intrinsic_storage_reflection_and_repeated_reads_preserve_function_identity();
        intrinsic_storage_all_three_helpers_execute_across_explicit_gc();
        intrinsic_storage_terminal_first_install_preserves_property_and_outer_stack();
        intrinsic_storage_original_termination_does_not_poison_another_borrower();
        intrinsic_storage_abandoned_preparation_does_not_bind_empty_flags();
    }
}
