// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Synthetic export buffers have one admitted owner across module snapshots.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.set_text_module_sources(HashMap::from([("value".into(), "a😀z".into())]));
    vm
}

fn synthesize(vm: &mut Vm) -> Result<HashMap<String, Bytecode>, RuntimeError> {
    let mut modules = HashMap::new();
    vm.ensure_synthetic_module(
        "value\0text",
        SyntheticModuleType::Text,
        &mut modules,
        &mut Vec::new(),
    )?;
    Ok(modules)
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

fn export_pointer(modules: &HashMap<String, Bytecode>) -> *const u16 {
    let Value::String(text) = modules["value\0text"]
        .synthetic_default_export
        .as_ref()
        .unwrap()
        .value()
    else {
        panic!("the text module has a string export")
    };
    text.as_code_units().as_ptr()
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_retained_export_adds_a_document_charge() {
    let mut vm = bounded_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let _modules = synthesize(&mut vm).unwrap();
    assert!(vm.host_scheduling_snapshot().payload_bytes >= before + 8);
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_refuses_before_text_conversion_when_no_budget_remains() {
    let mut vm = bounded_vm();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.heap.stats();
    assert!(matches!(
        synthesize(&mut vm),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.heap.stats().managed_bytes, before.managed_bytes);
    assert_eq!(
        vm.heap.stats().root_registrations,
        before.root_registrations
    );
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_bytecode_snapshots_share_the_same_export_buffer() {
    let mut vm = bounded_vm();
    let modules = synthesize(&mut vm).unwrap();
    let snapshot = modules.clone();
    assert_eq!(export_pointer(&modules), export_pointer(&snapshot));
}

fn export(modules: &HashMap<String, Bytecode>) -> &SyntheticModuleValue {
    modules["value\0text"]
        .synthetic_default_export
        .as_ref()
        .unwrap()
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_text_exact_budget_and_one_byte_refusal() {
    for source in ["", "a", "a😀z"] {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            vm.set_text_module_sources(HashMap::from([("value".into(), source.into())]));
            let bytes = SyntheticModuleValue::header_bytes() + source.encode_utf16().count() * 2;
            let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let result = synthesize(&mut vm);
            if accepts {
                let modules = result.unwrap();
                assert_eq!(export(&modules).value(), &Value::String(source.into()));
                assert_eq!(export(&modules).retained_bytes(), bytes);
                assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
                drop(modules);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_snapshots_refund_only_after_the_last_drop_even_after_failure() {
    let mut vm = bounded_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let modules = synthesize(&mut vm).unwrap();
    let bytes = export(&modules).retained_bytes();
    let peer = leave_room(&mut vm, 0);
    let snapshot = modules.clone();
    assert_eq!(export_pointer(&modules), export_pointer(&snapshot));
    drop(peer);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(modules);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_preloaded_values_attach_once_and_refused_activation_is_terminal() {
    let mut unbounded = Vm::default();
    unbounded.set_text_module_sources(HashMap::from([("value".into(), "tiny".into())]));
    let modules = synthesize(&mut unbounded).unwrap();
    let bytes = export(&modules).retained_bytes();
    unbounded.set_module_loader_context("entry", modules);
    unbounded.use_bounded_host_microtask_checkpoints().unwrap();
    let expected = unbounded.own_scheduling_usage().payload_bytes
        + unbounded.text_module_sources.retained_bytes()
        + bytes;
    assert_eq!(unbounded.host_scheduling_snapshot().payload_bytes, expected);
    unbounded.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(unbounded.host_scheduling_snapshot().payload_bytes, expected);

    let mut source = Vm::default();
    source.set_text_module_sources(HashMap::from([("value".into(), "tiny".into())]));
    let modules = synthesize(&mut source).unwrap();
    let mut target = bounded_vm();
    let _peer = leave_room(&mut target, 0);
    let before = target.host_scheduling_snapshot().payload_bytes;
    target.module_registry = modules;
    assert_eq!(
        target.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(target.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        target.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_another_document_cannot_replace_the_host_loader_context() {
    let mut owner = bounded_vm();
    let modules = synthesize(&mut owner).unwrap();
    let before = owner.host_scheduling_snapshot().payload_bytes;
    let mut other = bounded_vm();
    other.set_module_loader_context("kept", HashMap::from([("kept".into(), Bytecode::empty())]));
    other.set_module_loader_context("blocked", modules);
    assert_eq!(other.active_module_name.as_deref(), Some("kept"));
    assert_eq!(other.module_registry.len(), 1);
    assert!(other.module_registry.contains_key("kept"));
    assert_eq!(
        other.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    // The rejected incoming table was its value's last owner.
    assert_eq!(
        owner.host_scheduling_snapshot().payload_bytes,
        before - SyntheticModuleValue::header_bytes() - 8
    );
    assert_eq!(owner.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_scoped_inputs_admit_actual_buffers_and_refund_on_drop() {
    for kind in 0..3 {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            let bytes = if kind == 1 { 3 } else { 8 };
            let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let scope = vm.host_scheduling.as_ref();
            let result = match kind {
                0 => module_values::ModuleValueCopy::from_utf8("a😀z", scope).map(|copy| {
                    assert_eq!(copy.value(), &Value::String("a😀z".into()));
                    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
                    drop(copy);
                }),
                1 => module_values::ModuleBytesCopy::copy(&[1, 2, 3], scope).map(|copy| {
                    assert_eq!(copy.len(), 3);
                    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
                    drop(copy);
                }),
                _ => module_values::ModuleValueCopy::copy(&Value::String("a😀z".into()), scope)
                    .map(|copy| {
                        assert_eq!(copy.value(), &Value::String("a😀z".into()));
                        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
                        drop(copy);
                    }),
            };
            assert_eq!(
                result,
                if accepts {
                    Ok(())
                } else {
                    Err(RuntimeError::SchedulingLimit)
                }
            );
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_cell_handoff_admits_the_copy_and_releases_it_inside_heap_adoption() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let modules = synthesize(&mut vm).unwrap();
        let cell = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(cell).unwrap();
        let _peer = leave_room(&mut vm, 8 - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let managed = vm.heap.stats().managed_bytes;
        let result = vm.install_synthetic_export(cell, export(&modules));
        if accepts {
            result.unwrap();
            assert_eq!(vm.heap.get(cell, "value"), Ok(Value::String("a😀z".into())));
            assert!(vm.heap.stats().managed_bytes > managed);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(vm.heap.get_own(cell, "value"), Ok(None));
            assert_eq!(vm.heap.stats().managed_bytes, managed);
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        vm.heap.unroot(root).unwrap();
    }
    let mut vm = bounded_vm();
    let modules = synthesize(&mut vm).unwrap();
    let mut foreign = Vm::default();
    let cell = foreign.heap.alloc_object(None).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        vm.install_synthetic_export(cell, export(&modules)),
        Err(RuntimeError::Heap(HeapError::InvalidObject(cell)))
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

fn warm_bytes(vm: &mut Vm) {
    vm.buffer_prototype("ArrayBuffer").unwrap();
    vm.buffer_prototype("Uint8Array").unwrap();
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_bytes_transfer_to_the_heap_and_refuse_missing_root_storage() {
    for spare in [false, true] {
        let mut vm = bounded_vm();
        warm_bytes(&mut vm);
        vm.stack = Vec::new();
        if spare {
            vm.stack.reserve(4);
        }
        let _peer = leave_room(&mut vm, 3);
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let managed = vm.heap.stats().managed_bytes;
        let bytes =
            module_values::ModuleBytesCopy::copy(&[1, 2, 3], vm.host_scheduling.as_ref()).unwrap();
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + 3);
        let result = vm.create_bytes_module_value(bytes);
        if spare {
            let value = result.unwrap();
            assert_eq!(vm.get_property(&value, &"1".into()), Ok(Value::Number(2.0)));
            assert!(vm.heap.stats().managed_bytes > managed);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(vm.heap.stats().managed_bytes, managed);
        }
        assert!(vm.stack.is_empty());
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

fn synthesize_json(
    vm: &mut Vm,
    source: &str,
    roots: &mut Vec<RootId>,
) -> Result<HashMap<String, Bytecode>, RuntimeError> {
    vm.set_json_module_sources(HashMap::from([("json".into(), source.into())]));
    vm.remaining_instructions = 100;
    let mut modules = HashMap::new();
    vm.ensure_synthetic_module("json\0json", SyntheticModuleType::Json, &mut modules, roots)?;
    Ok(modules)
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_json_retains_only_the_derived_owner_and_refunds_refused_roots() {
    for source in ["42", "\"a😀z\"", "{\"answer\":42}", "{"] {
        let mut vm = bounded_vm();
        let mut roots = Vec::new();
        let result = synthesize_json(&mut vm, source, &mut roots);
        let before = vm.own_scheduling_usage().payload_bytes
            + vm.text_module_sources.retained_bytes()
            + vm.json_module_sources.retained_bytes();
        if source == "{" {
            assert!(matches!(result, Err(RuntimeError::ModuleResolution(_))));
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        } else {
            let modules = result.unwrap();
            let value = modules["json\0json"]
                .synthetic_default_export
                .as_ref()
                .unwrap();
            let bytes = value.retained_bytes();
            assert_eq!(
                bytes,
                SyntheticModuleValue::header_bytes()
                    + crate::vm::host_scheduling::value_payload_bytes(value.value())
            );
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
            drop(modules);
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
        for root in roots {
            vm.heap.unroot(root).unwrap();
        }
    }
    let mut vm = bounded_vm();
    vm.set_json_module_sources(HashMap::from([("json".into(), "{}".into())]));
    vm.stack.reserve(4);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.heap.allow_root_registrations(0);
    assert!(matches!(
        synthesize_json(&mut vm, "{}", &mut Vec::new()),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_json_input_and_output_coexistence_is_admitted_before_heap_effects() {
    let mut vm = bounded_vm();
    vm.set_json_module_sources(HashMap::from([("json".into(), "{}".into())]));
    // Four UTF-16 bytes for each input/output reservation, plus the owner.
    let room = SyntheticModuleValue::header_bytes() + 7;
    let _peer = leave_room(&mut vm, room);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let managed = vm.heap.stats().managed_bytes;
    let mut modules = HashMap::new();
    assert_eq!(
        vm.ensure_synthetic_module(
            "json\0json",
            SyntheticModuleType::Json,
            &mut modules,
            &mut Vec::new()
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(modules.is_empty());
    assert_eq!(vm.heap.stats().managed_bytes, managed);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_shadow_import_shares_primitives_filters_objects_and_refunds_the_last_realm(
) {
    let mut vm = bounded_vm();
    let mut modules = synthesize(&mut vm).unwrap();
    let pointer = export_pointer(&modules);
    let mut roots = Vec::new();
    modules.extend(synthesize_json(&mut vm, "{}", &mut roots).unwrap());
    modules.insert(
        "entry.mjs".into(),
        module("export function answer(){return 42;}"),
    );
    vm.set_module_loader_context("main.mjs", modules);
    let realm = vm.execute_script(&script("new ShadowRealm();")).unwrap();
    vm.shadow_realm_import_value(
        realm.clone(),
        Value::String("./entry.mjs".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    {
        let child = record.vm.borrow();
        assert_eq!(export_pointer(&child.module_registry), pointer);
        assert!(!child.module_registry.contains_key("json\0json"));
    }
    vm.set_module_loader_context("main.mjs", HashMap::new());
    for root in roots {
        vm.heap.unroot(root).unwrap();
    }
    assert!(
        vm.host_scheduling_snapshot().payload_bytes
            > vm.own_scheduling_usage().payload_bytes
                + vm.text_module_sources.retained_bytes()
                + vm.json_module_sources.retained_bytes()
    );
    drop(record);
    vm.execute_script(&script("0;")).unwrap();
    assert!(vm.shadow_realms.is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.text_module_sources.retained_bytes()
            + vm.json_module_sources.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_real_typed_imports_survive_gc_with_their_original_values() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("page/main.js", HashMap::new());
    vm.set_json_module_sources(HashMap::from([("page/j".into(), "\"j😀\"".into())]));
    vm.set_text_module_sources(HashMap::from([("page/t".into(), "text".into())]));
    vm.set_bytes_module_sources(HashMap::from([("page/b".into(), vec![4, 2])]));
    vm.execute_script(&script("var seen=false;Promise.all([import('./j',{with:{type:'json'}}),import('./t',{with:{type:'text'}}),import('./b',{with:{type:'bytes'}})]).then(([j,t,b])=>seen=j.default==='j😀'&&t.default==='text'&&b.default[0]===4&&b.default[1]===2);0;")).unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.promise_jobs.is_empty());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.lookup_global_name("seen").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(
        vm.module_registry
            .values()
            .filter(|code| code.synthetic_default_export.is_some())
            .count(),
        3
    );
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn synthetic_value_ownership_borrowed_graph_inputs_attach_before_publication() {
    let mut source = Vm::default();
    source.set_text_module_sources(HashMap::from([("value".into(), "tiny".into())]));
    let modules = synthesize(&mut source).unwrap();
    let mut vm = bounded_vm();
    vm.execute_module_graph("value\0text", &modules).unwrap();
    assert_eq!(
        export_pointer(&modules),
        export_pointer(&vm.module_registry)
    );
    let before = vm.host_scheduling_snapshot().payload_bytes;
    Vm::attach_synthetic_values(&modules, vm.host_scheduling.as_ref().unwrap()).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    let mut other = bounded_vm();
    assert!(matches!(
        other.execute_module_graph("value\0text", &modules),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(other.module_graph.is_none() && other.module_registry.is_empty());
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

impl crate::Vm {
    /// Runs finite synthetic-export storage contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_synthetic_value_ownership_contracts() {
        synthetic_value_ownership_retained_export_adds_a_document_charge();
        synthetic_value_ownership_refuses_before_text_conversion_when_no_budget_remains();
        synthetic_value_ownership_bytecode_snapshots_share_the_same_export_buffer();
        synthetic_value_ownership_text_exact_budget_and_one_byte_refusal();
        synthetic_value_ownership_snapshots_refund_only_after_the_last_drop_even_after_failure();
        synthetic_value_ownership_preloaded_values_attach_once_and_refused_activation_is_terminal();
        synthetic_value_ownership_another_document_cannot_replace_the_host_loader_context();
        synthetic_value_ownership_scoped_inputs_admit_actual_buffers_and_refund_on_drop();
        synthetic_value_ownership_cell_handoff_admits_the_copy_and_releases_it_inside_heap_adoption(
        );
        synthetic_value_ownership_bytes_transfer_to_the_heap_and_refuse_missing_root_storage();
        synthetic_value_ownership_json_retains_only_the_derived_owner_and_refunds_refused_roots();
        synthetic_value_ownership_json_input_and_output_coexistence_is_admitted_before_heap_effects(
        );
        synthetic_value_ownership_shadow_import_shares_primitives_filters_objects_and_refunds_the_last_realm();
        synthetic_value_ownership_real_typed_imports_survive_gc_with_their_original_values();
        synthetic_value_ownership_borrowed_graph_inputs_attach_before_publication();
    }
}
