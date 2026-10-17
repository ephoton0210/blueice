// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small dependency batches verify complete staged ownership and reclamation.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
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
#[test]
fn dependency_storage_full_quota_refuses_before_allocating_registration_scratch() {
    let mut vm = page();
    let input = vec!["leaf".into()];
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| vm.register_async_module_dependencies("root", input));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        allocations, 0,
        "admission must precede all new registration scratch"
    );
    assert!(vm.module_pending_dependencies.is_empty());
    assert!(vm.module_async_parents.is_empty());
}
#[cfg_attr(test, test)]
fn dependency_storage_reverse_growth_requires_complete_candidate_not_net_delta() {
    let mut vm = page();
    for index in 0..4 {
        vm.register_async_module_dependencies(&format!("p{index}"), vec!["leaf".into()])
            .unwrap();
    }
    assert_eq!(vm.module_async_parents["leaf"].capacity(), 4);
    let forward = vm.module_pending_dependencies.len();
    let parents = vm.module_async_parents["leaf"].clone();
    let live_delta = 2 * "p4".len()
        + "leaf".len()
        + HashSet::<String>::with_capacity(1).capacity() * std::mem::size_of::<String>()
        + 4 * std::mem::size_of::<String>();
    let _peer = leave_room(&mut vm, live_delta);
    assert_eq!(
        vm.register_async_module_dependencies("p4", vec!["leaf".into()]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_pending_dependencies.len(), forward);
    assert_eq!(vm.module_async_parents["leaf"], parents);
    assert_eq!(vm.module_async_parents["leaf"].capacity(), 4);
}
#[cfg_attr(test, test)]
fn dependency_storage_reclamation_without_headroom_keeps_existing_allocations() {
    let mut vm = page();
    vm.register_async_module_dependencies("root", vec!["leaf".into()])
        .unwrap();
    vm.module_pending_dependencies.reserve(8);
    vm.module_async_parents.reserve(8);
    let capacities = (
        vm.module_pending_dependencies.capacity(),
        vm.module_async_parents.capacity(),
    );
    let _peer = leave_room(&mut vm, 0);
    vm.reclaim_module_dependency_storage();
    assert_eq!(
        (
            vm.module_pending_dependencies.capacity(),
            vm.module_async_parents.capacity()
        ),
        capacities
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
}

#[derive(Debug, PartialEq, Eq)]
struct Shape {
    capacity: (usize, usize),
    bytes: usize,
    forward: Vec<(String, usize, usize, Vec<String>)>,
    reverse: Vec<(String, usize, Vec<String>)>,
}
fn shape(vm: &Vm) -> Shape {
    let mut forward: Vec<_> = vm
        .module_pending_dependencies
        .iter()
        .map(|(name, set)| {
            let mut values: Vec<_> = set.iter().cloned().collect();
            values.sort();
            (name.clone(), set.capacity(), set.backing_bytes(), values)
        })
        .collect();
    forward.sort_by(|a, b| a.0.cmp(&b.0));
    let mut reverse: Vec<_> = vm
        .module_async_parents
        .iter()
        .map(|(name, values)| (name.clone(), values.capacity(), values.clone()))
        .collect();
    reverse.sort_by(|a, b| a.0.cmp(&b.0));
    Shape {
        capacity: (
            vm.module_pending_dependencies.capacity(),
            vm.module_async_parents.capacity(),
        ),
        bytes: vm.module_dependency_bytes(),
        forward,
        reverse,
    }
}
fn input(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}
fn four_parents() -> Vm {
    let mut vm = page();
    for index in 0..4 {
        vm.register_async_module_dependencies(&format!("p{index}"), input(&["leaf"]))
            .unwrap();
    }
    vm
}
fn preview(vm: &mut Vm, parent: &str, names: &[&str]) -> usize {
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm
        .prepare_async_module_dependencies(parent, input(names))
        .unwrap()
        .unwrap();
    let peak = prepared._peak_bytes;
    drop(prepared);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    peak
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn sparse_layers() -> Vm {
    let mut vm = page();
    vm.register_async_module_dependencies("root", input(&["leaf"]))
        .unwrap();
    vm.module_pending_dependencies.reserve(8);
    vm.module_async_parents.reserve(8);
    vm.module_pending_dependencies
        .get_mut("root")
        .unwrap()
        .reserve(8);
    vm.module_async_parents
        .get_mut("leaf")
        .unwrap()
        .reserve_exact(15);
    vm
}
#[cfg_attr(test, test)]
fn dependency_storage_preparation_preserves_every_live_graph_buffer() {
    let mut vm = four_parents();
    let before = shape(&vm);
    let heap = vm.heap.stats();
    let prepared = vm
        .prepare_async_module_dependencies("p4", input(&["leaf", "twig"]))
        .unwrap()
        .unwrap();
    assert_eq!(shape(&vm), before);
    assert_eq!(vm.heap.stats(), heap);
    drop(prepared);
    assert_eq!(shape(&vm), before);
}
#[cfg_attr(test, test)]
fn dependency_storage_abandoned_batch_refunds_all_native_staging() {
    let mut vm = four_parents();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm
        .prepare_async_module_dependencies("p4", input(&["leaf", "twig"]))
        .unwrap()
        .unwrap();
    assert!(vm.host_scheduling_snapshot().payload_bytes > before);
    drop(prepared);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn dependency_storage_detached_batch_keeps_ownership_after_vm_destruction() {
    let mut vm = four_parents();
    let view = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let prepared = vm
        .prepare_async_module_dependencies("p4", input(&["leaf", "twig"]))
        .unwrap()
        .unwrap();
    let held = vm.host_scheduling_snapshot().payload_bytes - before;
    assert!(held > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, held);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn dependency_storage_exact_peak_admits_complete_reverse_growth() {
    let mut vm = four_parents();
    let peak = preview(&mut vm, "p4", &["leaf"]);
    let old = vm.module_dependency_bytes();
    let _peer = leave_room(&mut vm, peak);
    let prepared = vm
        .prepare_async_module_dependencies("p4", input(&["leaf"]))
        .unwrap()
        .unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    prepared.publish(&mut vm, "p4");
    assert_eq!(
        vm.module_async_parents["leaf"],
        vec!["p0", "p1", "p2", "p3", "p4"]
    );
    assert_eq!(vm.module_async_parents["leaf"].capacity(), 8);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - peak + vm.module_dependency_bytes() - old
    );
}
#[cfg_attr(test, test)]
fn dependency_storage_short_peak_preserves_both_directions_and_capacities() {
    let mut vm = four_parents();
    let peak = preview(&mut vm, "p4", &["leaf"]);
    let before = shape(&vm);
    let _peer = leave_room(&mut vm, peak - 1);
    let used = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        vm.register_async_module_dependencies("p4", input(&["leaf"])),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(shape(&vm), before);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
}
#[cfg_attr(test, test)]
fn dependency_storage_existing_batch_is_allocation_free_at_full_quota() {
    let mut vm = four_parents();
    let names = input(&["leaf", "leaf"]);
    let before = shape(&vm);
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    {
        let (result, allocations) =
            allocation_observer::count(|| vm.register_async_module_dependencies("p0", names));
        assert_eq!(result, Ok(()));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    vm.register_async_module_dependencies("p0", names).unwrap();
    assert_eq!(shape(&vm), before);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn dependency_storage_mixed_duplicates_preserve_registration_order() {
    let mut vm = page();
    vm.register_async_module_dependencies("p0", input(&["leaf"]))
        .unwrap();
    vm.register_async_module_dependencies("p0", input(&["twig", "leaf", "twig", "leaf"]))
        .unwrap();
    vm.register_async_module_dependencies("p1", input(&["twig", "leaf"]))
        .unwrap();
    assert_eq!(vm.module_pending_dependencies["p0"].len(), 2);
    assert_eq!(vm.module_async_parents["leaf"], vec!["p0", "p1"]);
    assert_eq!(vm.module_async_parents["twig"], vec!["p0", "p1"]);
}
#[cfg_attr(test, test)]
fn dependency_storage_inner_set_and_vector_reclamation_need_headroom() {
    let mut vm = sparse_layers();
    let before = shape(&vm);
    let _peer = leave_room(&mut vm, 0);
    vm.reclaim_module_dependency_storage();
    assert_eq!(shape(&vm), before);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn dependency_storage_exact_sequential_reclamation_preserves_name_buffers() {
    let mut vm = sparse_layers();
    let forward = vm.module_pending_dependencies["root"]
        .get("leaf")
        .unwrap()
        .as_ptr();
    let reverse = vm.module_async_parents["leaf"][0].as_ptr();
    let before = vm.module_dependency_bytes();
    let allowance = HashSet::<String>::with_capacity(1).capacity() * std::mem::size_of::<String>();
    let _peer = leave_room(&mut vm, allowance);
    vm.reclaim_module_dependency_storage();
    assert_eq!(vm.module_pending_dependencies.capacity(), 3);
    assert_eq!(vm.module_async_parents.capacity(), 3);
    assert_eq!(vm.module_pending_dependencies["root"].capacity(), 3);
    assert_eq!(vm.module_async_parents["leaf"].capacity(), 1);
    assert_eq!(
        vm.module_pending_dependencies["root"]
            .get("leaf")
            .unwrap()
            .as_ptr(),
        forward
    );
    assert_eq!(vm.module_async_parents["leaf"][0].as_ptr(), reverse);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - allowance - before + vm.module_dependency_bytes()
    );
}
#[cfg_attr(test, test)]
fn dependency_storage_short_reported_set_candidate_is_nonterminal() {
    let mut vm = page();
    vm.register_async_module_dependencies("root", input(&["leaf"]))
        .unwrap();
    vm.module_pending_dependencies
        .get_mut("root")
        .unwrap()
        .reserve(8);
    let before = shape(&vm);
    let allowance =
        HashSet::<String>::with_capacity(1).capacity() * std::mem::size_of::<String>() - 1;
    let _peer = leave_room(&mut vm, allowance);
    let used = vm.host_scheduling_snapshot().payload_bytes;
    vm.reclaim_module_dependency_storage();
    assert_eq!(shape(&vm), before);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn dependency_storage_optional_reclamation_retries_after_peer_release() {
    let mut vm = sparse_layers();
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_module_dependency_storage();
    assert!(vm.module_pending_dependencies.capacity() > 3);
    drop(peer);
    vm.reclaim_module_dependency_storage();
    assert_eq!(vm.module_pending_dependencies.capacity(), 3);
    assert_eq!(vm.module_async_parents.capacity(), 3);
    assert!(vm.check_scheduling_failure().is_ok());
}
#[cfg_attr(test, test)]
fn dependency_storage_terminal_cleanup_releases_empty_graph_without_allocation() {
    let mut vm = page();
    let baseline = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.register_async_module_dependencies("root", input(&["leaf", "twig"]))
        .unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    #[cfg(test)]
    {
        let (_, allocations) =
            allocation_observer::count(|| vm.remove_module_dependency_parent("root"));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    vm.remove_module_dependency_parent("root");
    assert_eq!(vm.module_pending_dependencies.retained_bytes(), 0);
    assert_eq!(vm.module_async_parents.retained_bytes(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, baseline);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn dependency_storage_deletion_refunds_names_without_refunding_backing() {
    let mut vm = page();
    vm.register_async_module_dependencies("root", input(&["a", "b", "c", "d", "e", "f", "g"]))
        .unwrap();
    let map = vm.module_async_parents.retained_bytes();
    let set = vm.module_pending_dependencies["root"].backing_bytes();
    let released = vm.module_pending_dependencies["root"]
        .get("a")
        .unwrap()
        .capacity()
        + vm.module_async_parents
            .get_key_value("a")
            .unwrap()
            .0
            .capacity()
        + string_vector_bytes(&vm.module_async_parents["a"]);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.module_pending_dependencies
        .get_mut("root")
        .unwrap()
        .remove("a");
    vm.module_async_parents.remove("a");
    assert_eq!(vm.module_async_parents.retained_bytes(), map);
    assert_eq!(vm.module_pending_dependencies["root"].backing_bytes(), set);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before - released
    );
}
#[cfg_attr(test, test)]
fn dependency_storage_host_neutral_registration_and_cleanup_preserve_edges() {
    let mut vm = Vm::default();
    vm.register_async_module_dependencies("root", input(&["leaf", "twig", "leaf"]))
        .unwrap();
    assert_eq!(vm.module_pending_dependencies["root"].len(), 2);
    assert_eq!(vm.module_async_parents["leaf"], vec!["root"]);
    vm.remove_module_dependency_parent("root");
    assert_eq!(vm.module_dependency_bytes(), 0);
}
#[cfg_attr(test, test)]
fn dependency_storage_forward_set_growth_moves_existing_name_buffers() {
    let mut vm = page();
    vm.register_async_module_dependencies("root", input(&["a", "b", "c"]))
        .unwrap();
    let pointer = vm.module_pending_dependencies["root"]
        .get("a")
        .unwrap()
        .as_ptr();
    let before = shape(&vm);
    let prepared = vm
        .prepare_async_module_dependencies("root", input(&["d"]))
        .unwrap()
        .unwrap();
    assert_eq!(shape(&vm), before);
    prepared.publish(&mut vm, "root");
    assert_eq!(vm.module_pending_dependencies["root"].len(), 4);
    assert_eq!(
        vm.module_pending_dependencies["root"]
            .get("a")
            .unwrap()
            .as_ptr(),
        pointer
    );
    assert_eq!(vm.module_async_parents["d"], vec!["root"]);
}
#[cfg_attr(test, test)]
fn dependency_storage_input_capacity_is_admitted_before_edge_copies() {
    let mut vm = page();
    let mut names = Vec::with_capacity(64);
    let mut name = String::with_capacity(32);
    name.push_str("leaf");
    names.push(name);
    let peak = string_vector_bytes(&names)
        + HashSet::<String>::with_capacity(1).capacity() * std::mem::size_of::<String>();
    let _peer = leave_room(&mut vm, peak - 1);
    #[cfg(test)]
    {
        let (result, allocations) =
            allocation_observer::count(|| vm.register_async_module_dependencies("root", names));
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert_eq!(allocations, 1);
    }
    #[cfg(not(test))]
    assert_eq!(
        vm.register_async_module_dependencies("root", names),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.module_pending_dependencies.is_empty());
    assert!(vm.module_async_parents.is_empty());
}
#[cfg_attr(test, test)]
fn dependency_storage_full_maps_support_zero_additional_key_slots() {
    let mut vm = page();
    vm.register_async_module_dependencies("p0", input(&["a", "b", "c"]))
        .unwrap();
    vm.register_async_module_dependencies("p1", input(&["a"]))
        .unwrap();
    vm.register_async_module_dependencies("p2", input(&["b"]))
        .unwrap();
    assert_eq!(
        vm.module_pending_dependencies.len(),
        vm.module_pending_dependencies.capacity()
    );
    assert_eq!(
        vm.module_async_parents.len(),
        vm.module_async_parents.capacity()
    );
    let peak = preview(&mut vm, "p1", &["c"]);
    let before = vm.module_dependency_bytes();
    let _peer = leave_room(&mut vm, peak);
    vm.register_async_module_dependencies("p1", input(&["c"]))
        .unwrap();
    assert_eq!(vm.module_pending_dependencies.capacity(), 3);
    assert_eq!(vm.module_async_parents.capacity(), 3);
    assert_eq!(vm.module_async_parents["c"], vec!["p0", "p1"]);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - peak + vm.module_dependency_bytes() - before
    );
}

impl crate::Vm {
    /// Runs finite dependency-batch and retained-storage contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_dependency_storage_contracts() {
        #[cfg(test)]
        dependency_storage_full_quota_refuses_before_allocating_registration_scratch();
        dependency_storage_reverse_growth_requires_complete_candidate_not_net_delta();
        dependency_storage_reclamation_without_headroom_keeps_existing_allocations();
        dependency_storage_preparation_preserves_every_live_graph_buffer();
        dependency_storage_abandoned_batch_refunds_all_native_staging();
        dependency_storage_detached_batch_keeps_ownership_after_vm_destruction();
        dependency_storage_exact_peak_admits_complete_reverse_growth();
        dependency_storage_short_peak_preserves_both_directions_and_capacities();
        dependency_storage_existing_batch_is_allocation_free_at_full_quota();
        dependency_storage_mixed_duplicates_preserve_registration_order();
        dependency_storage_inner_set_and_vector_reclamation_need_headroom();
        dependency_storage_exact_sequential_reclamation_preserves_name_buffers();
        dependency_storage_short_reported_set_candidate_is_nonterminal();
        dependency_storage_optional_reclamation_retries_after_peer_release();
        dependency_storage_terminal_cleanup_releases_empty_graph_without_allocation();
        dependency_storage_deletion_refunds_names_without_refunding_backing();
        dependency_storage_host_neutral_registration_and_cleanup_preserve_edges();
        dependency_storage_forward_set_growth_moves_existing_name_buffers();
        dependency_storage_input_capacity_is_admitted_before_edge_copies();
        dependency_storage_full_maps_support_zero_additional_key_slots();
    }
}
