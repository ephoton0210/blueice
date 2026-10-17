// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native inspection results retain their document ownership after VM teardown.

use super::*;

fn paused() -> (Vm, Vm, Bytecode) {
    paused_source("let data='kept';0;")
}

fn paused_source(source: &str) -> (Vm, Vm, Bytecode) {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            crate::BlueJsSourceIdentity::new("page:///inspection.js", "inspection-v1").unwrap(),
            &crate::BlueJsProgramV1::Script(crate::parse(source).unwrap()),
        )
        .unwrap();
    let code = registry.get(handle).unwrap().bytecode().clone();
    let halt = code.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.execute_script_until_debugger_pause(&code, halt).unwrap();
    let view = Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    };
    (vm, view, code)
}

fn preview(vm: &Vm, code: &Bytecode) -> VmDebuggerInspection<VmDebuggerValuePreview> {
    let snapshot = vm.debugger_stack_snapshot(None, 1, 8).unwrap();
    let frame = &snapshot.frames[0];
    let slot = code
        .bindings
        .iter()
        .position(|binding| binding.name == "data")
        .unwrap() as u32;
    let entry = frame
        .scope_entries
        .iter()
        .find(|entry| entry.slot_ordinal == slot)
        .unwrap();
    vm.debugger_value_preview(None, 0, 0, frame.bytecode_offset, *entry)
        .unwrap()
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_stack_outlives_its_vm() {
    let (vm, view, _) = paused();
    let snapshot = vm.debugger_stack_snapshot(None, 1, 8).unwrap();
    let bytes = host_scheduling::slots(&snapshot.frames)
        + snapshot
            .frames
            .iter()
            .map(|frame| host_scheduling::slots(&frame.scope_entries))
            .sum::<usize>();
    assert!(bytes > 0);
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= bytes,
        "retained inspection vectors must keep their native document credit"
    );
    drop(snapshot);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_value_outlives_its_vm() {
    let (vm, view, code) = paused();
    let value = preview(&vm, &code);
    assert_eq!(
        *value,
        VmDebuggerValuePreview::StringUnits(vec![107, 101, 112, 116])
    );
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= 8,
        "retained value copies must keep their native document credit"
    );
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_clones_keep_one_shared_owner() {
    let (vm, view, code) = paused();
    let value = preview(&vm, &code);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let other = value.clone();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(vm);
    drop(value);
    assert!(
        view.host_scheduling_snapshot().payload_bytes >= 8,
        "a retained clone must share data and its native owner"
    );
    drop(other);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn target(vm: &Vm, code: &Bytecode) -> (u32, VmDebuggerScopeEntry) {
    let snapshot = vm.debugger_stack_snapshot(None, 1, 8).unwrap();
    let frame = &snapshot.frames[0];
    let slot = code
        .bindings
        .iter()
        .position(|binding| binding.name == "data")
        .unwrap() as u32;
    (
        frame.bytecode_offset,
        *frame
            .scope_entries
            .iter()
            .find(|entry| entry.slot_ordinal == slot)
            .unwrap(),
    )
}
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
fn value_buffers(value: &VmDebuggerValuePreview) -> usize {
    match value {
        VmDebuggerValuePreview::BigIntBytes(bytes) => host_scheduling::slots(bytes),
        VmDebuggerValuePreview::StringUnits(units) => host_scheduling::slots(units),
        VmDebuggerValuePreview::Array(items) => {
            host_scheduling::slots(items) + items.iter().flatten().map(value_buffers).sum::<usize>()
        }
        VmDebuggerValuePreview::Record(items) => {
            host_scheduling::slots(items)
                + items
                    .iter()
                    .map(|(key, value)| {
                        key.owned_heap_payload_bytes().unwrap() + value_buffers(value)
                    })
                    .sum::<usize>()
        }
        _ => 0,
    }
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_stack_clones_share_all_buffers() {
    let (vm, view, _) = paused();
    let snapshot = vm.debugger_stack_snapshot(None, 1, 8).unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let clone = snapshot.clone();
    assert!(std::ptr::eq(snapshot.as_ref(), clone.as_ref()));
    assert_eq!(snapshot.frames.as_ptr(), clone.frames.as_ptr());
    assert_eq!(
        snapshot.frames[0].scope_entries.as_ptr(),
        clone.frames[0].scope_entries.as_ptr()
    );
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(vm);
    drop(snapshot);
    assert!(!clone.frames[0].scope_entries.is_empty());
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(clone);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_separate_reads_keep_separate_owners() {
    let (vm, view, code) = paused();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let first = preview(&vm, &code);
    let one = view.host_scheduling_snapshot().payload_bytes;
    assert!(one > before);
    let second = preview(&vm, &code);
    assert_eq!(first, second);
    assert!(!std::ptr::eq(first.as_ref(), second.as_ref()));
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes - one,
        one - before
    );
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, one);
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_primitive_result_keeps_its_header() {
    let (vm, view, code) = paused_source("let data=true;0;");
    let value = preview(&vm, &code);
    assert_eq!(*value, VmDebuggerValuePreview::Bool(true));
    drop(vm);
    assert!(
        view.host_scheduling_snapshot().payload_bytes
            >= std::mem::size_of::<VmDebuggerValuePreview>()
    );
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_records_keep_order_holes_and_utf16() {
    let (vm, view, code) = paused_source("let data={b:'\\ud800',10:10,2:2,a:[true,, 'ok']};0;");
    let value = preview(&vm, &code);
    let VmDebuggerValuePreview::Record(entries) = value.as_ref() else {
        panic!("plain record")
    };
    assert_eq!(
        entries
            .iter()
            .map(|(key, _)| key.to_utf8().unwrap())
            .collect::<Vec<_>>(),
        ["2", "10", "b", "a"]
    );
    assert_eq!(
        entries[2].1,
        VmDebuggerValuePreview::StringUnits(vec![0xd800])
    );
    let VmDebuggerValuePreview::Array(items) = &entries[3].1 else {
        panic!("sparse array")
    };
    assert_eq!(items.len(), 3);
    assert!(items[1].is_none());
    let bytes = value_buffers(&value);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= bytes);
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_bigint_encoding_uses_fixed_small_values() {
    let (vm, view, code) = paused_source("let data=[-129n,-128n,-1n,0n,1n,127n,128n,129n];0;");
    let value = preview(&vm, &code);
    let VmDebuggerValuePreview::Array(items) = value.as_ref() else {
        panic!("BigInt array")
    };
    for (item, expected) in items.iter().zip([-129i32, -128, -1, 0, 1, 127, 128, 129]) {
        assert_eq!(
            *item,
            Some(VmDebuggerValuePreview::BigIntBytes(
                num_bigint::BigInt::from(expected).to_signed_bytes_le()
            ))
        );
    }
    let bytes = value_buffers(&value);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= bytes);
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_unsupported_shapes_allocate_no_output() {
    for source in [
        "var hits=0;let data={get x(){hits++;return 1;}};0;",
        "var hits=0;let data={};data.self=data;0;",
    ] {
        let (mut vm, view, code) = paused_source(source);
        let (offset, entry) = target(&vm, &code);
        let before = view.host_scheduling_snapshot().payload_bytes;
        let scope = vm.host_scheduling.as_ref().unwrap();
        let (result, peak) =
            scope.measure_payload_peak(|| vm.debugger_value_preview(None, 0, 0, offset, entry));
        assert!(matches!(result, Err(RuntimeError::Unsupported(_))));
        assert_eq!(peak, 0);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(
            vm.lookup_global_name("hits").unwrap(),
            Some(Value::Number(0.0))
        );
    }
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_invalid_target_allocates_no_output() {
    let (vm, view, code) = paused();
    let (offset, entry) = target(&vm, &code);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let (result, peak) = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .measure_payload_peak(|| vm.debugger_value_preview(None, 0, 1, offset, entry));
    assert!(matches!(result, Err(RuntimeError::Unsupported(_))));
    assert_eq!(peak, 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.debugger_continuation.as_ref().unwrap().pc,
        offset as usize
    );
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_documents_keep_independent_results() {
    let (first, first_view, first_code) = paused();
    let (second, second_view, second_code) = paused();
    let value = preview(&first, &first_code);
    let before = first_view.host_scheduling_snapshot().payload_bytes;
    let other = preview(&second, &second_code);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, before);
    drop(first);
    drop(second);
    assert!(first_view.host_scheduling_snapshot().payload_bytes > 0);
    let second_bytes = second_view.host_scheduling_snapshot().payload_bytes;
    assert!(second_bytes > 0);
    drop(value);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(
        second_view.host_scheduling_snapshot().payload_bytes,
        second_bytes
    );
    drop(other);
    assert_eq!(second_view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_terminal_reads_preserve_the_paused_frame() {
    let (vm, view, code) = paused();
    let (offset, entry) = target(&vm, &code);
    let pointer = vm
        .debugger_continuation
        .as_ref()
        .unwrap()
        .code
        .code
        .as_ptr();
    let before = view.host_scheduling_snapshot().payload_bytes;
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.debugger_stack_snapshot(None, 1, 8),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.debugger_value_preview(None, 0, 0, offset, entry),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.debugger_continuation
            .as_ref()
            .unwrap()
            .code
            .code
            .as_ptr(),
        pointer
    );
    assert_eq!(
        vm.debugger_continuation.as_ref().unwrap().pc,
        offset as usize
    );
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_terminal_cleanup_refunds_retained_results() {
    let (vm, view, code) = paused();
    let value = preview(&vm, &code);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(
        view.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_scope_truncation_keeps_exact_order() {
    let (vm, view, _) = paused_source("let one=1;let data=2;0;");
    let full = vm.debugger_stack_snapshot(None, 1, 8).unwrap();
    let small = vm.debugger_stack_snapshot(None, 1, 1).unwrap();
    assert_eq!(
        small.frames[0].scope_entries,
        full.frames[0].scope_entries[..1]
    );
    assert!(small.frames[0].scope_truncated);
    drop(vm);
    drop(full);
    assert!(view.host_scheduling_snapshot().payload_bytes > 0);
    drop(small);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_three_frame_result_outlives_its_vm() {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry.install(crate::BlueJsSourceIdentity::new("page:///nested-inspection.js","nested").unwrap(),
        &crate::BlueJsProgramV1::Script(crate::parse("function inner(){let data=1;return data;}function middle(){let data=2;return inner();}middle();").unwrap())).unwrap();
    let code = registry.get(handle).unwrap().bytecode().clone();
    let mut vm = page();
    let view = observer(&vm);
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap()
    else {
        panic!("nested pause")
    };
    let result = vm
        .debugger_stack_snapshot(Some(frame_serial), 3, 8)
        .unwrap();
    assert_eq!(result.frames.len(), 3);
    let bytes = host_scheduling::slots(&result.frames)
        + result
            .frames
            .iter()
            .map(|frame| host_scheduling::slots(&frame.scope_entries))
            .sum::<usize>();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= bytes);
    drop(result);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_module_request_name_outlives_its_vm() {
    let vm = page();
    let view = observer(&vm);
    let request = vm
        .prepare_debugger_module_pause_request("entry.mjs", 3)
        .unwrap();
    assert_eq!(&*request.entry, "entry.mjs");
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        "entry.mjs".len()
    );
    drop(request);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_late_activation_attaches_pending_request_once() {
    let mut vm = Vm::default();
    vm.debugger_module_pause_request = Some(
        vm.prepare_debugger_module_pause_request("entry.mjs", 3)
            .unwrap(),
    );
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    let request = vm.debugger_module_pause_request.take().unwrap();
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        "entry.mjs".len()
    );
    drop(request);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_foreign_request_attachment_preserves_its_owner() {
    let first = page();
    let view = observer(&first);
    let mut second = page();
    second.debugger_module_pause_request = Some(
        first
            .prepare_debugger_module_pause_request("entry.mjs", 3)
            .unwrap(),
    );
    let before = view.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        second.attach_retained_debugger_storage(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(second.debugger_module_pause_request.is_some());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(first);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        "entry.mjs".len()
    );
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_terminal_request_refuses_before_diagnostic_changes() {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            crate::BlueJsSourceIdentity::new("entry.mjs", "module").unwrap(),
            module("export const data=1;"),
        )
        .unwrap();
    let code = registry.get(handle).unwrap().bytecode().clone();
    let offset = code
        .instructions()
        .find(|instruction| instruction.offset >= code.module_evaluate_entry.unwrap() as usize)
        .unwrap()
        .offset as u32;
    let site = VmDebuggerThrowSite {
        program_generation: handle.generation().as_u64(),
        code_unit_ordinal: 0,
        bytecode_offset: 0,
    };
    let mut vm = page();
    vm.pending_throw_site = Some(site);
    vm.uncaught_throw_site = Some(site);
    vm.throw_epoch = 7;
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.execute_module_graph_until_debugger_pause(
            "entry.mjs",
            &HashMap::from([("entry.mjs".into(), code)]),
            offset
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.pending_throw_site, Some(site));
    assert_eq!(vm.uncaught_throw_site, Some(site));
    assert_eq!(vm.throw_epoch, 7);
    assert!(vm.debugger_module_pause_request.is_none());
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_new_bounded_reads_preserve_neutral_results() {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            crate::BlueJsSourceIdentity::new("page:///neutral.js", "neutral").unwrap(),
            &crate::BlueJsProgramV1::Script(crate::parse("let data='kept';0;").unwrap()),
        )
        .unwrap();
    let code = registry.get(handle).unwrap().bytecode().clone();
    let halt = code.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    vm.execute_script_until_debugger_pause(&code, halt).unwrap();
    let neutral = preview(&vm, &code);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bounded = preview(&vm, &code);
    assert_eq!(neutral, bounded);
    drop(vm);
    let bytes = view.host_scheduling_snapshot().payload_bytes;
    assert!(bytes > 0);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(bounded);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_linked_results_outlive_their_vm() {
    let mut registry = crate::BlueJsProgramRegistry::default();
    let entry = "pages/entry.mjs";
    let dependency = "pages/dep.mjs";
    let dep = registry
        .install_precompiled(
            crate::BlueJsSourceIdentity::new(dependency, "dep").unwrap(),
            module("export function child(){return 2;}"),
        )
        .unwrap();
    let root = registry
        .install_precompiled(
            crate::BlueJsSourceIdentity::new(entry, "root").unwrap(),
            module("import {child} from './dep.mjs';export const data='kept';child();"),
        )
        .unwrap();
    let root_code = registry.get(root).unwrap().bytecode().clone();
    let graph = HashMap::from([
        (entry.into(), root_code.clone()),
        (
            dependency.into(),
            registry.get(dep).unwrap().bytecode().clone(),
        ),
    ]);
    let mut vm = page();
    let view = observer(&vm);
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_module_graph_until_linked_nested_debugger_pause(
            entry,
            dependency,
            &graph,
            VmDebuggerLinkedPauseTarget {
                entry_generation: root.generation().as_u64(),
                dependency_generation: dep.generation().as_u64(),
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            },
        )
        .unwrap()
    else {
        panic!("linked pause")
    };
    let stack = vm
        .debugger_linked_stack_snapshot(frame_serial, 2, 8)
        .unwrap();
    let slot = root_code
        .bindings
        .iter()
        .position(|binding| binding.name == "data")
        .unwrap() as u32;
    let binding = *stack.frames[1]
        .scope_entries
        .iter()
        .find(|binding| binding.slot_ordinal == slot)
        .unwrap();
    let value = vm
        .debugger_linked_value_preview(frame_serial, &stack, binding)
        .unwrap();
    assert_eq!(
        *value,
        VmDebuggerValuePreview::StringUnits(vec![107, 101, 112, 116])
    );
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes > 8);
    drop(stack);
    assert!(view.host_scheduling_snapshot().payload_bytes >= 8);
    drop(value);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_debugger_inspection_storage_contracts() {
        debugger_inspection_storage_page_results_survive_navigation_and_close();
        debugger_inspection_storage_stack_outlives_its_vm();
        debugger_inspection_storage_value_outlives_its_vm();
        debugger_inspection_storage_clones_keep_one_shared_owner();
        debugger_inspection_storage_stack_clones_share_all_buffers();
        debugger_inspection_storage_separate_reads_keep_separate_owners();
        debugger_inspection_storage_primitive_result_keeps_its_header();
        debugger_inspection_storage_records_keep_order_holes_and_utf16();
        debugger_inspection_storage_bigint_encoding_uses_fixed_small_values();
        debugger_inspection_storage_unsupported_shapes_allocate_no_output();
        debugger_inspection_storage_invalid_target_allocates_no_output();
        debugger_inspection_storage_documents_keep_independent_results();
        debugger_inspection_storage_terminal_reads_preserve_the_paused_frame();
        debugger_inspection_storage_terminal_cleanup_refunds_retained_results();
        debugger_inspection_storage_scope_truncation_keeps_exact_order();
        debugger_inspection_storage_three_frame_result_outlives_its_vm();
        debugger_inspection_storage_module_request_name_outlives_its_vm();
        debugger_inspection_storage_late_activation_attaches_pending_request_once();
        debugger_inspection_storage_foreign_request_attachment_preserves_its_owner();
        debugger_inspection_storage_terminal_request_refuses_before_diagnostic_changes();
        debugger_inspection_storage_new_bounded_reads_preserve_neutral_results();
        debugger_inspection_storage_linked_results_outlive_their_vm();
    }
}

#[cfg_attr(test, test)]
fn debugger_inspection_storage_page_results_survive_navigation_and_close() {
    let mut runtime = crate::BlueJsPageRuntime::default();
    let origin = crate::BlueJsPageOrigin::new("https://inspection.test").unwrap();
    runtime.open_realm(7, origin.clone()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin,
            crate::BlueJsSourceIdentity::new("page:///outgoing.js", "outgoing").unwrap(),
            &crate::BlueJsProgramV1::Script(crate::parse("let data='kept';0;").unwrap()),
        )
        .unwrap();
    let halt = runtime
        .safe_points(7, program, 64)
        .unwrap()
        .into_iter()
        .rfind(|point| point.code_unit.ordinal() == 0)
        .unwrap();
    runtime
        .execute_program_until_debugger_pause(7, program, halt)
        .unwrap();
    let stack = runtime
        .debugger_stack_snapshot(7, program, None, 1, 8)
        .unwrap();
    let frame = &stack.frames[0];
    let value = runtime
        .debugger_value_preview(
            7,
            program,
            None,
            crate::BlueJsPageDebuggerValueTarget {
                frame_index: 0,
                code_unit_ordinal: 0,
                bytecode_offset: frame.bytecode_offset,
                scope_entry: frame.scope_entries[0],
            },
        )
        .unwrap();
    let before = runtime.host_scheduling_usage(7).unwrap().payload_bytes;
    let clone = value.clone();
    assert!(std::ptr::eq(value.as_ref(), clone.as_ref()));
    assert_eq!(
        runtime.host_scheduling_usage(7).unwrap().payload_bytes,
        before
    );
    runtime.navigate(7, origin).unwrap();
    assert!(runtime.close_realm(7));
    drop(runtime);
    assert_eq!(
        *value,
        VmDebuggerValuePreview::StringUnits(vec![107, 101, 112, 116])
    );
    assert_eq!(stack.frames.len(), 1);
    drop(value);
    assert_eq!(
        *clone,
        VmDebuggerValuePreview::StringUnits(vec![107, 101, 112, 116])
    );
}
