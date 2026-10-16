// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny immutable host sources have one owner across attached realms.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

fn string(capacity: usize, value: &str) -> String {
    let mut result = String::with_capacity(capacity);
    result.push_str(value);
    result
}

#[cfg_attr(test, test)]
fn module_source_ownership_accounts_for_all_raw_source_kinds() {
    for kind in 0..5 {
        let mut vm = bounded_vm();
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let key = string(32, "page/value.js");
        let text = string(64, "tiny");
        let expected = if kind == 4 {
            let mut names = HashSet::new();
            names.insert(key.clone());
            names.capacity() * std::mem::size_of::<String>() + key.capacity()
        } else {
            let mut sources = HashMap::new();
            sources.insert(key.clone(), text.clone());
            sources.capacity() * std::mem::size_of::<(String, String)>()
                + key.capacity()
                + text.capacity()
        };
        match kind {
            0 => vm.set_dynamic_module_sources(HashMap::from([(key, text)])),
            1 => vm.set_json_module_sources(HashMap::from([(key, text)])),
            2 => vm.set_text_module_sources(HashMap::from([(key, text)])),
            3 => {
                let mut bytes = Vec::with_capacity(64);
                bytes.extend([1, 2, 3]);
                vm.set_bytes_module_sources(HashMap::from([(key, bytes)]));
            }
            _ => vm.set_module_source_loader_context(vec![key]),
        }
        assert!(
            vm.host_scheduling_snapshot().payload_bytes - before >= expected,
            "kind={kind}: raw source capacity was uncharged"
        );
    }
}

#[cfg_attr(test, test)]
fn module_source_ownership_replacement_refusal_keeps_old_source_and_latches_document() {
    let mut vm = bounded_vm();
    vm.set_text_module_sources(HashMap::from([("kept".into(), "old".into())]));
    let pointer = vm.text_module_sources.get("kept").unwrap().as_ptr();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.set_text_module_sources(HashMap::from([("new".into(), "tiny".into())]));
    assert_eq!(
        vm.text_module_sources.get("kept").map(String::as_str),
        Some("old")
    );
    assert_eq!(
        vm.text_module_sources.get("kept").unwrap().as_ptr(),
        pointer
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_source_ownership_shadow_import_shares_raw_source_buffers() {
    let mut vm = bounded_vm();
    let realm = vm
        .execute_script(&crate::compile(&crate::parse("new ShadowRealm();").unwrap()).unwrap())
        .unwrap();
    vm.set_module_loader_context("page/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "page/value.js".into(),
        "export function answer(){return 42;}".into(),
    )]));
    let pointer = vm
        .dynamic_module_sources
        .get("page/value.js")
        .unwrap()
        .as_ptr();
    vm.shadow_realm_import_value(
        realm.clone(),
        Value::String("./value.js".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    let record = vm
        .shadow_realms
        .get(&realm.object_id().unwrap())
        .unwrap()
        .clone();
    let child = record.vm.borrow();
    assert_eq!(
        child
            .dynamic_module_sources
            .get("page/value.js")
            .unwrap()
            .as_ptr(),
        pointer
    );
}

fn install(vm: &mut Vm, kind: usize) {
    let key = string(32, "page/value.js");
    let text = string(64, "tiny");
    match kind {
        0 => vm.set_dynamic_module_sources(HashMap::from([(key, text)])),
        1 => vm.set_json_module_sources(HashMap::from([(key, text)])),
        2 => vm.set_text_module_sources(HashMap::from([(key, text)])),
        3 => {
            let mut bytes = Vec::with_capacity(64);
            bytes.extend([1, 2, 3]);
            vm.set_bytes_module_sources(HashMap::from([(key, bytes)]));
        }
        _ => vm.set_module_source_loader_context(vec![key]),
    }
}

fn retained(vm: &Vm, kind: usize) -> usize {
    match kind {
        0 => vm.dynamic_module_sources.retained_bytes(),
        1 => vm.json_module_sources.retained_bytes(),
        2 => vm.text_module_sources.retained_bytes(),
        3 => vm.bytes_module_sources.retained_bytes(),
        _ => vm.module_source_registry.retained_bytes(),
    }
}

fn clear(vm: &mut Vm) {
    vm.set_dynamic_module_sources(HashMap::new());
    vm.set_json_module_sources(HashMap::new());
    vm.set_text_module_sources(HashMap::new());
    vm.set_bytes_module_sources(HashMap::new());
    vm.set_module_source_loader_context(Vec::new());
}

#[cfg_attr(test, test)]
fn module_source_ownership_exact_install_peak_and_one_byte_refusal() {
    for kind in 0..5 {
        let mut measured = Vm::default();
        install(&mut measured, kind);
        let owner = retained(&measured, kind);
        let peak = owner
            + if kind == 4 {
                std::mem::size_of::<String>()
            } else {
                0
            };
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            let _peer = leave_room(&mut vm, peak - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            install(&mut vm, kind);
            if accepts {
                assert_eq!(retained(&vm, kind), owner);
                assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + owner);
                assert_eq!(vm.check_scheduling_failure(), Ok(()));
            } else {
                assert_eq!(retained(&vm, kind), 0);
                assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
                assert_eq!(
                    vm.check_scheduling_failure(),
                    Err(RuntimeError::SchedulingLimit)
                );
            }
        }
    }
}

