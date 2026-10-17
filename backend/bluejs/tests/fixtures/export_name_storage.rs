// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small export graphs verify admitted namespace name traversal.

use super::*;

fn compile(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
#[cfg(test)]
fn record() -> LinkedModule {
    LinkedModule {
        cells: HashMap::new(),
        cells_charge: Vm::default().charge_module_cells(0),
        namespace: None,
        deferred_namespace: None,
        namespace_epochs: [0; 2],
        evaluated: false,
        evaluating: false,
        suspended: false,
        completion: None,
        error: None,
        host_error_pending: false,
    }
}
#[cfg(test)]
fn refused_names(source: &str) {
    let mut vm = page();
    let modules = HashMap::from([
        ("t/root.mjs".into(), compile(source)),
        ("t/leaf.mjs".into(), compile("export const x=1;")),
    ]);
    let mut linked = HashMap::from([("t/root.mjs".into(), record())]);
    let mut roots = ModuleRoots::default();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let (preview, preparation_allocations) = allocation_observer::count(|| {
        namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "t/root.mjs", false)
    });
    let preview = preview.unwrap();
    let staging = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(preview);
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - before - staging)
        .unwrap();
    let heap = vm.heap.stats();
    let (result, allocations) = allocation_observer::count(|| {
        vm.module_namespace("t/root.mjs", false, &modules, &mut linked, &mut roots)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, preparation_allocations);
    assert_eq!(vm.heap.stats(), heap);
    assert!(roots.is_empty());
    assert!(vm.module_namespace_cache.is_empty());
    assert_eq!(vm.namespace_transaction_depth, 0);
    assert!(linked["t/root.mjs"].namespace.is_none());
}
#[cfg(test)]
#[test]
fn export_name_storage_empty_refusal_precedes_star_set() {
    refused_names("");
}
#[cfg(test)]
#[test]
fn export_name_storage_local_refusal_precedes_name_copies() {
    refused_names("export const x=1;");
}
#[cfg(test)]
#[test]
fn export_name_storage_star_refusal_precedes_request_scratch() {
    refused_names("export * from './leaf.mjs';");
}

#[cfg_attr(test, test)]
fn export_name_storage_collects_all_explicit_forms_without_resolving_them() {
    let modules = HashMap::from([(
        "root".into(),
        compile("export const z=1; export default 2; export {q} from 'missing'; export * as ns from 'missing'; import defer * as late from 'missing'; export {late}; import source src from 'missing.wasm'; export {src};"),
    )]);
    let vm = page();
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["default", "late", "ns", "q", "src", "z"]);
}

#[cfg_attr(test, test)]
fn export_name_storage_filters_star_defaults_but_keeps_the_root_default() {
    let modules = HashMap::from([
        (
            "root".into(),
            compile("export * from 'leaf'; export default 0;"),
        ),
        (
            "leaf".into(),
            compile("export default 1; export const x=2;"),
        ),
    ]);
    let vm = page();
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["default", "x"]);
    let root = HashMap::from([
        ("root".into(), compile("export * from 'leaf';")),
        (
            "leaf".into(),
            compile("export default 1; export const x=2;"),
        ),
    ]);
    let names = Vm::exported_names(&root, "root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["x"]);
}

#[cfg_attr(test, test)]
fn export_name_storage_diamonds_and_cycles_deduplicate_names() {
    let modules = HashMap::from([
        (
            "root".into(),
            compile("export * from 'left'; export * from 'right'; export const root=0;"),
        ),
        (
            "left".into(),
            compile("export * from 'leaf'; export const shared=1;"),
        ),
        (
            "right".into(),
            compile("export * from 'leaf'; export const shared=2;"),
        ),
        (
            "leaf".into(),
            compile("export * from 'root'; export default 3; export const leaf=4;"),
        ),
    ]);
    let vm = page();
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["leaf", "root", "shared"]);
}

#[cfg_attr(test, test)]
fn export_name_storage_uses_source_order_for_missing_star_errors() {
    let modules = HashMap::from([
        (
            "t/root".into(),
            compile("export * from './child'; export * from './second';"),
        ),
        ("t/child".into(), compile("export * from './first';")),
    ]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        Vm::exported_names(&modules, "t/root", vm.host_scheduling.as_ref()).err(),
        Some(RuntimeError::ModuleResolution(
            "module t/first was not supplied by the host".into()
        ))
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn export_name_storage_missing_root_and_invalid_star_preserve_errors() {
    let vm = page();
    let modules = HashMap::from([("root".into(), compile("export * from '../outside';"))]);
    assert_eq!(
        Vm::exported_names(&modules, "absent", vm.host_scheduling.as_ref()).err(),
        Some(RuntimeError::ModuleResolution(
            "module absent was not supplied by the host".into()
        ))
    );
    assert!(matches!(
        Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()),
        Err(RuntimeError::ModuleResolution(_))
    ));
}

#[cfg_attr(test, test)]
fn export_name_storage_numeric_refusals_refund_partial_walks() {
    let modules = HashMap::from([
        (
            "root".into(),
            compile("export const z=0; export * from 'leaf';"),
        ),
        (
            "leaf".into(),
            compile("export const x=1; export const y=2;"),
        ),
    ]);
    let mut failures = 0;
    let mut successes = 0;
    for spare in [0, 1, 64, 128, 192, 256, 512, 1024] {
        let mut vm = page();
        let used = vm.host_scheduling_usage().unwrap().payload_bytes;
        let _peer = vm
            .host_scheduling
            .as_ref()
            .unwrap()
            .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - spare)
            .unwrap();
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let heap = vm.heap.stats();
        match Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()) {
            Ok(names) => {
                assert_eq!(&*names, &["x", "y", "z"]);
                successes += 1;
            }
            Err(error) => {
                assert_eq!(error, RuntimeError::SchedulingLimit);
                failures += 1;
            }
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(vm.heap.stats(), heap);
    }
    assert!(failures > 0 && successes > 0);
}

