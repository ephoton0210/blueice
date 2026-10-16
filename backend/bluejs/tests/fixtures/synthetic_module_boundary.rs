// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small synthetic modules preserve resource refusals at their GC boundary.

use super::*;

fn vm_with_sources() -> Vm {
    let mut vm = Vm {
        remaining_instructions: 100,
        ..Vm::default()
    };
    vm.set_json_module_sources(HashMap::from([("value".into(), "{\"answer\":42}".into())]));
    vm.set_bytes_module_sources(HashMap::from([("value".into(), vec![1, 2, 3])]));
    vm.buffer_prototype("ArrayBuffer").unwrap();
    vm.buffer_prototype("Uint8Array").unwrap();
    vm
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_root_refusal_returns_the_heap_error_without_publication() {
    for (module_type, synthetic_type) in [
        (ModuleType::Json, SyntheticModuleType::Json),
        (ModuleType::Bytes, SyntheticModuleType::Bytes),
    ] {
        let mut vm = vm_with_sources();
        let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
        let mut roots = ModuleRoots::default();
        vm.heap.allow_root_registrations(0);
        assert_eq!(
            vm.ensure_synthetic_module(
                &module_type.module_key("value"),
                synthetic_type,
                &mut modules,
                &mut roots,
            ),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        assert!(modules.is_empty() && roots.is_empty());
    }
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_json_instruction_refusal_is_not_a_resolution_error() {
    let mut vm = vm_with_sources();
    vm.remaining_instructions = 0;
    let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
    let mut roots = ModuleRoots::default();
    assert_eq!(
        vm.ensure_synthetic_module(
            &ModuleType::Json.module_key("value"),
            SyntheticModuleType::Json,
            &mut modules,
            &mut roots,
        ),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(modules.is_empty() && roots.is_empty() && vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_json_string_refusal_is_not_a_resolution_error() {
    let mut vm = vm_with_sources();
    vm.config.max_string_bytes = 2;
    vm.set_json_module_sources(HashMap::from([("value".into(), "\"ab\"".into())]));
    let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
    let mut roots = ModuleRoots::default();
    assert_eq!(
        vm.ensure_synthetic_module(
            &ModuleType::Json.module_key("value"),
            SyntheticModuleType::Json,
            &mut modules,
            &mut roots,
        ),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert!(modules.is_empty() && roots.is_empty() && vm.stack.is_empty());
}

fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_refusal_preserves_existing_modules_roots_and_stack() {
    let mut vm = vm_with_sources();
    let caller = vm.heap.alloc_object(None).unwrap();
    let caller_root = vm.heap.root(caller).unwrap();
    let mut roots = ModuleRoots::default();
    roots
        .prepare_slot(vm.host_scheduling.as_ref())
        .unwrap()
        .commit(&mut roots, caller_root);
    let mut modules = ModuleRegistryDraft::new(
        ModuleRegistry::new(
            HashMap::from([("retained".into(), Bytecode::empty())]),
            None,
        )
        .unwrap(),
    );
    vm.stack.push(Value::Number(7.0));
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.ensure_synthetic_module(
            &ModuleType::Json.module_key("value"),
            SyntheticModuleType::Json,
            &mut modules,
            &mut roots,
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(roots.snapshot(), vec![caller_root]);
    assert_eq!(modules.len(), 1);
    assert!(modules.contains_key("retained"));
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    collect(&mut vm);
    assert!(vm.heap.contains(caller));
    assert_eq!(vm.heap.unroot(caller_root), Ok(caller));
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_objects_live_until_their_synthesis_roots_are_released() {
    for (module_type, synthetic_type) in [
        (ModuleType::Json, SyntheticModuleType::Json),
        (ModuleType::Bytes, SyntheticModuleType::Bytes),
    ] {
        let mut vm = vm_with_sources();
        let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
        let mut roots = ModuleRoots::default();
        let key = module_type.module_key("value");
        vm.ensure_synthetic_module(&key, synthetic_type, &mut modules, &mut roots)
            .unwrap();
        let value = modules[&key]
            .synthetic_default_export
            .as_ref()
            .unwrap()
            .value()
            .clone();
        let object = value.object_id().unwrap();
        assert_eq!(roots.len(), 1);
        collect(&mut vm);
        let (property, expected) = match module_type {
            ModuleType::Json => ("answer", 42.0),
            _ => ("0", 1.0),
        };
        assert_eq!(
            vm.get_property(&value, &property.into()),
            Ok(Value::Number(expected))
        );
        vm.heap.unroot(roots.pop().unwrap()).unwrap();
        drop(modules);
        collect(&mut vm);
        assert!(!vm.heap.contains(object));
    }
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_primitives_need_no_object_root_registration() {
    for (source, expected) in [
        ("null", Value::Null),
        ("42", Value::Number(42.0)),
        ("\"ab\"", Value::String("ab".into())),
    ] {
        let mut vm = vm_with_sources();
        vm.set_json_module_sources(HashMap::from([("value".into(), source.into())]));
        vm.set_text_module_sources(HashMap::from([("value".into(), "text".into())]));
        vm.heap.allow_root_registrations(0);
        let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
        let mut roots = ModuleRoots::default();
        for (module_type, synthetic_type, expected) in [
            (ModuleType::Json, SyntheticModuleType::Json, expected),
            (
                ModuleType::Text,
                SyntheticModuleType::Text,
                Value::String("text".into()),
            ),
        ] {
            let key = module_type.module_key("value");
            vm.ensure_synthetic_module(&key, synthetic_type, &mut modules, &mut roots)
                .unwrap();
            assert_eq!(
                modules[&key]
                    .synthetic_default_export
                    .as_ref()
                    .map(|value| value.value()),
                Some(&expected)
            );
        }
        assert!(roots.is_empty());
    }
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_existing_records_remain_idempotent_after_root_refusal() {
    let mut vm = vm_with_sources();
    let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
    let mut roots = ModuleRoots::default();
    let key = ModuleType::Json.module_key("value");
    vm.ensure_synthetic_module(&key, SyntheticModuleType::Json, &mut modules, &mut roots)
        .unwrap();
    let original = modules[&key]
        .synthetic_default_export
        .as_ref()
        .unwrap()
        .value()
        .clone();
    let root = *roots.iter().next().unwrap();
    vm.heap.allow_root_registrations(0);
    vm.ensure_synthetic_module(&key, SyntheticModuleType::Json, &mut modules, &mut roots)
        .unwrap();
    assert_eq!(
        modules[&key]
            .synthetic_default_export
            .as_ref()
            .map(|value| value.value()),
        Some(&original)
    );
    assert_eq!(roots.snapshot(), vec![root]);
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_malformed_and_missing_sources_keep_resolution_semantics() {
    let mut vm = vm_with_sources();
    vm.set_json_module_sources(HashMap::from([("value".into(), "{".into())]));
    let mut modules = ModuleRegistryDraft::new(ModuleRegistry::default());
    let mut roots = ModuleRoots::default();
    assert_eq!(
        vm.ensure_synthetic_module(
            &ModuleType::Json.module_key("value"),
            SyntheticModuleType::Json,
            &mut modules,
            &mut roots,
        ),
        Err(RuntimeError::ModuleResolution(
            "invalid JSON module value: SyntaxError: invalid JSON text".into()
        ))
    );
    for (module_type, synthetic_type, kind) in [
        (ModuleType::Json, SyntheticModuleType::Json, "JSON"),
        (ModuleType::Text, SyntheticModuleType::Text, "text"),
        (ModuleType::Bytes, SyntheticModuleType::Bytes, "bytes"),
    ] {
        assert_eq!(
            vm.ensure_synthetic_module(
                &module_type.module_key("missing"),
                synthetic_type,
                &mut modules,
                &mut roots,
            ),
            Err(RuntimeError::TypeError(format!(
                "host did not provide a {kind} module source for missing"
            )))
        );
    }
    assert!(modules.is_empty() && roots.is_empty());
}

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_fresh_graph_refusal_releases_earlier_synthesis_roots() {
    let mut vm = vm_with_sources();
    vm.set_json_module_sources(HashMap::from([
        ("a".into(), "{}".into()),
        ("b".into(), "{}".into()),
    ]));
    let modules = HashMap::from([(
        "main".into(),
        module("import a from './a' with {type:'json'}; import b from './b' with {type:'json'}; globalThis.ran=true;"),
    )]);
    collect(&mut vm);
    let before = vm.heap.stats();
    vm.heap.allow_root_registrations(1);
    assert_eq!(
        vm.execute_module_graph("main", &modules),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.module_graph.is_none() && vm.module_registry.is_empty());
    assert_eq!(vm.lookup_global_name("ran").unwrap(), None);
    collect(&mut vm);
    let after = vm.heap.stats();
    assert_eq!(
        after.nursery_objects + after.tenured_objects,
        before.nursery_objects + before.tenured_objects
    );
    assert_eq!(after.root_registrations, before.root_registrations + 1);
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_existing_graph_keeps_its_roots_after_refused_import() {
    let mut vm = vm_with_sources();
    let modules = HashMap::from([("main".into(), module("export const answer=42;"))]);
    vm.execute_module_graph("main", &modules).unwrap();
    let graph = vm.module_graph.as_ref().unwrap();
    let original_roots = graph.roots.snapshot();
    let original_cells = graph.linked["main"].cells.clone();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.execute_module_graph_inner(
            &ModuleType::Json.module_key("value"),
            &modules,
            false,
            true,
            ImportPhase::Evaluation,
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    let graph = vm.module_graph.as_ref().unwrap();
    assert_eq!(graph.roots.snapshot(), original_roots);
    assert_eq!(graph.linked.len(), 1);
    assert_eq!(graph.linked["main"].cells, original_cells);
    collect(&mut vm);
    for cell in original_cells.values() {
        assert_eq!(vm.heap.get(*cell, "value"), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn synthetic_module_boundary_graph_instruction_refusal_precedes_module_evaluation() {
    let mut vm = vm_with_sources();
    vm.config.instruction_budget = 0;
    let modules = HashMap::from([(
        "main".into(),
        module("import value from './value' with {type:'json'}; globalThis.ran=true;"),
    )]);
    assert_eq!(
        vm.execute_module_graph("main", &modules),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(vm.module_graph.is_none() && vm.module_registry.is_empty());
    assert_eq!(vm.lookup_global_name("ran").unwrap(), None);
}

#[cfg_attr(test, test)]
fn module_root_ownership_primitive_json_needs_no_root_reservation() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.set_json_module_sources(HashMap::from([("value".into(), "1".into())]));
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let probe = vm.prepare_synthetic_value(0).unwrap();
    let header = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(probe);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - header)
        .unwrap();
    let mut roots = ModuleRoots::default();
    let code = vm
        .create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots)
        .unwrap();
    assert_eq!(
        code.synthetic_default_export.as_ref().unwrap().value(),
        &Value::Number(1.0)
    );
    assert!(roots.is_empty());
    assert_eq!(roots.retained_bytes(), 0);
}

#[cfg_attr(test, test)]
fn module_root_ownership_synthetic_object_refusal_precedes_managed_construction() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.set_json_module_sources(HashMap::from([("value".into(), "{}".into())]));
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let probe = ModuleRoots::default();
    let slot = probe.prepare_slot(vm.host_scheduling.as_ref()).unwrap();
    let bytes = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(slot);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes + 1)
        .unwrap();
    vm.heap.allow_only(0);
    let mut roots = ModuleRoots::default();
    assert!(matches!(
        vm.create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(roots.is_empty());
}

impl crate::Vm {
    /// Runs finite root-admission controls at the synthetic-module boundary.
    #[doc(hidden)]
    pub fn verify_module_root_synthetic_contracts() {
        module_root_ownership_primitive_json_needs_no_root_reservation();
        module_root_ownership_synthetic_object_refusal_precedes_managed_construction();
    }
}

impl crate::Vm {
    /// Runs finite synthetic-module refusal contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_synthetic_module_boundary_contracts() {
        synthetic_module_boundary_root_refusal_returns_the_heap_error_without_publication();
        synthetic_module_boundary_json_instruction_refusal_is_not_a_resolution_error();
        synthetic_module_boundary_json_string_refusal_is_not_a_resolution_error();
        synthetic_module_boundary_refusal_preserves_existing_modules_roots_and_stack();
        synthetic_module_boundary_objects_live_until_their_synthesis_roots_are_released();
        synthetic_module_boundary_primitives_need_no_object_root_registration();
        synthetic_module_boundary_existing_records_remain_idempotent_after_root_refusal();
        synthetic_module_boundary_malformed_and_missing_sources_keep_resolution_semantics();
        synthetic_module_boundary_fresh_graph_refusal_releases_earlier_synthesis_roots();
        synthetic_module_boundary_existing_graph_keeps_its_roots_after_refused_import();
        synthetic_module_boundary_graph_instruction_refusal_precedes_module_evaluation();
    }
}