#[cfg_attr(test, test)]
fn module_source_ownership_shared_snapshots_need_no_new_budget_and_refund_last_drop() {
    let mut vm = bounded_vm();
    for kind in 0..5 {
        install(&mut vm, kind);
    }
    let bytes = (0..5).map(|kind| retained(&vm, kind)).sum::<usize>();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshots = (
        vm.dynamic_module_sources.clone(),
        vm.json_module_sources.clone(),
        vm.text_module_sources.clone(),
        vm.bytes_module_sources.clone(),
        vm.module_source_registry.clone(),
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    clear(&mut vm);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        snapshots.0.get("page/value.js").map(String::as_str),
        Some("tiny")
    );
    drop(snapshots);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
}

#[cfg_attr(test, test)]
fn module_source_ownership_preloaded_sources_attach_once_when_bounded_mode_starts() {
    let mut vm = Vm::default();
    for kind in 0..5 {
        install(&mut vm, kind);
    }
    let bytes = (0..5).map(|kind| retained(&vm, kind)).sum::<usize>();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes + bytes
    );
    let before = vm.host_scheduling_snapshot();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot(), before);
}

#[cfg_attr(test, test)]
fn module_source_ownership_source_names_deduplicate_without_charging_removed_names() {
    let mut vm = bounded_vm();
    vm.set_module_source_loader_context(vec![string(32, "b"), string(32, "a"), string(64, "a")]);
    assert_eq!(vm.module_source_registry.len(), 2);
    assert!(vm.module_source_registry.contains("a") && vm.module_source_registry.contains("b"));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.module_source_registry.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_source_ownership_cleanup_refunds_sources_after_terminal_failure() {
    let mut vm = bounded_vm();
    for kind in 0..5 {
        install(&mut vm, kind);
    }
    assert!(vm.host_scheduling_snapshot().payload_bytes > 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    clear(&mut vm);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_source_ownership_another_document_cannot_adopt_the_existing_charge() {
    let mut owner = bounded_vm();
    install(&mut owner, 0);
    let bytes = owner.host_scheduling_snapshot().payload_bytes;
    let mut other = bounded_vm();
    other.dynamic_module_sources = owner.dynamic_module_sources.clone();
    assert!(matches!(
        other.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        other.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(owner.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(owner.check_scheduling_failure(), Ok(()));
    clear(&mut other);
    assert_eq!(owner.host_scheduling_snapshot().payload_bytes, bytes);
}

#[cfg_attr(test, test)]
fn module_source_ownership_parent_replacement_preserves_the_child_snapshot_until_refresh() {
    let mut vm = bounded_vm();
    let realm = vm
        .execute_script(&crate::compile(&crate::parse("new ShadowRealm();").unwrap()).unwrap())
        .unwrap();
    vm.set_module_loader_context("page/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "page/value.js".into(),
        "export function answer(){return 1;}".into(),
    )]));
    vm.shadow_realm_import_value(
        realm.clone(),
        Value::String("./value.js".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    let old = vm
        .dynamic_module_sources
        .get("page/value.js")
        .unwrap()
        .as_ptr();
    let record = vm
        .shadow_realms
        .get(&realm.object_id().unwrap())
        .unwrap()
        .clone();
    vm.set_dynamic_module_sources(HashMap::from([(
        "page/value.js".into(),
        "export function answer(){return 2;}".into(),
    )]));
    let new = vm
        .dynamic_module_sources
        .get("page/value.js")
        .unwrap()
        .as_ptr();
    assert_ne!(old, new);
    assert_eq!(
        record
            .vm
            .borrow()
            .dynamic_module_sources
            .get("page/value.js")
            .unwrap()
            .as_ptr(),
        old
    );
    vm.shadow_realm_import_value(
        realm,
        Value::String("./value.js".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    assert_eq!(
        record
            .vm
            .borrow()
            .dynamic_module_sources
            .get("page/value.js")
            .unwrap()
            .as_ptr(),
        new
    );
}

#[cfg_attr(test, test)]
fn module_source_ownership_discarded_last_realm_releases_its_source_snapshot() {
    let mut vm = bounded_vm();
    let realm = vm
        .execute_script(&crate::compile(&crate::parse("new ShadowRealm();").unwrap()).unwrap())
        .unwrap();
    vm.set_module_loader_context("page/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "page/value.js".into(),
        "export function answer(){return 42;}".into(),
    )]));
    vm.shadow_realm_import_value(
        realm,
        Value::String("./value.js".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    vm.set_dynamic_module_sources(HashMap::new());
    assert!(vm.host_scheduling_snapshot().payload_bytes > vm.own_scheduling_usage().payload_bytes);
    vm.execute_script(&crate::compile(&crate::parse("0;").unwrap()).unwrap())
        .unwrap();
    assert!(vm.shadow_realms.is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes + vm.heap.collector_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_source_ownership_empty_reserved_tables_stay_charged_until_the_last_owner() {
    let mut vm = bounded_vm();
    vm.set_text_module_sources(HashMap::with_capacity(9));
    let bytes = vm.text_module_sources.retained_bytes();
    assert!(bytes > 0);
    let copy = vm.text_module_sources.clone();
    assert_eq!(
        &*copy as *const HashMap<String, String>,
        &*vm.text_module_sources as *const HashMap<String, String>
    );
    vm.set_text_module_sources(HashMap::new());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copy);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_source_ownership_real_typed_and_source_phase_imports_keep_their_values() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("page/main.js", HashMap::new());
    vm.set_json_module_sources(HashMap::from([("page/value.json".into(), "42".into())]));
    vm.set_text_module_sources(HashMap::from([("page/value.txt".into(), "blue".into())]));
    vm.set_bytes_module_sources(HashMap::from([("page/value.bin".into(), vec![4, 2])]));
    vm.set_module_source_loader_context(vec!["page/opaque.src".into()]);
    let source="var seen=false;Promise.all([import('./value.json',{with:{type:'json'}}),import('./value.txt',{with:{type:'text'}}),import('./value.bin',{with:{type:'bytes'}}),import.source('./opaque.src')]).then(([j,t,b,s])=>seen=j.default===42&&t.default==='blue'&&b.default[0]===4&&b.default[1]===2&&typeof s==='object');0;";
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.lookup_global_name("seen").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

impl crate::Vm {
    /// Runs finite immutable module-source contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_module_source_ownership_contracts() {
        module_source_ownership_accounts_for_all_raw_source_kinds();
        module_source_ownership_replacement_refusal_keeps_old_source_and_latches_document();
        module_source_ownership_shadow_import_shares_raw_source_buffers();
        module_source_ownership_exact_install_peak_and_one_byte_refusal();
        module_source_ownership_shared_snapshots_need_no_new_budget_and_refund_last_drop();
        module_source_ownership_preloaded_sources_attach_once_when_bounded_mode_starts();
        module_source_ownership_source_names_deduplicate_without_charging_removed_names();
        module_source_ownership_cleanup_refunds_sources_after_terminal_failure();
        module_source_ownership_another_document_cannot_adopt_the_existing_charge();
        module_source_ownership_parent_replacement_preserves_the_child_snapshot_until_refresh();
        module_source_ownership_discarded_last_realm_releases_its_source_snapshot();
        module_source_ownership_empty_reserved_tables_stay_charged_until_the_last_owner();
        module_source_ownership_real_typed_and_source_phase_imports_keep_their_values();
    }
}
