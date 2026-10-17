// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite static synthetic requests retain their complete staging ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
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
fn graph(source: &str) -> HashMap<String, Bytecode> {
    HashMap::from([(
        "dir/main.mjs".into(),
        crate::compile_module(&crate::parse_module(source).unwrap()).unwrap(),
    )])
}

#[cfg(test)]
#[test]
fn synthetic_request_storage_names_refuse_before_sort_buffer_allocation() {
    let mut vm = page();
    let modules = graph("export const answer = 42;");
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref())
    });
    assert_eq!(allocations, 0);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
}

#[cfg(test)]
#[test]
fn synthetic_request_storage_requests_refuse_before_target_allocations() {
    let mut vm = page();
    let modules = graph("import value from './value.txt' with {type:'text'};");
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref())
    });
    assert_eq!(allocations, 0);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_retained_targets_join_document_usage() {
    let vm = page();
    let modules = graph("import value from './value.txt' with {type:'text'};");
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    let bytes = targets.bytes();
    assert!(bytes > 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes - before, bytes);
    drop(targets);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_detached_targets_keep_ownership_after_vm_drop() {
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let modules = graph("import value from './value.txt' with {type:'text'};");
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    let bytes = targets.bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(targets);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

fn code(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn names(targets: &request_storage::Requests) -> Vec<&str> {
    targets.iter().map(|(name, _)| name).collect()
}
fn scratch_bytes(modules: &HashMap<String, Bytecode>) -> usize {
    let mut scratch = crate::vm::module_walk_storage::Storage::<&String>::default();
    scratch.reserve(modules.len(), None).unwrap();
    scratch.bytes()
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_empty_graph_needs_no_room() {
    let mut vm = page();
    let _peer = leave_room(&mut vm, 0);
    let targets =
        request_storage::Requests::collect(&HashMap::new(), vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(targets.bytes(), 0);
    assert_eq!(targets.iter().count(), 0);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_terminal_document_refuses_even_empty_graph() {
    let vm = page();
    vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        request_storage::Requests::collect(&HashMap::new(), vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    ));
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_ordinary_and_source_requests_do_not_become_targets() {
    let vm = page();
    let mut module = code("import source blob from './blob.wasm'; import './code.mjs'; export {blob}; export * from './other.mjs';");
    // Even a host-supplied synthetic type cannot turn a source import into an
    // executable synthetic record. The same source re-export stays excluded.
    module.module_imports[0].module_type = ModuleType::Text;
    let modules = HashMap::from([("dir/main".into(), module)]);
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(targets.iter().count(), 0);
    assert_eq!(targets.bytes(), 0);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_canonical_modules_keep_import_then_export_order() {
    let vm = page();
    let modules = HashMap::from([
        ("z/main".into(), code("import './z.txt' with {type:'text'};")),
        ("a/main".into(), code("import './a.json' with {type:'json'}; import './a.bin' with {type:'bytes'}; export * from './a.txt' with {type:'text'};")),
    ]);
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        names(&targets),
        [
            "a/a.json\0json",
            "a/a.bin\0bytes",
            "a/a.txt\0text",
            "z/z.txt\0text"
        ]
    );
    let kinds: Vec<_> = targets
        .iter()
        .map(|(_, kind)| match kind {
            SyntheticModuleType::Json => 0,
            SyntheticModuleType::Text => 1,
            SyntheticModuleType::Bytes => 2,
        })
        .collect();
    assert_eq!(kinds, [0, 2, 1, 1]);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_all_reexport_routes_are_selected() {
    let vm = page();
    let mut module = Bytecode::empty();
    module.module_exports = vec![
        ModuleExport::Indirect {
            export_name: "one".into(),
            module_request: "./one".into(),
            import_name: "default".into(),
            module_type: ModuleType::Json,
        },
        ModuleExport::Star {
            module_request: "./two".into(),
            module_type: ModuleType::Text,
        },
        ModuleExport::Namespace {
            export_name: "three".into(),
            module_request: "./three".into(),
            module_type: ModuleType::Bytes,
        },
        ModuleExport::DeferredNamespace {
            export_name: "four".into(),
            module_request: "./four".into(),
            module_type: ModuleType::Text,
        },
        ModuleExport::Local {
            export_name: "local".into(),
            local_slot: 0,
        },
        ModuleExport::Source {
            export_name: "source".into(),
            module_request: "./blob".into(),
        },
    ];
    let modules = HashMap::from([("dir/main".into(), module)]);
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        names(&targets),
        [
            "dir/one\0json",
            "dir/two\0text",
            "dir/three\0bytes",
            "dir/four\0text"
        ]
    );
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_repeated_requests_preserve_occurrences() {
    let vm = page();
    let modules = graph("import a from './value.txt' with {type:'text'}; import b from './value.txt' with {type:'text'}; export {default as c} from './value.txt' with {type:'text'};");
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(names(&targets), ["dir/value.txt\0text"; 3]);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_exact_peak_succeeds_and_one_short_refuses() {
    let modules =
        graph("import './value.txt' with {type:'text'}; import './other.json' with {type:'json'};");
    let bytes = request_storage::Requests::collect(&modules, None)
        .unwrap()
        .bytes();
    let peak = scratch_bytes(&modules) + bytes;
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref());
        if accepts {
            let targets = result.unwrap();
            assert_eq!(targets.bytes(), bytes);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES - peak + bytes
            );
            drop(targets);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_target_backing_precedes_resolution() {
    let mut vm = page();
    let mut modules = graph("import './value.txt' with {type:'text'};");
    modules.get_mut("dir/main.mjs").unwrap().module_imports[0].module_request =
        "bad\0request".into();
    let _peer = leave_room(&mut vm, scratch_bytes(&modules));
    assert!(matches!(
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    ));
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_resolution_failure_refunds_partial_batch() {
    let vm = page();
    let modules = graph(
        "import './value.txt' with {type:'text'}; export * from '../../escape' with {type:'text'};",
    );
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(
        matches!(request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()), Err(RuntimeError::ModuleResolution(message)) if message.contains("../../escape"))
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_no_target_scan_refunds_name_scratch() {
    let mut vm = page();
    let modules = graph("export const answer = 42;");
    let room = scratch_bytes(&modules);
    let _peer = leave_room(&mut vm, room);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(targets.bytes(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_yielded_keys_survive_batch_destruction() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let modules = graph("import './value.txt' with {type:'text'};");
    let mut targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    let key = targets.drain().next().unwrap().0;
    drop(targets);
    assert_eq!(key.as_str(), "dir/value.txt\0text");
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        key.bytes()
    );
    drop(key);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_abandoned_drain_refunds_remaining_names() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let modules =
        graph("import './value.txt' with {type:'text'}; import './other.txt' with {type:'text'};");
    let mut targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    let bytes = targets.bytes();
    let mut drain = targets.drain();
    drop(drain.next().unwrap());
    drop(drain);
    assert_eq!(targets.iter().count(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        targets.bytes()
    );
    assert!(targets.bytes() < bytes, "all names have been refunded");
    drop(targets);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_collection_preserves_graph_and_heap() {
    let mut vm = page();
    let modules = graph("import './value.txt' with {type:'text'};");
    let capacity = modules.capacity();
    let pointer = modules.keys().next().unwrap().as_ptr();
    vm.heap.allow_only(0);
    let heap = vm.heap.stats();
    let targets =
        request_storage::Requests::collect(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules.capacity(), capacity);
    assert_eq!(modules.keys().next().unwrap().as_ptr(), pointer);
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(names(&targets), ["dir/value.txt\0text"]);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_errors_follow_canonical_module_keys() {
    for reverse in [false, true] {
        let mut vm = page();
        let mut items = [
            (
                "a/main".into(),
                code("import '../../a' with {type:'text'};"),
            ),
            (
                "z/main".into(),
                code("import '../../z' with {type:'text'};"),
            ),
        ];
        if reverse {
            items.reverse();
        }
        let mut modules = ModuleRegistryDraft::new(
            ModuleRegistry::new(items.into_iter().collect(), vm.host_scheduling.as_ref()).unwrap(),
        );
        let mut roots = ModuleRoots::default();
        let heap = vm.heap.stats();
        assert!(
            matches!(vm.register_static_synthetic_modules(&mut modules, &mut roots), Err(RuntimeError::ModuleResolution(message)) if message.contains("../../a"))
        );
        assert_eq!(modules.len(), 2);
        assert!(roots.is_empty());
        assert_eq!(vm.heap.stats(), heap);
    }
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_all_resolution_precedes_record_publication() {
    let mut vm = page();
    vm.set_text_module_sources(HashMap::from([("dir/value.txt".into(), "value".into())]));
    let source = ModuleRegistry::new(graph("import './value.txt' with {type:'text'}; export * from '../../escape' with {type:'json'};"), vm.host_scheduling.as_ref()).unwrap();
    let mut modules = ModuleRegistryDraft::new(source);
    let mut roots = ModuleRoots::default();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    assert!(matches!(
        vm.register_static_synthetic_modules(&mut modules, &mut roots),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(modules.len(), 1);
    assert!(!modules.contains_key("dir/value.txt\0text"));
    assert!(roots.is_empty());
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_missing_source_refunds_batch() {
    let mut vm = page();
    let source = ModuleRegistry::new(
        graph("import './missing.txt' with {type:'text'};"),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let mut modules = ModuleRegistryDraft::new(source);
    let mut roots = ModuleRoots::default();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    assert!(
        matches!(vm.register_static_synthetic_modules(&mut modules, &mut roots), Err(RuntimeError::TypeError(message)) if message.contains("dir/missing.txt"))
    );
    assert_eq!(modules.len(), 1);
    assert!(roots.is_empty());
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_host_neutral_registration_keeps_shared_values() {
    let mut vm = Vm::default();
    vm.set_text_module_sources(HashMap::from([("dir/value.txt".into(), "hello".into())]));
    let mut modules = ModuleRegistryDraft::new(ModuleRegistry::new(graph("import a from './value.txt' with {type:'text'}; import b from './value.txt' with {type:'text'};"), None).unwrap());
    let mut roots = ModuleRoots::default();
    vm.register_static_synthetic_modules(&mut modules, &mut roots)
        .unwrap();
    assert_eq!(modules.len(), 2);
    assert_eq!(
        modules["dir/value.txt\0text"]
            .synthetic_default_export
            .as_ref()
            .unwrap()
            .value(),
        &Value::String("hello".into())
    );
    assert!(roots.is_empty());
    assert!(vm.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn synthetic_request_storage_real_mixed_imports_link_and_preserve_identity() {
    let mut vm = page();
    vm.set_text_module_sources(HashMap::from([("dir/value.txt".into(), "hello".into())]));
    vm.set_json_module_sources(HashMap::from([(
        "dir/value.json".into(),
        "{\"answer\":42}".into(),
    )]));
    vm.set_bytes_module_sources(HashMap::from([("dir/value.bin".into(), vec![7, 8])]));
    let modules = graph("import text from './value.txt' with {type:'text'}; import a from './value.json' with {type:'json'}; import b from './value.json' with {type:'json'}; import bytes from './value.bin' with {type:'bytes'}; export const result=text==='hello' && a===b && a.answer===42 && bytes[0]===7;");
    vm.execute_module_graph("dir/main.mjs", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"result".into()),
        Ok(Value::Bool(true))
    );
}

impl crate::Vm {
    /// Runs finite static synthetic request admission and lifetime contracts.
    #[doc(hidden)]
    pub fn verify_synthetic_request_storage_contracts() {
        synthetic_request_storage_retained_targets_join_document_usage();
        synthetic_request_storage_detached_targets_keep_ownership_after_vm_drop();
        synthetic_request_storage_empty_graph_needs_no_room();
        synthetic_request_storage_terminal_document_refuses_even_empty_graph();
        synthetic_request_storage_ordinary_and_source_requests_do_not_become_targets();
        synthetic_request_storage_canonical_modules_keep_import_then_export_order();
        synthetic_request_storage_all_reexport_routes_are_selected();
        synthetic_request_storage_repeated_requests_preserve_occurrences();
        synthetic_request_storage_exact_peak_succeeds_and_one_short_refuses();
        synthetic_request_storage_target_backing_precedes_resolution();
        synthetic_request_storage_resolution_failure_refunds_partial_batch();
        synthetic_request_storage_no_target_scan_refunds_name_scratch();
        synthetic_request_storage_yielded_keys_survive_batch_destruction();
        synthetic_request_storage_abandoned_drain_refunds_remaining_names();
        synthetic_request_storage_collection_preserves_graph_and_heap();
        synthetic_request_storage_errors_follow_canonical_module_keys();
        synthetic_request_storage_all_resolution_precedes_record_publication();
        synthetic_request_storage_missing_source_refunds_batch();
        synthetic_request_storage_host_neutral_registration_keeps_shared_values();
        synthetic_request_storage_real_mixed_imports_link_and_preserve_identity();
    }
}
