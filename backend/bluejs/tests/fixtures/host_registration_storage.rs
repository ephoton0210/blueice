// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny registration ownership and native-index publication contracts.

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

fn install_all(vm: &mut Vm, family: HostObjectFamily) -> Rc<std::cell::Cell<f64>> {
    let owner = [7u64, 4];
    vm.install_host_object_factory("target", 1, family, move |args: &[HostValue]| {
        let Some(HostValue::Number(id)) = args.first() else {
            return Ok(None);
        };
        Ok(Some(HostObjectKey::new(owner[0], owner[1], *id as u64)))
    })
    .unwrap();
    let offset = [5u64];
    vm.install_host_object_method(
        family,
        "read",
        0,
        move |key: HostObjectKey, _: &[HostValue]| {
            Ok(HostValue::Number((key.object() + offset[0]) as f64))
        },
    )
    .unwrap();
    let owner = [7u64, 4];
    vm.install_host_object_pair_method(
        family,
        "append",
        1,
        move |parent: HostObjectKey, child: HostObjectKey| {
            if !parent.matches_owner(owner[0], owner[1]) || !child.matches_owner(owner[0], owner[1])
            {
                return Err(HostFunctionError::new("pair owner"));
            }
            Ok(())
        },
    )
    .unwrap();
    let state = Rc::new(std::cell::Cell::new(1.0));
    let getter = state.clone();
    let setter = state.clone();
    vm.install_host_object_accessor(
        family,
        "value",
        move |_: HostObjectKey, _: &[HostValue]| Ok(HostValue::Number(getter.get())),
        move |_: HostObjectKey, args: &[HostValue]| {
            if let Some(HostValue::Number(value)) = args.first() {
                setter.set(*value);
            }
            Ok(HostValue::Undefined)
        },
    )
    .unwrap();
    state
}

fn retained_bytes(vm: &Vm) -> usize {
    vm.host_object_factories.capacity() * std::mem::size_of::<HostObjectFactoryRegistration>()
        + vm.host_object_methods.capacity() * std::mem::size_of::<HostObjectMethodRegistration>()
        + vm.host_object_pair_methods.capacity()
            * std::mem::size_of::<HostObjectPairMethodRegistration>()
        + vm.host_object_factories
            .iter()
            .map(|record| std::mem::size_of_val(&*record.factory))
            .sum::<usize>()
        + vm.host_object_methods
            .iter()
            .map(|record| std::mem::size_of_val(&*record.method))
            .sum::<usize>()
        + vm.host_object_pair_methods
            .iter()
            .map(|record| std::mem::size_of_val(&*record.method))
            .sum::<usize>()
}

fn loaned_family(vm: &mut Vm, source: &Vm) -> HostObjectFamily {
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    vm.with_realm_call_roots(roots, |vm| vm.create_host_object_family())
        .unwrap()
}

