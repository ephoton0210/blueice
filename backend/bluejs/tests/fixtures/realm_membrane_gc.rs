// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny membrane graphs exercise public liveness, without pressure or services.

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
fn child(vm: &Vm) -> realm_cells::RealmHandle {
    vm.test262_realms.values().next().unwrap().vm.clone()
}
fn collect(vm: &mut Vm) {
    vm.run_host_microtask_checkpoint().unwrap();
    vm.collect_realm_garbage().unwrap();
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_dead_forward_facade_releases_both_heap_roots() {
    let mut root = page();
    run(&mut root, "var foreign=realm.evalScript('var weak;(()=>{var object={value:7};weak=new WeakRef(object);return object;})();');var localWeak=new WeakRef(foreign);realm.evalScript('0;');foreign=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "localWeak.deref()===undefined;"),
        Value::Bool(true)
    );
    assert_eq!(
        run(&mut child(&root).borrow_mut(), "weak.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_dead_reverse_facade_releases_the_original_object() {
    let mut root = page();
    run(&mut root, "var local={value:7};var weak=new WeakRef(local);realm.global.value=local;realm.evalScript('delete globalThis.value;0;');0;");
    let source = root
        .lookup_global_name("local")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    run(&mut root, "local=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
    assert!(!child(&root)
        .borrow()
        .test262_reverse_values
        .values()
        .any(|value| value.target == source));
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_dead_typed_array_snapshot_releases_transport_identity() {
    let mut root = page();
    run(&mut root, "var local=new Uint8Array([7,8]);var weak=new WeakRef(local);realm.global.value=local;realm.evalScript('delete globalThis.value;0;');0;");
    let source = root
        .lookup_global_name("local")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    run(&mut root, "local=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
    let realm = root.test262_realms.values().next().unwrap();
    assert!(!realm.imported_sources.contains_key(&source));
    assert!(!realm
        .imported_values
        .values()
        .any(|value| value.value.object_id() == Some(source)));
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_dead_buffer_mirror_releases_its_backing_object() {
    let mut root = page();
    run(&mut root, "var foreign=realm.evalScript('new ArrayBuffer(2);');var local=new Uint8Array(foreign);var weak=new WeakRef(local.buffer);realm.evalScript('0;');local=null;foreign=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
    assert!(root.test262_foreign_buffer_mirrors.is_empty());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_unreachable_forward_reverse_cycle_has_no_persistent_owner() {
    let mut root = page();
    run(&mut root, "var local={};var foreign=realm.evalScript('var childWeak;globalThis.object={};childWeak=new WeakRef(object);object;');local.child=foreign;foreign.parent=local;var weak=new WeakRef(local);realm.evalScript('delete globalThis.object;0;');local=null;foreign=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
    assert_eq!(
        run(
            &mut child(&root).borrow_mut(),
            "childWeak.deref()===undefined;"
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_dead_forward_callable_releases_its_shadow_home() {
    let mut root = page();
    run(&mut root, "var callback=realm.evalScript(\"var nested=new ShadowRealm();nested.evaluate('()=>42');\");0;");
    let weak = child(&root)
        .borrow()
        .shadow_realms
        .values()
        .next()
        .unwrap()
        .vm
        .downgrade();
    run(
        &mut root,
        "realm.evalScript('nested=null;0;');callback=null;0;",
    );
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_discarded_owned_realm_releases_its_vm() {
    let mut root = page();
    let weak = child(&root).downgrade();
    run(&mut root, "realm=null;0;");
    collect(&mut root);
    assert!(weak.upgrade().is_none());
    assert!(root.test262_realms.is_empty());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_live_foreign_callable_keeps_its_owned_home_then_releases_it() {
    let mut root = page();
    run(
        &mut root,
        "var callback=realm.evalScript('()=>42;');realm.evalScript('0;');realm=null;0;",
    );
    let weak = child(&root).downgrade();
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    assert_eq!(run(&mut root, "callback();"), Value::Number(42.0));
    run(&mut root, "callback=null;0;");
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_live_reverse_facade_preserves_call_and_round_trip_identity() {
    let mut root = page();
    run(&mut root, "var callback=()=>42;var weak=new WeakRef(callback);realm.global.callback=callback;callback=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "realm.evalScript('callback()');"),
        Value::Number(42.0)
    );
    assert_eq!(
        run(&mut root, "realm.evalScript('callback')===weak.deref();"),
        Value::Bool(true)
    );
    run(
        &mut root,
        "realm.evalScript('delete globalThis.callback;0;');0;",
    );
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_live_snapshot_preserves_original_identity_then_releases_it() {
    let mut root = page();
    run(&mut root, "var local=new Uint8Array([7,8]);var weak=new WeakRef(local);realm.global.value=local;local=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "realm.evalScript('value')===weak.deref();"),
        Value::Bool(true)
    );
    run(
        &mut root,
        "realm.evalScript('delete globalThis.value;0;');0;",
    );
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_live_mirror_keeps_its_facade_bytes_and_owned_vm() {
    let mut root = page();
    run(&mut root, "var foreign=realm.evalScript('new ArrayBuffer(2);');var local=new Uint8Array(foreign);realm.evalScript('0;');local[0]=7;local[1]=8;foreign=null;realm=null;0;");
    let weak = child(&root).downgrade();
    collect(&mut root);
    assert!(weak.upgrade().is_some());
    assert_eq!(run(&mut root, "var copy=new Uint8Array(ArrayBuffer.prototype.slice.call(local.buffer,0,2));copy[0]===7&&copy[1]===8;"), Value::Bool(true));
    run(&mut root, "local=null;copy=null;0;");
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_live_prototype_override_is_a_conditional_local_edge() {
    let mut root = page();
    run(&mut root, "var foreign=realm.global.Date;var target=function(){};target.prototype={answer:42};var value=Reflect.construct(foreign,[0],target);var weak=new WeakRef(target.prototype);target=null;realm.evalScript('0;');0;");
    let facade = root
        .lookup_global_name("value")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    assert!(root.test262_foreign_values[&facade]
        .prototype_override
        .is_some());
    collect(&mut root);
    assert_eq!(
        run(
            &mut root,
            "Object.getPrototypeOf(value)===weak.deref()&&Object.getPrototypeOf(value).answer===42;"
        ),
        Value::Bool(true)
    );
    run(&mut root, "value=null;0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_busy_member_defers_pair_release_until_full_discovery() {
    let mut root = page();
    run(
        &mut root,
        "var foreign=realm.evalScript('({answer:42})');realm.evalScript('0;');0;",
    );
    let wrapper = root
        .lookup_global_name("foreign")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    root.run_host_microtask_checkpoint().unwrap();
    let home = child(&root);
    {
        let _held = home.borrow_mut();
        run(&mut root, "foreign=null;0;");
        root.collect_realm_garbage().unwrap();
        assert!(root.heap.contains(wrapper));
        assert!(root.test262_foreign_values.contains_key(&wrapper));
    }
    drop(home);
    collect(&mut root);
    assert!(!root.heap.contains(wrapper));
    assert!(!root.test262_foreign_values.contains_key(&wrapper));
}

fn orphaned_owned_work(
    root: &mut Vm,
    source: &str,
) -> (
    realm_cells::RealmWeak,
    realm_cells::RealmWeak,
    Rc<RefCell<Vec<HostValue>>>,
) {
    run(root, "var outer=new ShadowRealm();0;");
    let id = root
        .lookup_global_name("outer")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let outer = root.shadow_realms[&id].clone();
    let outer_weak = outer.vm.downgrade();
    let log = Rc::new(RefCell::new(Vec::new()));
    let inner_weak;
    {
        let mut vm = outer.vm.borrow_mut();
        vm.install_test262_harness().unwrap();
        run(&mut vm, "var realm=$262.createRealm();0;");
        let home = child(&vm);
        inner_weak = home.downgrade();
        let mut inner = home.borrow_mut();
        run(&mut inner, "delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
        inner.install_host_timer_functions().unwrap();
        let values = log.clone();
        inner
            .install_host_function("record", 1, move |args: &[HostValue]| {
                values.borrow_mut().push(args[0].clone());
                Ok(HostValue::Undefined)
            })
            .unwrap();
        run(&mut inner, source);
    }
    drop(outer);
    run(root, "outer=null;0;");
    (outer_weak, inner_weak, log)
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_owned_timer_retains_discarded_creator_only_until_delivery() {
    let mut root = page();
    let (outer, inner, log) = orphaned_owned_work(&mut root, "setTimeout(()=>record(7),1);0;");
    collect(&mut root);
    assert!(outer.upgrade().is_some() && inner.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::Number(7.0)]);
    collect(&mut root);
    assert!(outer.upgrade().is_none() && inner.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_owned_promise_retains_discarded_creator_only_until_checkpoint() {
    let mut root = page();
    let (outer, inner, log) =
        orphaned_owned_work(&mut root, "Promise.resolve().then(()=>record(7));0;");
    root.collect_realm_garbage().unwrap();
    assert!(outer.upgrade().is_some() && inner.upgrade().is_some());
    assert_eq!(root.run_host_microtask_checkpoint().unwrap(), 0);
    assert_eq!(*log.borrow(), [HostValue::Number(7.0)]);
    collect(&mut root);
    assert!(outer.upgrade().is_none() && inner.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_owned_child_jobs_survive_losing_the_public_global_facade() {
    let mut root = page();
    run(&mut root, "var seen=0;realm.global.record=()=>{seen++;};realm.evalScript('Promise.resolve().then(()=>record());0;');realm=null;0;");
    let weak = child(&root).downgrade();
    root.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_some());
    root.run_host_microtask_checkpoint().unwrap();
    assert_eq!(run(&mut root, "seen;"), Value::Number(1.0));
    collect(&mut root);
    assert!(weak.upgrade().is_none());
}

fn shared_descendant_work(
    root: &mut Vm,
    owned_parent: bool,
    source: &str,
) -> (
    realm_cells::RealmWeak,
    realm_cells::RealmWeak,
    Rc<RefCell<Vec<HostValue>>>,
) {
    let parent = if owned_parent {
        child(root)
    } else {
        run(root, "var outer=new ShadowRealm();0;");
        let id = root
            .lookup_global_name("outer")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        root.shadow_realms[&id].vm.clone()
    };
    let parent_weak = parent.downgrade();
    let log = Rc::new(RefCell::new(Vec::new()));
    let leaf_weak;
    {
        let mut vm = parent.borrow_mut();
        run(&mut vm, "var nested=new ShadowRealm();0;");
        let leaf = vm.shadow_realms.values().next().unwrap().clone();
        leaf_weak = leaf.vm.downgrade();
        let mut inner = leaf.vm.borrow_mut();
        inner.install_host_timer_functions().unwrap();
        let values = log.clone();
        inner
            .install_host_function("record", 1, move |args: &[HostValue]| {
                values.borrow_mut().push(args[0].clone());
                Ok(HostValue::Undefined)
            })
            .unwrap();
        run(&mut inner, source);
    }
    drop(parent);
    run(
        root,
        if owned_parent {
            "realm=null;0;"
        } else {
            "outer=null;0;"
        },
    );
    (parent_weak, leaf_weak, log)
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_shared_descendant_timer_retains_its_owned_access_route() {
    let mut root = page();
    let (parent, leaf, log) =
        shared_descendant_work(&mut root, true, "setTimeout(()=>record(7),1);0;");
    root.collect_realm_garbage().unwrap();
    assert!(parent.upgrade().is_some() && leaf.upgrade().is_some());
    assert_eq!(root.run_host_task_turn(1, 1).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::Number(7.0)]);
    collect(&mut root);
    assert!(parent.upgrade().is_none() && leaf.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_shared_descendant_promise_retains_its_shared_access_route() {
    let mut root = page();
    let (parent, leaf, log) =
        shared_descendant_work(&mut root, false, "Promise.resolve().then(()=>record(7));0;");
    root.collect_realm_garbage().unwrap();
    assert!(parent.upgrade().is_some() && leaf.upgrade().is_some());
    assert_eq!(root.run_host_microtask_checkpoint().unwrap(), 0);
    assert_eq!(*log.borrow(), [HostValue::Number(7.0)]);
    collect(&mut root);
    assert!(parent.upgrade().is_none() && leaf.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_snapshot_charge_covers_every_pin_and_edge_buffer() {
    let mut root = page();
    run(&mut root, "var local={};realm.global.value=local;var foreign=realm.evalScript('({answer:42})');realm.evalScript('0;');0;");
    let plan = root.prepare_realm_collection().unwrap().unwrap();
    assert!(!plan.pins.is_empty());
    assert_eq!(
        super::ownership_tests::metadata_bytes(&plan),
        plan.retained_bytes()
    );
    root.recycle_realm_collection(plan);
    let plan = root.realm_collection_cache.take().unwrap();
    let bytes = plan.retained_bytes();
    let native = root.host_scheduling_snapshot().payload_bytes;
    drop(plan);
    assert_eq!(
        root.host_scheduling_snapshot().payload_bytes,
        native - bytes
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_terminal_cleanup_releases_dead_pairs_using_cached_storage() {
    let mut root = page();
    run(
        &mut root,
        "var foreign=realm.evalScript('({answer:42})');realm.evalScript('0;');0;",
    );
    let wrapper = root
        .lookup_global_name("foreign")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    collect(&mut root);
    run(&mut root, "foreign=null;0;");
    root.run_host_microtask_checkpoint().unwrap();
    root.host_scheduling.as_ref().unwrap().fail();
    root.collect_realm_garbage().unwrap();
    assert!(!root.heap.contains(wrapper));
    assert!(!root.test262_foreign_values.contains_key(&wrapper));
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_constructor_result_survives_a_collecting_prototype_getter() {
    let mut root = page();
    run(&mut root, "var prototype={answer:42};var target=new Proxy(function(){},{get(object,key){if(key==='prototype'){new ShadowRealm();return prototype;}return Reflect.get(object,key);}});var value=Reflect.construct(realm.global.Date,[0],target);0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "Object.getPrototypeOf(value)===prototype;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_constructor_result_releases_its_temporary_root_after_throw() {
    let mut root = page();
    run(&mut root, "var caught=false;var target=new Proxy(function(){},{get(object,key){if(key==='prototype'){new ShadowRealm();throw 7;}return Reflect.get(object,key);}});try{Reflect.construct(realm.global.Date,[0],target);}catch(error){caught=error===7;}0;");
    assert_eq!(run(&mut root, "caught;"), Value::Bool(true));
    collect(&mut root);
    let home = child(&root);
    let vm = home.borrow();
    assert!(!root
        .test262_foreign_values
        .values()
        .any(|value| vm.heap.date_value(value.target).is_ok()));
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_identity_live_foreign_target_preserves_the_weak_facade() {
    let mut root = page();
    run(&mut root, "var foreign=realm.evalScript('globalThis.keep={answer:42};keep;');var weak=new WeakRef(foreign);realm.evalScript('0;');foreign=null;0;");
    collect(&mut root);
    assert_eq!(
        run(
            &mut root,
            "weak.deref()!==undefined&&realm.evalScript('keep')===weak.deref();"
        ),
        Value::Bool(true)
    );
    run(
        &mut root,
        "realm.evalScript('delete globalThis.keep;0;');0;",
    );
    collect(&mut root);
    assert_eq!(
        run(&mut root, "weak.deref()===undefined;"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_identity_live_original_preserves_the_weak_transport() {
    let mut root = page();
    run(&mut root, "var local={answer:42};realm.global.value=local;realm.evalScript('var weak=new WeakRef(value);delete globalThis.value;0;');0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "realm.evalScript('weak.deref()!==undefined');"),
        Value::Bool(true)
    );
    assert_eq!(
        run(&mut root, "realm.evalScript('weak.deref()')===local;"),
        Value::Bool(true)
    );
    run(&mut root, "local=null;realm.evalScript('0;');0;");
    collect(&mut root);
    assert_eq!(
        run(&mut root, "realm.evalScript('weak.deref()===undefined');"),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_identity_cache_does_not_activate_a_discarded_destination() {
    let mut root = page();
    run(
        &mut root,
        "var local={answer:42};realm.global.value=local;0;",
    );
    let weak = child(&root).downgrade();
    run(&mut root, "realm=null;0;");
    collect(&mut root);
    assert!(weak.upgrade().is_none());
    assert_eq!(run(&mut root, "local.answer;"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn realm_membrane_gc_kept_target_preserves_its_vm_until_the_document_checkpoint() {
    let mut root = page();
    run(&mut root, "var foreign=realm.evalScript('var object={answer:42};var weak=new WeakRef(object);object;');var weakRoot=new WeakRef(foreign);realm.evalScript('0;');foreign=null;0;");
    root.run_host_microtask_checkpoint().unwrap();
    let weak = child(&root).downgrade();
    run(
        &mut root,
        "realm.evalScript('weak.deref();object=null;0;');realm=null;0;",
    );
    root.collect_realm_garbage().unwrap();
    assert!(weak.upgrade().is_some());
    assert_eq!(
        run(&mut root, "weakRoot.deref()!==undefined;"),
        Value::Bool(true)
    );
    collect(&mut root);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        run(&mut root, "weakRoot.deref()===undefined;"),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_realm_membrane_gc_contracts() {
        realm_membrane_gc_dead_forward_facade_releases_both_heap_roots();
        realm_membrane_gc_dead_reverse_facade_releases_the_original_object();
        realm_membrane_gc_dead_typed_array_snapshot_releases_transport_identity();
        realm_membrane_gc_dead_buffer_mirror_releases_its_backing_object();
        realm_membrane_gc_unreachable_forward_reverse_cycle_has_no_persistent_owner();
        realm_membrane_gc_dead_forward_callable_releases_its_shadow_home();
        realm_membrane_gc_discarded_owned_realm_releases_its_vm();
        realm_membrane_gc_live_foreign_callable_keeps_its_owned_home_then_releases_it();
        realm_membrane_gc_live_reverse_facade_preserves_call_and_round_trip_identity();
        realm_membrane_gc_live_snapshot_preserves_original_identity_then_releases_it();
        realm_membrane_gc_live_mirror_keeps_its_facade_bytes_and_owned_vm();
        realm_membrane_gc_live_prototype_override_is_a_conditional_local_edge();
        realm_membrane_gc_busy_member_defers_pair_release_until_full_discovery();
        realm_membrane_gc_owned_timer_retains_discarded_creator_only_until_delivery();
        realm_membrane_gc_owned_promise_retains_discarded_creator_only_until_checkpoint();
        realm_membrane_gc_owned_child_jobs_survive_losing_the_public_global_facade();
        realm_membrane_gc_shared_descendant_timer_retains_its_owned_access_route();
        realm_membrane_gc_shared_descendant_promise_retains_its_shared_access_route();
        realm_membrane_gc_snapshot_charge_covers_every_pin_and_edge_buffer();
        realm_membrane_gc_terminal_cleanup_releases_dead_pairs_using_cached_storage();
        realm_membrane_gc_constructor_result_survives_a_collecting_prototype_getter();
        realm_membrane_gc_constructor_result_releases_its_temporary_root_after_throw();
        realm_membrane_gc_identity_live_foreign_target_preserves_the_weak_facade();
        realm_membrane_gc_identity_live_original_preserves_the_weak_transport();
        realm_membrane_gc_identity_cache_does_not_activate_a_discarded_destination();
        realm_membrane_gc_kept_target_preserves_its_vm_until_the_document_checkpoint();
    }
}
