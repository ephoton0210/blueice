// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small, successful membrane operations retain their physical table ownership.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    match vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap()) {
        Ok(value) => value,
        Err(RuntimeError::Thrown(error)) => {
            panic!(
                "script threw {:?}",
                vm.get_property(&error, &"message".into())
            );
        }
        Err(error) => panic!("script failed: {error:?}"),
    }
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "var realm=$262.createRealm();0;");
    vm
}

#[cfg_attr(test, test)]
fn membrane_map_storage_forward_table_releases_its_own_native_credit() {
    let mut vm = page();
    let table = std::mem::take(&mut vm.test262_foreign_values);
    let bytes = host_scheduling::map_slots(&table);
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_nested_wrapper_table_releases_its_own_native_credit() {
    let mut vm = page();
    let realm = vm.test262_realms.values_mut().next().unwrap();
    let table = std::mem::take(&mut realm.wrappers);
    let bytes = host_scheduling::map_slots(&table);
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_import_identity_table_releases_its_own_native_credit() {
    let mut vm = page();
    run(&mut vm, "var local={answer:42};realm.global.value=local;0;");
    let realm = vm.test262_realms.values_mut().next().unwrap();
    let table = std::mem::take(&mut realm.imported_sources);
    let bytes = host_scheduling::map_slots(&table);
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
}

fn released<K: Eq + std::hash::Hash, V>(vm: &Vm, table: MembraneMap<K, V>) {
    let bytes = table.retained_bytes();
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
}
fn observer(scope: &SchedulingScope) -> Vm {
    Vm {
        host_scheduling: Some(scope.child()),
        ..Vm::default()
    }
}
fn put(map: &mut MembraneMap<u64, u64>, key: u64, scope: Option<&SchedulingScope>) {
    let prepared = map.prepare_insert(scope).unwrap();
    map.commit(prepared);
    map.insert(key, key + 1);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_import_value_table_owns_native_credit() {
    let mut vm = page();
    run(&mut vm, "var local={answer:42};realm.global.value=local;0;");
    let table = std::mem::take(
        &mut vm
            .test262_realms
            .values_mut()
            .next()
            .unwrap()
            .imported_values,
    );
    released(&vm, table);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_reverse_table_owns_native_credit() {
    let mut vm = page();
    run(&mut vm, "var local={answer:42};realm.global.value=local;0;");
    let child = vm.test262_realms.values().next().unwrap().vm.clone();
    let table = std::mem::take(&mut child.borrow_mut().test262_reverse_values);
    released(&vm, table);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_mirror_table_owns_native_credit() {
    let mut vm = page();
    run(
        &mut vm,
        "var foreign=realm.evalScript('new ArrayBuffer(2);');var local=new Uint8Array(foreign);0;",
    );
    let table = std::mem::take(&mut vm.test262_foreign_buffer_mirrors);
    released(&vm, table);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_reexport_cache_owns_native_credit() {
    let mut vm = page();
    run(&mut vm, "var second=$262.createRealm();var shared=realm.evalScript('new ShadowRealm();');second.global.value=shared;0;");
    let id = vm
        .test262_realms
        .iter()
        .find_map(|(id, realm)| (!realm.shadow_realm_reexports.is_empty()).then_some(*id))
        .unwrap();
    let table = std::mem::take(
        &mut vm
            .test262_realms
            .get_mut(&id)
            .unwrap()
            .shadow_realm_reexports,
    );
    released(&vm, table);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_realm_table_owns_backing_separately_from_records() {
    let mut vm = page();
    let id = *vm.test262_realms.keys().next().unwrap();
    let record = vm.test262_realms.remove(&id).unwrap();
    let table = std::mem::take(&mut vm.test262_realms);
    released(&vm, table);
    assert!(record.vm.borrow().host_scheduling.is_some());
}

#[cfg_attr(test, test)]
fn membrane_map_storage_rust_held_table_remains_charged_after_vm_teardown() {
    let mut vm = page();
    let view = observer(vm.host_scheduling.as_ref().unwrap());
    let table = std::mem::take(&mut vm.test262_foreign_values);
    let bytes = table.retained_bytes();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= bytes);
    released(&view, table);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_growth_owns_old_and_candidate_backing_until_commit() {
    let scope = SchedulingScope::new();
    let view = observer(&scope);
    let mut map = MembraneMap::default();
    for key in [1, 2, 3] {
        put(&mut map, key, Some(&scope));
    }
    let old = map.retained_bytes();
    let prepared = map.prepare_insert(Some(&scope)).unwrap();
    let candidate = prepared.staging_bytes();
    assert!(candidate > old);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        old + candidate
    );
    map.commit(prepared);
    map.insert(4, 5);
    assert_eq!(map.len(), 4);
    assert_eq!(map.get(&1), Some(&2));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, candidate);
    assert_eq!(map.retained_bytes(), candidate);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_reusable_slot_keeps_one_backing_owner() {
    let scope = SchedulingScope::new();
    let view = observer(&scope);
    let mut map = MembraneMap::default();
    put(&mut map, 1, Some(&scope));
    let bytes = map.retained_bytes();
    let (_, peak) = scope.measure_payload_peak(|| put(&mut map, 2, Some(&scope)));
    assert_eq!(peak, 0);
    assert_eq!(map.retained_bytes(), bytes);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_abandoned_preparation_refunds_only_candidate() {
    let scope = SchedulingScope::new();
    let view = observer(&scope);
    let mut map = MembraneMap::default();
    for key in [1, 2, 3] {
        put(&mut map, key, Some(&scope));
    }
    let bytes = map.retained_bytes();
    let prepared = map.prepare_insert(Some(&scope)).unwrap();
    assert!(prepared.staging_bytes() > 0);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&3), Some(&4));
}

#[cfg_attr(test, test)]
fn membrane_map_storage_neutral_backing_attaches_once() {
    let scope = SchedulingScope::new();
    let view = observer(&scope);
    let mut map = MembraneMap::default();
    put(&mut map, 1, None);
    let bytes = map.retained_bytes();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    map.attach(&scope).unwrap();
    map.attach(&scope).unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    released(&view, map);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_foreign_owner_is_preserved_and_original_stays_healthy() {
    let scope = SchedulingScope::new();
    let foreign = SchedulingScope::new();
    let view = observer(&scope);
    let mut map = MembraneMap::default();
    put(&mut map, 1, Some(&scope));
    let bytes = map.retained_bytes();
    assert!(!map.can_join(&foreign));
    assert!(matches!(
        map.prepare_insert(Some(&foreign)),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    put(&mut map, 2, Some(&scope));
    assert_eq!(map.get(&2), Some(&3));
    released(&view, map);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_bound_table_cannot_silently_become_neutral() {
    let scope = SchedulingScope::new();
    let mut map = MembraneMap::default();
    put(&mut map, 1, Some(&scope));
    assert!(matches!(
        map.prepare_insert(None),
        Err(RuntimeError::SchedulingLimit)
    ));
    put(&mut map, 2, Some(&scope));
    assert_eq!(map.len(), 2);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_terminal_cleanup_frees_backing_before_native_credit() {
    let scope = SchedulingScope::new();
    let view = observer(&scope);
    let mut map = MembraneMap::default();
    put(&mut map, 1, Some(&scope));
    let bytes = map.retained_bytes();
    assert!(matches!(scope.fail(), RuntimeError::SchedulingLimit));
    assert!(matches!(
        map.prepare_insert(Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(map.remove(&1), Some(2));
    assert_eq!(map.retained_bytes(), bytes);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    map.release_empty();
    assert_eq!(map.retained_bytes(), 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_late_document_activation_attaches_existing_tables() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        "var realm=$262.createRealm();var value={answer:42};realm.global.value=value;0;",
    );
    let bytes = vm.unowned_membrane_storage_bytes();
    assert!(bytes > 0);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.unowned_membrane_storage_bytes(), 0);
    let table = std::mem::take(
        &mut vm
            .test262_realms
            .values_mut()
            .next()
            .unwrap()
            .imported_sources,
    );
    released(&vm, table);
}

#[cfg_attr(test, test)]
fn membrane_map_storage_source_getter_can_publish_another_transport() {
    let mut vm = page();
    run(&mut vm, "var value=[7,8];Object.defineProperty(value,'0',{get:function(){realm.global.extra={answer:41};return 7;}});realm.global.value=value;0;");
    assert_eq!(
        run(&mut vm, "realm.evalScript('value[0]+extra.answer;');"),
        Value::Number(48.0)
    );
    assert_eq!(
        run(&mut vm, "realm.global.value===value;"),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_membrane_map_storage_contracts() {
        membrane_map_storage_forward_table_releases_its_own_native_credit();
        membrane_map_storage_nested_wrapper_table_releases_its_own_native_credit();
        membrane_map_storage_import_identity_table_releases_its_own_native_credit();
        membrane_map_storage_import_value_table_owns_native_credit();
        membrane_map_storage_reverse_table_owns_native_credit();
        membrane_map_storage_mirror_table_owns_native_credit();
        membrane_map_storage_reexport_cache_owns_native_credit();
        membrane_map_storage_realm_table_owns_backing_separately_from_records();
        membrane_map_storage_rust_held_table_remains_charged_after_vm_teardown();
        membrane_map_storage_growth_owns_old_and_candidate_backing_until_commit();
        membrane_map_storage_reusable_slot_keeps_one_backing_owner();
        membrane_map_storage_abandoned_preparation_refunds_only_candidate();
        membrane_map_storage_neutral_backing_attaches_once();
        membrane_map_storage_foreign_owner_is_preserved_and_original_stays_healthy();
        membrane_map_storage_bound_table_cannot_silently_become_neutral();
        membrane_map_storage_terminal_cleanup_frees_backing_before_native_credit();
        membrane_map_storage_late_document_activation_attaches_existing_tables();
        membrane_map_storage_source_getter_can_publish_another_transport();
    }
}