#[cfg_attr(test, test)]
fn host_registration_storage_records_and_all_callback_boxes_outlive_the_vm() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let _host_state = install_all(&mut vm, family);
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    assert!(bytes > 0);
    let records = (
        std::mem::take(&mut vm.host_object_factories),
        std::mem::take(&mut vm.host_object_methods),
        std::mem::take(&mut vm.host_object_pair_methods),
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_registration_storage_neutral_records_adopt_backing_and_callback_credit() {
    let mut vm = Vm::default();
    let family = vm.create_host_object_family().unwrap();
    let _host_state = install_all(&mut vm, family);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let records = (
        std::mem::take(&mut vm.host_object_factories),
        std::mem::take(&mut vm.host_object_methods),
        std::mem::take(&mut vm.host_object_pair_methods),
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_registration_storage_terminal_method_does_not_publish_a_native_index() {
    let source = page();
    let mut vm = Vm::default();
    let family = loaned_family(&mut vm, &source);
    let prototype = vm.host_object_families[family.index as usize].prototype;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.install_host_object_method(family, "read", 0, |_: HostObjectKey, _: &[HostValue]| Ok(
            HostValue::Number(7.0)
        )),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .heap
        .get_own_property_descriptor(prototype, "read")
        .unwrap()
        .is_none());
    assert!(vm.host_object_methods.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_terminal_pair_does_not_publish_a_native_index() {
    let source = page();
    let mut vm = Vm::default();
    let family = loaned_family(&mut vm, &source);
    let prototype = vm.host_object_families[family.index as usize].prototype;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.install_host_object_pair_method(
            family,
            "append",
            1,
            |_: HostObjectKey, _: HostObjectKey| Ok(())
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .heap
        .get_own_property_descriptor(prototype, "append")
        .unwrap()
        .is_none());
    assert!(vm.host_object_pair_methods.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_terminal_accessor_does_not_publish_either_callback() {
    let source = page();
    let mut vm = Vm::default();
    let family = loaned_family(&mut vm, &source);
    let prototype = vm.host_object_families[family.index as usize].prototype;
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.install_host_object_accessor(
            family,
            "value",
            |_: HostObjectKey, _: &[HostValue]| Ok(HostValue::Number(7.0)),
            |_: HostObjectKey, _: &[HostValue]| Ok(HostValue::Undefined)
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .heap
        .get_own_property_descriptor(prototype, "value")
        .unwrap()
        .is_none());
    assert!(vm.host_object_methods.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_terminal_factory_precedes_global_object_allocation() {
    let source = page();
    let mut vm = Vm::default();
    let family = loaned_family(&mut vm, &source);
    let before = vm.heap.stats();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let result = vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| Ok(None));
    let after = vm.heap.stats();
    assert_eq!(
        after.nursery_objects + after.tenured_objects,
        before.nursery_objects + before.tenured_objects
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.host_object_factories.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_terminal_receiver_factory_preserves_its_owner() {
    let source = page();
    let mut vm = Vm::default();
    let family = loaned_family(&mut vm, &source);
    let object = vm
        .install_host_family_object("holder", family, HostObjectKey::new(7, 4, 1))
        .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.install_host_object_factory_method(object, "child", 0, family, |_: &[HostValue]| Ok(
            None
        )),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm
        .heap
        .get_own_property_descriptor(object.0, "child")
        .unwrap()
        .is_none());
    assert!(vm.host_object_factories.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_public_indices_and_accessor_state_are_preserved() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let host_state = install_all(&mut vm, family);
    assert_eq!(run(&mut vm,"var parent=target(1),child=target(2);parent.value=9;parent.read()===6&&parent.append(child)===child&&parent.value===9&&parent===target(1);"),Value::Bool(true));
    assert_eq!(host_state.get(), 9.0);
    assert_eq!(vm.host_object_methods.len(), 3);
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_duplicate_rejection_keeps_the_original_record() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_method(family, "read", 0, |_: HostObjectKey, _: &[HostValue]| {
        Ok(HostValue::Number(7.0))
    })
    .unwrap();
    let prototype = vm.host_object_families[family.index as usize].prototype;
    let before = vm.heap.get_own(prototype, "read").unwrap();
    assert!(matches!(
        vm.install_host_object_method(family, "read", 0, |_: HostObjectKey, _: &[HostValue]| Ok(
            HostValue::Number(8.0)
        )),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(vm.host_object_methods.len(), 1);
    assert_eq!(vm.heap.get_own(prototype, "read").unwrap(), before);
}

#[cfg_attr(test, test)]
fn host_registration_storage_prepared_accessor_keeps_both_callbacks_and_new_backing_paid() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let host_state = install_all(&mut vm, family);
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let owner = vm.argument_list_owner().unwrap();
    let prepared = vm.host_object_methods.prepare(2, owner.as_ref()).unwrap();
    let slots = prepared.staging_bytes();
    assert!(slots > 0);
    let getter_state = host_state.clone();
    let setter_state = host_state.clone();
    let getter = move |_: HostObjectKey, _: &[HostValue]| Ok(HostValue::Number(getter_state.get()));
    let setter = move |_: HostObjectKey, args: &[HostValue]| {
        if let Some(HostValue::Number(value)) = args.first() {
            setter_state.set(*value);
        }
        Ok(HostValue::Undefined)
    };
    let sizes = [
        std::mem::size_of_val(&getter),
        std::mem::size_of_val(&setter),
    ];
    let callbacks = prepared
        .callbacks(&sizes, || {
            [
                HostObjectMethodRegistration {
                    family_index: family.index,
                    method: Box::new(getter),
                },
                HostObjectMethodRegistration {
                    family_index: family.index,
                    method: Box::new(setter),
                },
            ]
        })
        .unwrap();
    let bytes = slots + sizes.iter().sum::<usize>();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    assert_eq!(vm.host_object_methods.len(), 3);
    assert!(vm
        .heap
        .get_own_property_descriptor(vm.host_object_families[0].prototype, "nextValue")
        .unwrap()
        .is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(callbacks);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

struct DropProbe {
    view: Rc<Vm>,
    seen: Rc<std::cell::Cell<usize>>,
}
impl DropProbe {
    fn touch(&self) {
        let _ = self.view.host_scheduling_snapshot();
    }
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.seen
            .set(self.view.host_scheduling_snapshot().payload_bytes);
    }
}

#[cfg_attr(test, test)]
fn host_registration_storage_callback_drop_observes_its_still_owned_allocation() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let view = Rc::new(observer(&vm));
    let seen = Rc::new(std::cell::Cell::new(0));
    let probe = DropProbe {
        view: view.clone(),
        seen: seen.clone(),
    };
    let callback = move |_: HostObjectKey, _: &[HostValue]| {
        probe.touch();
        Ok(HostValue::Undefined)
    };
    let size = std::mem::size_of_val(&callback);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let owner = vm.argument_list_owner().unwrap();
    let prepared = vm.host_object_methods.prepare(1, owner.as_ref()).unwrap();
    let slots = prepared.staging_bytes();
    let callbacks = prepared
        .callbacks(&[size], || {
            [HostObjectMethodRegistration {
                family_index: family.index,
                method: Box::new(callback),
            }]
        })
        .unwrap();
    drop(callbacks);
    assert_eq!(seen.get(), before + slots + size);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + slots
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

struct DropCount(Rc<std::cell::Cell<usize>>);
impl DropCount {
    fn touch(&self) {
        let _ = self.0.get();
    }
}
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[cfg_attr(test, test)]
fn host_registration_storage_failed_property_installation_releases_the_callback() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("target", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 4, 1)))
    })
    .unwrap();
    run(
        &mut vm,
        "Object.preventExtensions(Object.getPrototypeOf(target()));0;",
    );
    let view = observer(&vm);
    let drops = Rc::new(std::cell::Cell::new(0));
    let probe = DropCount(drops.clone());
    let result = vm.install_host_object_method(
        family,
        "read",
        0,
        move |_: HostObjectKey, _: &[HostValue]| {
            probe.touch();
            Ok(HostValue::Number(7.0))
        },
    );
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert_eq!(drops.get(), 1);
    assert!(vm.host_object_methods.is_empty());
    assert!(vm
        .heap
        .get_own_property_descriptor(vm.host_object_families[0].prototype, "read")
        .unwrap()
        .is_none());
    assert!(vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_registration_storage_growth_releases_old_backing_without_losing_boxes() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    for (index, name) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        let value = [index as u64];
        vm.install_host_object_method(family, name, 0, move |_: HostObjectKey, _: &[HostValue]| {
            Ok(HostValue::Number(value[0] as f64))
        })
        .unwrap();
    }
    let view = observer(&vm);
    let bytes = retained_bytes(&vm);
    let records = std::mem::take(&mut vm.host_object_methods);
    assert_eq!(records.len(), 5);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_registration_storage_neutral_callback_entry_adopts_existing_records() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let family = vm.create_host_object_family().unwrap();
    let object = vm
        .mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 1))
        .unwrap();
    let value = [7u64];
    vm.install_host_object_method(
        family,
        "read",
        0,
        move |_: HostObjectKey, _: &[HostValue]| Ok(HostValue::Number(value[0] as f64)),
    )
    .unwrap();
    let bytes = retained_bytes(&vm);
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&vm.heap, object).unwrap();
    assert_eq!(
        vm.with_realm_call_roots(roots, |vm| vm.host_object_method_call(
            0,
            Value::Object(object),
            &[],
            false
        )),
        Ok(Value::Number(7.0))
    );
    assert!(vm.realm_call_roots.is_none());
    let records = std::mem::take(&mut vm.host_object_methods);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    drop(records);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_registration_storage_each_registry_keeps_its_original_document() {
    let source = page();
    let other = page();
    let mut vm = Vm::default();
    let family = loaned_family(&mut vm, &source);
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    let _host_state = vm
        .with_realm_call_roots(roots, |vm| Ok(install_all(vm, family)))
        .unwrap();
    assert!(vm.host_registration_storage_can_join(source.host_scheduling.as_ref().unwrap()));
    assert!(!vm.host_registration_storage_can_join(other.host_scheduling.as_ref().unwrap()));
    for owner in [
        vm.host_object_factories.owner_view().unwrap(),
        vm.host_object_methods.owner_view().unwrap(),
        vm.host_object_pair_methods.owner_view().unwrap(),
    ] {
        assert!(owner
            .unwrap()
            .belongs_to(source.host_scheduling.as_ref().unwrap()));
    }
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
    assert!(other
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
}

#[cfg_attr(test, test)]
fn host_registration_storage_manual_timer_and_promise_preserve_callable_indices_through_gc() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let _host_state = install_all(&mut vm, family);
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm,"var result=false;setTimeout(()=>{let value=target(1);value.value=13;Promise.resolve(value).then(object=>{$262.gc();result=object.read()===6&&object.value===13&&object===target(1);});},1);0;");
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::Bool(true));
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_registration_storage_terminal_prepared_owner_rejects_even_zero_sized_callbacks() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let view = observer(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let prepared = vm.host_object_methods.prepare(1, owner.as_ref()).unwrap();
    let slots = prepared.staging_bytes();
    let built = std::cell::Cell::new(false);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    let result = prepared.callbacks(&[0], || {
        built.set(true);
        [HostObjectMethodRegistration {
            family_index: family.index,
            method: Box::new(|_: HostObjectKey, _: &[HostValue]| Ok(HostValue::Undefined)),
        }]
    });
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(!built.get());
    assert!(vm.host_object_methods.is_empty());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_registration_storage_contracts() {
        host_registration_storage_records_and_all_callback_boxes_outlive_the_vm();
        host_registration_storage_neutral_records_adopt_backing_and_callback_credit();
        host_registration_storage_terminal_method_does_not_publish_a_native_index();
        host_registration_storage_terminal_pair_does_not_publish_a_native_index();
        host_registration_storage_terminal_accessor_does_not_publish_either_callback();
        host_registration_storage_terminal_factory_precedes_global_object_allocation();
        host_registration_storage_terminal_receiver_factory_preserves_its_owner();
        host_registration_storage_public_indices_and_accessor_state_are_preserved();
        host_registration_storage_duplicate_rejection_keeps_the_original_record();
        host_registration_storage_prepared_accessor_keeps_both_callbacks_and_new_backing_paid();
        host_registration_storage_callback_drop_observes_its_still_owned_allocation();
        host_registration_storage_failed_property_installation_releases_the_callback();
        host_registration_storage_growth_releases_old_backing_without_losing_boxes();
        host_registration_storage_neutral_callback_entry_adopts_existing_records();
        host_registration_storage_each_registry_keeps_its_original_document();
        host_registration_storage_manual_timer_and_promise_preserve_callable_indices_through_gc();
        host_registration_storage_terminal_prepared_owner_rejects_even_zero_sized_callbacks();
    }
}