#[cfg_attr(test, test)]
fn export_name_storage_result_owns_only_its_retained_pointer_vector() {
    let modules = HashMap::from([(
        "root".into(),
        compile("export const z=0; export const a=1;"),
    )]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["a", "z"]);
    assert!(names.bytes() >= 2 * std::mem::size_of::<&str>());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        names.bytes()
    );
    drop(names);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn export_name_storage_result_borrows_original_export_name_buffers() {
    let modules = HashMap::from([(
        "root".into(),
        compile("export const z=0; export const a=1;"),
    )]);
    let vm = page();
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    for &name in names.iter() {
        assert!(modules["root"].module_exports.iter().any(|export| matches!(export, ModuleExport::Local { export_name, .. } if export_name.as_str().as_ptr() == name.as_ptr())));
    }
}

#[cfg_attr(test, test)]
fn export_name_storage_retained_result_survives_vm_destruction() {
    let modules = HashMap::from([("root".into(), compile("export const x=1;"))]);
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    let bytes = names.bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    assert_eq!(&*names, &["x"]);
    drop(names);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn export_name_storage_exclusion_retains_charged_vector_backing() {
    let modules = HashMap::from([(
        "root".into(),
        compile("export const then=1; export const x=2;"),
    )]);
    let vm = page();
    let mut names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let bytes = names.bytes();
    names.exclude("then");
    names.exclude("absent");
    assert_eq!(&*names, &["x"]);
    assert_eq!(names.bytes(), bytes);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn export_name_storage_empty_results_release_all_traversal_backing() {
    let modules = HashMap::from([("root".into(), compile(""))]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    assert!(names.is_empty());
    assert_eq!(names.bytes(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn export_name_storage_terminal_scope_refuses_before_scratch() {
    let modules = HashMap::from([("root".into(), compile("export const x=1;"))]);
    let vm = page();
    vm.host_scheduling.as_ref().unwrap().fail();
    #[cfg(test)]
    let result = {
        let (result, allocations) = allocation_observer::count(|| {
            Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref())
        });
        assert_eq!(allocations, 0);
        result
    };
    #[cfg(not(test))]
    let result = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref());
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn export_name_storage_typed_star_requests_use_qualified_host_keys() {
    let modules = HashMap::from([
        (
            "t/root".into(),
            compile("export * from './data' with {type:'json'};"),
        ),
        (
            "t/data\0json".into(),
            compile("export const json=1; export default 2;"),
        ),
        ("t/data".into(), compile("export const wrong=3;")),
    ]);
    let vm = page();
    let names = Vm::exported_names(&modules, "t/root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["json"]);
}

#[cfg_attr(test, test)]
fn export_name_storage_name_vector_growth_preserves_sorted_distinct_names() {
    let source = "export const a=0,b=1,c=2,d=3,e=4,f=5,g=6,h=7,i=8,j=9;";
    let modules = HashMap::from([("root".into(), compile(source))]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let names = Vm::exported_names(&modules, "root", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(&*names, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        names.bytes()
    );
}

#[cfg_attr(test, test)]
fn export_name_storage_host_neutral_names_preserve_unicode_order() {
    let modules = HashMap::from([(
        "root".into(),
        compile("const x=0; export {x as 'é', x as 'a', x as '💠', x as 'z'};"),
    )]);
    let names = Vm::exported_names(&modules, "root", None).unwrap();
    assert_eq!(&*names, &["a", "z", "é", "💠"]);
}

#[cfg_attr(test, test)]
fn export_name_storage_real_cyclic_namespaces_keep_live_binding_values() {
    let modules = HashMap::from([
        (
            "root".into(),
            compile("export * from 'leaf'; export const a=1;"),
        ),
        (
            "leaf".into(),
            compile("export * from 'root'; export const b=2;"),
        ),
    ]);
    let mut vm = page();
    vm.execute_module_graph("root", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"a".into()),
        Ok(Value::Number(1.0))
    );
    assert_eq!(
        vm.get_property(&namespace, &"b".into()),
        Ok(Value::Number(2.0))
    );
}

impl crate::Vm {
    /// Runs finite export-name traversal and native ownership contracts.
    #[doc(hidden)]
    pub fn verify_export_name_storage_contracts() {
        export_name_storage_collects_all_explicit_forms_without_resolving_them();
        export_name_storage_filters_star_defaults_but_keeps_the_root_default();
        export_name_storage_diamonds_and_cycles_deduplicate_names();
        export_name_storage_uses_source_order_for_missing_star_errors();
        export_name_storage_missing_root_and_invalid_star_preserve_errors();
        export_name_storage_numeric_refusals_refund_partial_walks();
        export_name_storage_result_owns_only_its_retained_pointer_vector();
        export_name_storage_result_borrows_original_export_name_buffers();
        export_name_storage_retained_result_survives_vm_destruction();
        export_name_storage_exclusion_retains_charged_vector_backing();
        export_name_storage_empty_results_release_all_traversal_backing();
        export_name_storage_terminal_scope_refuses_before_scratch();
        export_name_storage_typed_star_requests_use_qualified_host_keys();
        export_name_storage_name_vector_growth_preserves_sorted_distinct_names();
        export_name_storage_host_neutral_names_preserve_unicode_order();
        export_name_storage_real_cyclic_namespaces_keep_live_binding_values();
    }
}
