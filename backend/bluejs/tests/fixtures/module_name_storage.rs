// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny module requests exercise admission before producer allocation.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn fixture(deferred: bool) -> (Vm, ModuleRegistry) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let script =
        crate::compile(&crate::parse("var gate=Promise.withResolvers();").unwrap()).unwrap();
    vm.execute_script(&script).unwrap();
    let parent = if deferred {
        "import defer * as ns from './leaf.mjs'; export const value=1;"
    } else {
        "import './leaf.mjs'; export const value=1;"
    };
    let modules = HashMap::from([
        ("test/root.mjs".into(), compile(parent)),
        ("test/leaf.mjs".into(), compile("await gate.promise;")),
    ]);
    vm.execute_module_graph("test/leaf.mjs", &modules).unwrap();
    let registry = vm.module_registry.clone();
    (vm, registry)
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
#[cfg(test)]
fn refuse_without_scratch(deferred: bool) {
    let (mut vm, registry) = fixture(deferred);
    let heap = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.with_module_records(|vm, linked| {
            vm.evaluate_module_record("test/root.mjs", &registry, linked)
        })
        .unwrap()
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0, "request admission must precede scratch");
    assert_eq!(vm.heap.stats(), heap);
    let parent = vm.linked_record("test/root.mjs").unwrap();
    assert!(!parent.evaluating && !parent.suspended && !parent.evaluated);
    assert!(vm.module_pending_dependencies.is_empty());
    assert!(vm.module_async_parents.is_empty());
}
#[cfg(test)]
#[test]
fn module_name_storage_eager_refusal_precedes_request_and_pending_allocation() {
    refuse_without_scratch(false);
}
#[cfg(test)]
#[test]
fn module_name_storage_deferred_refusal_precedes_request_and_frontier_allocation() {
    refuse_without_scratch(true);
}

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn names(scope: Option<&SchedulingScope>, values: &[&str]) -> ModuleNames {
    let mut result = ModuleNames::default();
    for name in values {
        result.push_copy(name, scope).unwrap();
    }
    result
}
#[cfg_attr(test, test)]
fn module_name_storage_resolution_preserves_paths_types_and_errors() {
    for (referrer, request) in [
        ("main", "./a"),
        ("main", "../a"),
        ("a/b/c", "../d/./e"),
        ("a//b", "./c"),
        ("/a", "./b"),
        ("/a", "../b"),
        ("a/", "./b"),
        ("", "./"),
        ("x/y", "././"),
        ("a/b", "/absolute"),
        ("a/b", "bare/../name"),
        ("a/b", "./中文/../leaf"),
        ("a/b", "bad\0key"),
    ] {
        for kind in [
            ModuleType::JavaScript,
            ModuleType::Json,
            ModuleType::Text,
            ModuleType::Bytes,
        ] {
            assert_eq!(
                ModuleName::resolve(referrer, request, kind, None).map(ModuleName::into_unleased),
                Vm::resolve_module_target(referrer, request, kind),
                "{referrer:?} / {request:?} / {kind:?}",
            );
        }
    }
}
#[cfg_attr(test, test)]
fn module_name_storage_resolution_owns_one_reported_work_buffer() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap();
    #[cfg(test)]
    let value = {
        let (result, allocations) = allocation_observer::count(|| {
            ModuleName::resolve("test/root", "./a/../leaf", ModuleType::Json, Some(scope))
        });
        assert_eq!(allocations, 1);
        result.unwrap()
    };
    #[cfg(not(test))]
    let value =
        ModuleName::resolve("test/root", "./a/../leaf", ModuleType::Json, Some(scope)).unwrap();
    assert_eq!(value.as_str(), "test/leaf\0json");
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + value.bytes()
    );
    drop(value);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn module_name_storage_request_minimum_refuses_before_allocation() {
    let mut vm = page();
    let requested = "test".len() + "./leaf".len() + 1 + "\0json".len();
    let _peer = leave_room(&mut vm, requested - 1);
    let used = vm.host_scheduling_snapshot().payload_bytes;
    #[cfg(test)]
    let result = {
        let (result, allocations) = allocation_observer::count(|| {
            ModuleName::resolve(
                "test/root",
                "./leaf",
                ModuleType::Json,
                vm.host_scheduling.as_ref(),
            )
        });
        assert_eq!(allocations, 0);
        result
    };
    #[cfg(not(test))]
    let result = ModuleName::resolve(
        "test/root",
        "./leaf",
        ModuleType::Json,
        vm.host_scheduling.as_ref(),
    );
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
}
#[cfg_attr(test, test)]
fn module_name_storage_growth_requires_the_complete_candidate() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a", "b", "c", "d"]);
    let before = values.as_slice().to_owned();
    let pointer = values.as_ptr();
    let capacity = values.capacity();
    let value = ModuleName::copy("e", Some(&scope)).unwrap();
    let payload = value.bytes();
    let _peer = leave_room(&mut vm, 2 * capacity * std::mem::size_of::<String>() - 1);
    let used = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        values.push_owned(value, Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(values.as_slice(), before);
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.capacity(), capacity);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used - payload);
}
#[cfg_attr(test, test)]
fn module_name_storage_exact_growth_moves_and_adopts_existing_owners() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a", "b", "c", "d"]);
    let original = values[0].as_ptr();
    let slots = host_scheduling::slots(&values);
    let value = ModuleName::copy("e", Some(&scope)).unwrap();
    let incoming = value.as_ptr();
    let _peer = leave_room(&mut vm, 2 * slots);
    values.push_owned(value, Some(&scope)).unwrap();
    assert_eq!(values.capacity(), 8);
    assert_eq!(values[0].as_ptr(), original);
    assert_eq!(values[4].as_ptr(), incoming);
    assert_eq!(values.as_slice(), ["a", "b", "c", "d", "e"]);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - slots
    );
}
#[cfg_attr(test, test)]
fn module_name_storage_spare_slots_accept_an_owned_name_at_full_quota() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a"]);
    let value = ModuleName::copy("b", Some(&scope)).unwrap();
    let pointer = values.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    {
        let (result, allocations) =
            allocation_observer::count(|| values.push_owned(value, Some(&scope)));
        assert_eq!(result, Ok(()));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    values.push_owned(value, Some(&scope)).unwrap();
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.as_slice(), ["a", "b"]);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn module_name_storage_iterator_keeps_backing_until_drop() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let values = names(vm.host_scheduling.as_ref(), &["a", "bb"]);
    let held = values.bytes();
    let mut values = values.into_iter();
    let first = values.next().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + held);
    let released = first.bytes();
    drop(first);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + held - released
    );
    let last = values.next().unwrap();
    let slots = held - released - last.bytes();
    drop(last);
    assert!(values.next().is_none());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + slots);
    drop(values);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn module_name_storage_detached_iterator_and_name_survive_vm_destruction() {
    let vm = page();
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let values = names(vm.host_scheduling.as_ref(), &["a", "bb"]);
    let held = values.bytes();
    let mut values = values.into_iter();
    let first = values.next().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, held);
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, first.bytes());
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn module_name_storage_append_moves_payloads_and_refunds_source_backing() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a", "b", "c", "d"]);
    let more = names(Some(&scope), &["e", "f"]);
    let original = values[0].as_ptr();
    let incoming = more[0].as_ptr();
    values.append(more, Some(&scope)).unwrap();
    assert_eq!(values.as_slice(), ["a", "b", "c", "d", "e", "f"]);
    assert_eq!(values[0].as_ptr(), original);
    assert_eq!(values[4].as_ptr(), incoming);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + values.bytes()
    );
    drop(values);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn module_name_storage_terminal_retention_refunds_names_but_keeps_backing() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a", "b", "c"]);
    let slots = host_scheduling::slots(&values);
    let released = values.bytes() - slots;
    let _peer = leave_room(&mut vm, 0);
    scope.fail();
    values.retain(|name| name == "a");
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - 2
    );
    values.retain(|_| false);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - released
    );
    assert_eq!(values.bytes(), slots);
    drop(values);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - released - slots
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn module_name_storage_foreign_owners_refuse_before_mutation() {
    let mut vm = page();
    let other = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a"]);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let value = ModuleName::copy("b", other.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        values.push_owned(value, Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(values.as_slice(), ["a"]);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(other.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn module_name_storage_host_neutral_lists_preserve_order() {
    let mut values = names(None, &["a", "b"]);
    let more = ModuleNames::singleton(
        ModuleName::resolve("test/root", "./c", ModuleType::Text, None).unwrap(),
        None,
    )
    .unwrap();
    values.append(more, None).unwrap();
    let values: Vec<_> = values.into_iter().map(ModuleName::into_unleased).collect();
    assert_eq!(values, ["a", "b", "test/c\0text"]);
}
#[cfg_attr(test, test)]
fn module_name_storage_cached_dynamic_import_retains_its_detached_name() {
    let (mut vm, _) = fixture(false);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    let value = vm
        .dynamic_import_job(
            "test/root.mjs",
            "./leaf.mjs",
            ModuleType::JavaScript,
            ImportPhase::Evaluation,
        )
        .unwrap();
    let DynamicImportResult::Waiting(name) = value else {
        panic!("leaf is still suspended");
    };
    assert_eq!(name.as_str(), "test/leaf.mjs");
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + name.bytes()
    );
    drop(name);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn module_name_storage_frontier_result_retains_its_complete_storage() {
    let (mut vm, registry) = fixture(true);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let frontier = vm
        .with_module_records(|vm, linked| {
            Vm::gather_async_dependency_names(
                "test/root.mjs",
                &registry,
                linked,
                &mut HashSet::new(),
                vm.host_scheduling.as_ref(),
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(frontier.as_slice(), ["test/leaf.mjs"]);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + frontier.bytes()
    );
    drop(frontier);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn module_name_storage_pending_refusal_after_target_preparation_publishes_no_edges() {
    let (mut vm, registry) = fixture(false);
    let value = ModuleName::resolve(
        "test/root.mjs",
        "./leaf.mjs",
        ModuleType::JavaScript,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let list = ModuleNames::singleton(value, vm.host_scheduling.as_ref()).unwrap();
    let bytes = list.bytes();
    drop(list);
    let _peer = leave_room(&mut vm, bytes);
    let used = vm.host_scheduling_snapshot().payload_bytes;
    let result = vm
        .with_module_records(|vm, linked| {
            vm.evaluate_module_record("test/root.mjs", &registry, linked)
        })
        .unwrap();
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
    let root = vm.linked_record("test/root.mjs").unwrap();
    assert!(!root.evaluating && !root.suspended && !root.evaluated);
    assert!(vm.module_pending_dependencies.is_empty());
    assert!(vm.module_async_parents.is_empty());
}
#[cfg_attr(test, test)]
fn module_name_storage_terminal_spare_push_preserves_existing_names() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut values = names(Some(&scope), &["a"]);
    let value = ModuleName::copy("b", Some(&scope)).unwrap();
    let released = value.bytes();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    scope.fail();
    assert_eq!(
        values.push_owned(value, Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(values.as_slice(), ["a"]);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used - released);
}
#[cfg_attr(test, test)]
fn module_name_storage_batch_handoff_admits_without_duplicate_input_charge() {
    let mut vm = page();
    let values = names(vm.host_scheduling.as_ref(), &["leaf"]);
    let mut preview = Vec::with_capacity(values.capacity());
    preview.extend(values.iter().cloned());
    let prepared = vm
        .prepare_async_module_dependencies("root", preview)
        .unwrap()
        .unwrap();
    let peak = prepared._peak_bytes;
    drop(prepared);
    let _peer = leave_room(&mut vm, peak - values.bytes());
    vm.register_async_module_dependencies("root", values.into_unleased())
        .unwrap();
    assert_eq!(vm.module_async_parents["leaf"], ["root"]);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - peak + vm.module_dependency_bytes()
    );
}

impl crate::Vm {
    /// Runs finite module-name producer and detached ownership contracts.
    #[doc(hidden)]
    pub fn verify_module_name_storage_contracts() {
        #[cfg(test)]
        {
            module_name_storage_eager_refusal_precedes_request_and_pending_allocation();
            module_name_storage_deferred_refusal_precedes_request_and_frontier_allocation();
        }
        module_name_storage_resolution_preserves_paths_types_and_errors();
        module_name_storage_resolution_owns_one_reported_work_buffer();
        module_name_storage_request_minimum_refuses_before_allocation();
        module_name_storage_growth_requires_the_complete_candidate();
        module_name_storage_exact_growth_moves_and_adopts_existing_owners();
        module_name_storage_spare_slots_accept_an_owned_name_at_full_quota();
        module_name_storage_iterator_keeps_backing_until_drop();
        module_name_storage_detached_iterator_and_name_survive_vm_destruction();
        module_name_storage_append_moves_payloads_and_refunds_source_backing();
        module_name_storage_terminal_retention_refunds_names_but_keeps_backing();
        module_name_storage_foreign_owners_refuse_before_mutation();
        module_name_storage_host_neutral_lists_preserve_order();
        module_name_storage_cached_dynamic_import_retains_its_detached_name();
        module_name_storage_frontier_result_retains_its_complete_storage();
        module_name_storage_pending_refusal_after_target_preparation_publishes_no_edges();
        module_name_storage_terminal_spare_push_preserves_existing_names();
        module_name_storage_batch_handoff_admits_without_duplicate_input_charge();
    }
}
