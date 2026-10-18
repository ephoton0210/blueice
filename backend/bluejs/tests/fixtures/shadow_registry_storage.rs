// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed small ShadowRealm registries retain their native document ownership.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    run(&mut vm, "var realm=new ShadowRealm();0;");
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_instance_table_retains_native_credit() {
    let mut vm = page();
    let mut table = std::mem::take(&mut vm.shadow_realms);
    let key = *table.keys().next().unwrap();
    let record = table.remove(&key).unwrap();
    let bytes = host_scheduling::map_slots(&table);
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
    drop(record);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_reverse_index_retains_native_credit() {
    let mut vm = page();
    let table = std::mem::take(&mut vm.shadow_realm_by_heap);
    let bytes = host_scheduling::map_slots(&table);
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_callable_table_retains_native_credit() {
    let mut vm = page();
    run(&mut vm, "var callback=realm.evaluate('()=>42');0;");
    let mut table = std::mem::take(&mut vm.shadow_wrapped_functions);
    let key = *table.keys().next().unwrap();
    let record = table.remove(&key).unwrap();
    let bytes = host_scheduling::map_slots(&table);
    assert!(bytes > 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    drop(table);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes + bytes, before);
    drop(record);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_owner_metadata_outlives_all_vm_data() {
    let mut vm = page();
    let view = observer(&vm);
    let key = *vm.shadow_realms.keys().next().unwrap();
    let record = vm.shadow_realms.remove(&key).unwrap();
    let owner = record.owner.clone();
    let owners = record.owners.clone();
    drop(record);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(owner);
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(owners);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn scope_view(scope: &SchedulingScope) -> Vm {
    Vm {
        host_scheduling: Some(scope.child()),
        ..Vm::default()
    }
}
fn owner_list(
    scope: Option<&SchedulingScope>,
) -> (
    shadow_owner_storage::RealmOwners,
    shadow_owner_storage::RealmOwner,
) {
    let owners = shadow_owner_storage::RealmOwners::new(scope).unwrap();
    let owner = owners.commit_owner(owners.prepare_owner(scope).unwrap());
    (owners, owner)
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_owner_list_counts_shared_header_and_identity_nodes() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    assert_eq!(owners.len(), 1);
    assert!(!owners.has_external_owner(&[owner.identity()]));
    assert!(owners.has_external_owner(&[]));
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        owners.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_dead_weak_owner_keeps_credit_until_pruning() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let before = view.host_scheduling_snapshot().payload_bytes;
    drop(owner);
    assert!(!owners.has_external_owner(&[]));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    owners.prune();
    assert_eq!(owners.len(), 0);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        owners.retained_bytes()
    );
    drop(owners);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_clones_share_identity_and_native_credit() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let before = view.host_scheduling_snapshot().payload_bytes;
    let copy = owner.clone();
    let list_copy = owners.clone();
    assert_eq!(copy.identity(), owner.identity());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(owner);
    drop(owners);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(list_copy);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        shadow_owner_storage::RealmOwner::bytes()
    );
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_prepared_owner_is_fully_charged_before_publication() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let mut tokens = vec![owner];
    for _ in [1, 2, 3] {
        tokens.push(owners.commit_owner(owners.prepare_owner(Some(&scope)).unwrap()));
    }
    let before = view.host_scheduling_snapshot().payload_bytes;
    let prepared = owners.prepare_owner(Some(&scope)).unwrap();
    assert_eq!(
        prepared.staging_bytes(),
        shadow_owner_storage::RealmOwner::bytes()
    );
    assert_eq!(owners.len(), 4);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + prepared.staging_bytes()
    );
    tokens.push(owners.commit_owner(prepared));
    assert_eq!(owners.len(), 5);
    assert_eq!(tokens.len(), 5);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        owners.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_abandoned_owner_preparation_publishes_nothing() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let mut tokens = vec![owner];
    for _ in [1, 2, 3] {
        tokens.push(owners.commit_owner(owners.prepare_owner(Some(&scope)).unwrap()));
    }
    let before = view.host_scheduling_snapshot().payload_bytes;
    let identity = tokens[0].identity();
    let prepared = owners.prepare_owner(Some(&scope)).unwrap();
    assert!(view.host_scheduling_snapshot().payload_bytes > before);
    drop(prepared);
    assert_eq!(owners.len(), 4);
    assert_eq!(tokens[0].identity(), identity);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_append_charges_only_one_complete_node() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let before = view.host_scheduling_snapshot().payload_bytes;
    let (second, peak) = scope
        .measure_payload_peak(|| owners.commit_owner(owners.prepare_owner(Some(&scope)).unwrap()));
    assert_ne!(second.identity(), owner.identity());
    assert_eq!(peak, shadow_owner_storage::RealmOwner::bytes());
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + shadow_owner_storage::RealmOwner::bytes()
    );
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_neutral_storage_attaches_live_nodes_and_prunes_dead_ones() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(None);
    let dead = owners.commit_owner(owners.prepare_owner(None).unwrap());
    drop(dead);
    assert_eq!(owners.len(), 2);
    owners.attach(&scope).unwrap();
    owners.attach(&scope).unwrap();
    owner.attach(&scope).unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        owners.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_foreign_alias_charges_only_its_receiving_document() {
    let scope = SchedulingScope::new();
    let foreign = SchedulingScope::new();
    let view = scope_view(&scope);
    let foreign_view = scope_view(&foreign);
    let (owners, owner) = owner_list(Some(&scope));
    let before = view.host_scheduling_snapshot().payload_bytes;
    assert!(!owners.can_join(&foreign));
    let second = owners.commit_owner(owners.prepare_owner(Some(&foreign)).unwrap());
    assert_eq!(owners.len(), 2);
    assert_ne!(owner.identity(), second.identity());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        foreign_view.host_scheduling_snapshot().payload_bytes,
        shadow_owner_storage::RealmOwner::bytes()
    );
    drop(second);
    owners.prune();
    assert_eq!(foreign_view.host_scheduling_snapshot().payload_bytes, 0);
    let third = owners.commit_owner(owners.prepare_owner(Some(&scope)).unwrap());
    assert_ne!(owner.identity(), third.identity());
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_neutral_alias_preserves_bound_head_and_existing_owner() {
    let scope = SchedulingScope::new();
    let foreign = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let before = view.host_scheduling_snapshot().payload_bytes;
    let second = owners.commit_owner(owners.prepare_owner(None).unwrap());
    second.attach(&foreign).unwrap();
    assert!(owner.can_join(&scope));
    assert!(!owner.can_join(&foreign));
    assert!(second.can_join(&foreign));
    assert!(owners.can_join(&scope));
    assert!(!owners.can_join(&foreign));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_terminal_list_cleanup_needs_no_admission() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, owner) = owner_list(Some(&scope));
    let before = view.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(scope.fail(), RuntimeError::SchedulingLimit));
    assert!(matches!(
        owners.prepare_owner(Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(owners.len(), 1);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(owner);
    owners.prune();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        owners.retained_bytes()
    );
    drop(owners);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_reverse_index_outlives_vm_without_losing_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let index = std::mem::take(&mut vm.shadow_realm_by_heap);
    let bytes = index.retained_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(index);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_late_activation_attaches_existing_shared_metadata() {
    let mut vm = Vm::default();
    run(
        &mut vm,
        "var realm=new ShadowRealm();var callback=realm.evaluate('()=>42');0;",
    );
    let record = vm.shadow_realms.values().next().unwrap().clone();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap();
    assert!(record.owner_storage_can_join(scope));
    assert_eq!(vm.unowned_membrane_storage_bytes(), 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    record.attach_owner_storage(scope).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(run(&mut vm, "callback();"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_alias_reuses_index_and_adds_one_owner() {
    let mut vm = page();
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let original = record.owner.identity();
    let instance = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let bytes = vm.shadow_realm_by_heap.retained_bytes();
    let (result, peak) =
        scope.measure_payload_peak(|| vm.adopt_shadow_realm(instance, record.clone()));
    result.unwrap();
    assert_eq!(peak, shadow_owner_storage::RealmOwner::bytes());
    assert_eq!(vm.shadow_realm_by_heap.retained_bytes(), bytes);
    assert_eq!(vm.shadow_realm_by_heap.get(&record.heap), Some(&instance));
    assert_eq!(record.owners.len(), 2);
    assert_ne!(vm.shadow_realms[&instance].owner.identity(), original);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_constructor_getter_can_publish_another_realm() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var count=0;var target=new Proxy(function(){},{get:function(t,k,r){if(k==='prototype'){count++;globalThis.inner=new ShadowRealm();return {};}return Reflect.get(t,k,r);}});var outer=Reflect.construct(ShadowRealm,[],target);count;"), Value::Number(1.0));
    assert_eq!(vm.shadow_realms.len(), 3);
    assert_eq!(vm.shadow_realm_by_heap.len(), 3);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_function_metadata_getter_can_publish_another_wrapper() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"var count=0;var other;var make=realm.evaluate('(callback)=>{var f=()=>42;Object.defineProperty(f,"name",{get:function(){callback();return "wrapped";}});return f;}');var callback=make(function(){count++;other=realm.evaluate('()=>7');});callback()+other()+count;"#
        ),
        Value::Number(50.0)
    );
    assert_eq!(vm.shadow_wrapped_functions.len(), 3);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_throwing_metadata_getter_publishes_no_wrapper() {
    let mut vm = page();
    let before = vm.shadow_wrapped_functions.len();
    assert_eq!(
        run(
            &mut vm,
            r#"var threw=false;try{realm.evaluate('(()=>{var f=()=>42;Object.defineProperty(f,"name",{get:function(){throw 7;}});return f;})();');}catch(error){threw=error instanceof TypeError;}threw;"#
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.shadow_wrapped_functions.len(), before);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_foreign_adoption_preserves_both_existing_registries() {
    let mut vm = page();
    let mut foreign = page();
    let record = foreign.shadow_realms.values().next().unwrap().clone();
    let foreign_before = foreign.host_scheduling_snapshot().payload_bytes;
    let instance = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.adopt_shadow_realm(instance, record.clone()).unwrap();
    assert_eq!(vm.shadow_realms.len(), 2);
    assert_eq!(vm.shadow_realm_by_heap.len(), 2);
    assert_eq!(record.owners.len(), 2);
    assert!(vm.shadow_realms[&instance]
        .owner
        .can_join(vm.host_scheduling.as_ref().unwrap()));
    assert_eq!(
        foreign.host_scheduling_snapshot().payload_bytes,
        foreign_before
    );
    assert_eq!(
        run(&mut foreign, "realm.evaluate('42');"),
        Value::Number(42.0)
    );
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_reexport_identity_does_not_publish_duplicate_owners() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(run(&mut vm, "var first=$262.createRealm();var second=$262.createRealm();var shared=first.evalScript('new ShadowRealm();');second.global.a=shared;second.global.b=shared;second.evalScript('a===b;');"), Value::Bool(true));
    let record = vm
        .test262_realms
        .values()
        .find_map(|child| child.vm.borrow().shadow_realms.values().next().cloned())
        .unwrap();
    assert_eq!(record.owners.len(), 2);
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_replacing_instance_removes_the_old_reverse_index() {
    let mut vm = page();
    let key = *vm.shadow_realms.keys().next().unwrap();
    let old = vm.shadow_realms[&key].clone();
    let weak = old.vm.downgrade();
    let instance = run(&mut vm, "var second=new ShadowRealm();second;")
        .object_id()
        .unwrap();
    let record = vm.shadow_realms[&instance].clone();
    vm.adopt_shadow_realm(key, record.clone()).unwrap();
    assert!(!vm.shadow_realm_by_heap.contains_key(&old.heap));
    assert_eq!(vm.shadow_realm_by_heap.get(&record.heap), Some(&key));
    assert_eq!(vm.shadow_realms.len(), 2);
    assert_eq!(vm.shadow_realms[&key].heap, record.heap);
    drop(old);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn shadow_registry_storage_dropping_list_detaches_independently_held_owner_nodes() {
    let scope = SchedulingScope::new();
    let view = scope_view(&scope);
    let (owners, first) = owner_list(Some(&scope));
    let second = owners.commit_owner(owners.prepare_owner(Some(&scope)).unwrap());
    drop(owners);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        2 * shadow_owner_storage::RealmOwner::bytes()
    );
    drop(first);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        shadow_owner_storage::RealmOwner::bytes()
    );
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_shadow_registry_storage_contracts() {
        shadow_registry_storage_instance_table_retains_native_credit();
        shadow_registry_storage_reverse_index_retains_native_credit();
        shadow_registry_storage_callable_table_retains_native_credit();
        shadow_registry_storage_owner_metadata_outlives_all_vm_data();
        shadow_registry_storage_owner_list_counts_shared_header_and_identity_nodes();
        shadow_registry_storage_dead_weak_owner_keeps_credit_until_pruning();
        shadow_registry_storage_clones_share_identity_and_native_credit();
        shadow_registry_storage_prepared_owner_is_fully_charged_before_publication();
        shadow_registry_storage_abandoned_owner_preparation_publishes_nothing();
        shadow_registry_storage_append_charges_only_one_complete_node();
        shadow_registry_storage_neutral_storage_attaches_live_nodes_and_prunes_dead_ones();
        shadow_registry_storage_foreign_alias_charges_only_its_receiving_document();
        shadow_registry_storage_neutral_alias_preserves_bound_head_and_existing_owner();
        shadow_registry_storage_terminal_list_cleanup_needs_no_admission();
        shadow_registry_storage_reverse_index_outlives_vm_without_losing_credit();
        shadow_registry_storage_late_activation_attaches_existing_shared_metadata();
        shadow_registry_storage_alias_reuses_index_and_adds_one_owner();
        shadow_registry_storage_constructor_getter_can_publish_another_realm();
        shadow_registry_storage_function_metadata_getter_can_publish_another_wrapper();
        shadow_registry_storage_throwing_metadata_getter_publishes_no_wrapper();
        shadow_registry_storage_foreign_adoption_preserves_both_existing_registries();
        shadow_registry_storage_reexport_identity_does_not_publish_duplicate_owners();
        shadow_registry_storage_replacing_instance_removes_the_old_reverse_index();
        shadow_registry_storage_dropping_list_detaches_independently_held_owner_nodes();
    }
}
