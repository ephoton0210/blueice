// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small export graphs verify resolver scratch and result ownership.

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
#[test]
fn export_resolution_storage_local_binding_borrows_without_allocating() {
    let modules = HashMap::from([("root".into(), compile("export const x=1;"))]);
    let (result, allocations) =
        allocation_observer::count(|| Vm::resolve_export(&modules, "root", "x", None));
    assert_eq!(allocations, 0);
    assert!(
        matches!(result, Ok(ExportResolution::Binding { module, slot: 0 }) if module.as_str()=="root")
    );
}
#[cfg(test)]
#[test]
fn export_resolution_storage_indirect_refusal_precedes_query_and_request_scratch() {
    let modules = HashMap::from([
        ("root".into(), compile("export {x} from 'leaf';")),
        ("leaf".into(), compile("export const x=1;")),
    ]);
    let mut vm = page();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let (result, allocations) = allocation_observer::count(|| {
        Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref())
    });
    assert_eq!(allocations, 0);
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
}

fn owned_target(source: &str, name: &str) {
    let modules = HashMap::from([("root".into(), compile(source))]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let result = Vm::resolve_export(&modules, "root", name, vm.host_scheduling.as_ref()).unwrap();
    let module = match &result {
        ExportResolution::Namespace { module }
        | ExportResolution::DeferredNamespace { module }
        | ExportResolution::Source { module } => module,
        _ => panic!("expected a namespace/source resolution"),
    };
    assert_eq!(module.as_str(), "missing");
    assert!(vm.host_scheduling_snapshot().payload_bytes >= before + module.len());
    drop(result);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn export_resolution_storage_namespace_result_retains_its_native_owner() {
    owned_target("export * as ns from 'missing';", "ns");
}
#[cfg_attr(test, test)]
fn export_resolution_storage_deferred_result_retains_its_native_owner() {
    owned_target("import defer * as ns from 'missing'; export {ns};", "ns");
}
#[cfg_attr(test, test)]
fn export_resolution_storage_source_result_retains_its_native_owner() {
    owned_target("import source blob from 'missing'; export {blob};", "blob");
}

fn graph(entries: &[(&str, &str)]) -> HashMap<String, Bytecode> {
    entries
        .iter()
        .map(|(name, source)| ((*name).to_owned(), compile(source)))
        .collect()
}
fn target<'a, 'r>(result: &'r ExportResolution<'a>) -> &'r ResolvedModule<'a> {
    match result {
        ExportResolution::Binding { module, .. }
        | ExportResolution::Namespace { module }
        | ExportResolution::DeferredNamespace { module }
        | ExportResolution::Source { module } => module,
        _ => panic!("expected a resolved target"),
    }
}
fn owned_bytes(result: &ExportResolution<'_>) -> usize {
    match target(result) {
        ResolvedModule::Known(_) => 0,
        ResolvedModule::Owned(name) => name.bytes(),
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_fast_results_succeed_at_full_healthy_quota() {
    let modules = graph(&[("root", "export const x=1;")]);
    let mut vm = page();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    for name in ["x", "default", "absent"] {
        #[cfg(test)]
        let result = {
            let (result, allocations) = allocation_observer::count(|| {
                Vm::resolve_export(&modules, "root", name, vm.host_scheduling.as_ref())
            });
            assert_eq!(allocations, 0);
            result
        };
        #[cfg(not(test))]
        let result = Vm::resolve_export(&modules, "root", name, vm.host_scheduling.as_ref());
        let result = result.unwrap();
        if name == "x" {
            assert!(matches!(result, ExportResolution::Binding { slot: 0, .. }));
        } else {
            assert_eq!(result, ExportResolution::Missing);
        }
    }
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn export_resolution_storage_binding_borrows_canonical_key_after_inputs_drop() {
    let mut key = String::with_capacity(64);
    key.push_str("root");
    let modules = HashMap::from([(key, compile("export const x=1;"))]);
    let vm = page();
    let input_module = String::from("root");
    let input_name = String::from("x");
    let result = Vm::resolve_export(
        &modules,
        &input_module,
        &input_name,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    drop(input_module);
    drop(input_name);
    assert_eq!(
        target(&result).as_str().as_ptr(),
        modules.get_key_value("root").unwrap().0.as_ptr()
    );
    assert_eq!(owned_bytes(&result), 0);
}

#[cfg_attr(test, test)]
fn export_resolution_storage_renamed_queries_reenter_the_same_module() {
    let modules = graph(&[
        ("root", "export {q as x} from 'leaf'; export const local=1;"),
        ("leaf", "export {local as q} from 'root';"),
    ]);
    let vm = page();
    let result = Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref()).unwrap();
    assert!(matches!(&result, ExportResolution::Binding { slot: 0, .. }));
    assert_eq!(target(&result).as_str(), "root");
}

#[cfg_attr(test, test)]
fn export_resolution_storage_indirect_and_star_cycles_return_missing() {
    for modules in [
        graph(&[
            ("root", "export {x} from 'leaf';"),
            ("leaf", "export {x} from 'root';"),
        ]),
        graph(&[
            ("root", "export * from 'leaf';"),
            ("leaf", "export * from 'root';"),
        ]),
    ] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        assert_eq!(
            Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref()).unwrap(),
            ExportResolution::Missing
        );
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_diamond_paths_keep_the_same_binding_unambiguous() {
    let modules = graph(&[
        ("root", "export * from 'left'; export * from 'right';"),
        ("left", "export {x} from 'leaf';"),
        ("right", "export {x} from 'leaf';"),
        ("leaf", "export const x=1;"),
    ]);
    let vm = page();
    let result = Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref()).unwrap();
    assert!(matches!(&result, ExportResolution::Binding { slot: 0, .. }));
    assert_eq!(target(&result).as_str(), "leaf");
}

#[cfg_attr(test, test)]
fn export_resolution_storage_different_slots_and_modules_are_ambiguous() {
    for modules in [
        graph(&[
            ("root", "export * from 'left'; export * from 'right';"),
            ("left", "export {a as x} from 'leaf';"),
            ("right", "export {b as x} from 'leaf';"),
            ("leaf", "export const a=1,b=2;"),
        ]),
        graph(&[
            ("root", "export * from 'left'; export * from 'right';"),
            ("left", "export const x=1;"),
            ("right", "export const x=2;"),
        ]),
    ] {
        let vm = page();
        assert_eq!(
            Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref()).unwrap(),
            ExportResolution::Ambiguous
        );
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_explicit_and_default_queries_precede_star_errors() {
    let modules = graph(&[("root", "export * from 'missing'; export const x=1;")]);
    let vm = page();
    assert!(matches!(
        Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref()).unwrap(),
        ExportResolution::Binding { .. }
    ));
    assert_eq!(
        Vm::resolve_export(&modules, "root", "default", vm.host_scheduling.as_ref()).unwrap(),
        ExportResolution::Missing
    );
}

#[cfg_attr(test, test)]
fn export_resolution_storage_equal_owned_namespace_candidates_refund_duplicates() {
    let modules = graph(&[
        ("t/root", "export * from './left'; export * from './right';"),
        ("t/left", "export * as ns from './data';"),
        ("t/right", "export * as ns from './x/../data';"),
    ]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let result = Vm::resolve_export(&modules, "t/root", "ns", vm.host_scheduling.as_ref()).unwrap();
    assert!(matches!(&result, ExportResolution::Namespace { .. }));
    assert_eq!(target(&result).as_str(), "t/data");
    assert!(owned_bytes(&result) > 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        owned_bytes(&result)
    );
    drop(result);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn export_resolution_storage_namespace_deferred_and_source_kinds_stay_distinct() {
    for source in [
        "import defer * as ns from 'data'; export {ns};",
        "import source ns from 'data'; export {ns};",
    ] {
        let modules = graph(&[
            ("root", "export * from 'left'; export * from 'right';"),
            ("left", "export * as ns from 'data';"),
            ("right", source),
        ]);
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        assert_eq!(
            Vm::resolve_export(&modules, "root", "ns", vm.host_scheduling.as_ref()).unwrap(),
            ExportResolution::Ambiguous
        );
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_source_targets_ignore_type_qualification() {
    let modules = graph(&[
        (
            "t/root",
            "import source blob from './resource' with {type:'json'}; export {blob};",
        ),
        ("t/resource", ""),
        ("t/resource\0json", "export default 1;"),
    ]);
    let vm = page();
    let result =
        Vm::resolve_export(&modules, "t/root", "blob", vm.host_scheduling.as_ref()).unwrap();
    assert!(matches!(&result, ExportResolution::Source { .. }));
    assert_eq!(target(&result).as_str(), "t/resource");
    assert_eq!(owned_bytes(&result), 0);
}

#[cfg_attr(test, test)]
fn export_resolution_storage_typed_indirect_requests_borrow_qualified_keys() {
    let modules = graph(&[
        (
            "t/root",
            "export {default as x} from './data' with {type:'json'};",
        ),
        ("t/data\0json", "export default 1;"),
        ("t/data", "export const wrong=2;"),
    ]);
    let vm = page();
    let result = Vm::resolve_export(&modules, "t/root", "x", vm.host_scheduling.as_ref()).unwrap();
    assert!(matches!(&result, ExportResolution::Binding { slot: 0, .. }));
    assert_eq!(target(&result).as_str(), "t/data\0json");
    assert_eq!(
        target(&result).as_str().as_ptr(),
        modules.get_key_value("t/data\0json").unwrap().0.as_ptr()
    );
}

#[cfg_attr(test, test)]
fn export_resolution_storage_source_ordered_and_invalid_errors_refund_scratch() {
    for modules in [
        graph(&[(
            "t/root",
            "export * from './first'; export * from './second';",
        )]),
        graph(&[("t/root", "export {x} from './first';")]),
    ] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        assert_eq!(
            Vm::resolve_export(&modules, "t/root", "x", vm.host_scheduling.as_ref()).err(),
            Some(RuntimeError::ModuleResolution(
                "module t/first was not supplied by the host".into()
            ))
        );
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
    let modules = graph(&[("root", "export * from '../outside';")]);
    let vm = page();
    for module in ["root", "absent"] {
        assert!(matches!(
            Vm::resolve_export(&modules, module, "x", vm.host_scheduling.as_ref()),
            Err(RuntimeError::ModuleResolution(_))
        ));
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_numeric_refusals_release_partial_walks() {
    let modules = graph(&[
        ("root", "export * from 'middle';"),
        ("middle", "export {x} from 'leaf';"),
        ("leaf", "export const x=1;"),
    ]);
    let mut failures = 0;
    let mut successes = 0;
    for spare in [0, 1, 64, 128, 512, 1024, 2048] {
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
        match Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref()) {
            Ok(result) => {
                assert!(matches!(result, ExportResolution::Binding { slot: 0, .. }));
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
fn export_resolution_storage_terminal_failure_precedes_even_fast_results() {
    let modules = graph(&[("root", "export const x=1;")]);
    let vm = page();
    vm.host_scheduling.as_ref().unwrap().fail();
    #[cfg(test)]
    let result = {
        let (result, allocations) = allocation_observer::count(|| {
            Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref())
        });
        assert_eq!(allocations, 0);
        result
    };
    #[cfg(not(test))]
    let result = Vm::resolve_export(&modules, "root", "x", vm.host_scheduling.as_ref());
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn export_resolution_storage_owned_targets_admit_exact_and_refuse_short_budgets() {
    for source in [
        "export * as ns from 'missing';",
        "import defer * as ns from 'missing'; export {ns};",
        "import source ns from 'missing'; export {ns};",
    ] {
        let modules = graph(&[("root", source)]);
        for short in [false, true] {
            let mut vm = page();
            let used = vm.host_scheduling_usage().unwrap().payload_bytes;
            let preview =
                Vm::resolve_export(&modules, "root", "ns", vm.host_scheduling.as_ref()).unwrap();
            let bytes = owned_bytes(&preview);
            drop(preview);
            let _peer = vm
                .host_scheduling
                .as_ref()
                .unwrap()
                .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes + usize::from(short))
                .unwrap();
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let result = Vm::resolve_export(&modules, "root", "ns", vm.host_scheduling.as_ref());
            if short {
                assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
            } else {
                let result = result.unwrap();
                assert_eq!(owned_bytes(&result), bytes);
                assert_eq!(
                    vm.host_scheduling_snapshot().payload_bytes,
                    MAX_HOST_SCHEDULING_BYTES
                );
                drop(result);
            }
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_owned_results_survive_vm_destruction() {
    for source in [
        "export * as ns from 'missing';",
        "import defer * as ns from 'missing'; export {ns};",
        "import source ns from 'missing'; export {ns};",
    ] {
        let modules = graph(&[("root", source)]);
        let vm = page();
        let observer = Vm {
            host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
            ..Vm::default()
        };
        let result =
            Vm::resolve_export(&modules, "root", "ns", vm.host_scheduling.as_ref()).unwrap();
        let bytes = owned_bytes(&result);
        drop(vm);
        assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
        assert_eq!(target(&result).as_str(), "missing");
        drop(result);
        assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_known_targets_borrow_host_keys() {
    for source in [
        "export * as ns from 'data';",
        "import defer * as ns from 'data'; export {ns};",
        "import source ns from 'data'; export {ns};",
    ] {
        let modules = graph(&[("root", source), ("data", "")]);
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let result =
            Vm::resolve_export(&modules, "root", "ns", vm.host_scheduling.as_ref()).unwrap();
        assert_eq!(owned_bytes(&result), 0);
        assert_eq!(
            target(&result).as_str().as_ptr(),
            modules.get_key_value("data").unwrap().0.as_ptr()
        );
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn export_resolution_storage_real_named_imports_keep_live_binding_updates() {
    let modules = graph(&[
        (
            "root",
            "import {x,bump} from 'middle'; bump(); export const value=x;",
        ),
        ("middle", "export {x,bump} from 'leaf';"),
        ("leaf", "export let x=1; export function bump(){x+=1;}"),
    ]);
    let mut vm = page();
    vm.execute_module_graph("root", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"value".into()),
        Ok(Value::Number(2.0))
    );
}

#[cfg_attr(test, test)]
fn export_resolution_storage_real_ambiguous_imports_fail_before_evaluation() {
    let modules = graph(&[
        (
            "root",
            "import {x} from 'middle'; evaluated+=1; export {x};",
        ),
        ("middle", "export * from 'left'; export * from 'right';"),
        ("left", "export const x=1;"),
        ("right", "export const x=2;"),
    ]);
    let mut vm = page();
    let setup = crate::compile(&crate::parse("var evaluated=0;").unwrap()).unwrap();
    vm.execute_script(&setup).unwrap();
    assert!(matches!(
        vm.execute_module_graph("root", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
    let check = crate::compile(&crate::parse("evaluated").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&check), Ok(Value::Number(0.0)));
}

#[cfg_attr(test, test)]
fn export_resolution_storage_later_host_failure_refunds_an_owned_candidate() {
    let modules = graph(&[
        ("root", "export * from 'left'; export * from 'absent';"),
        ("left", "export * as ns from 'data';"),
    ]);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        Vm::resolve_export(&modules, "root", "ns", vm.host_scheduling.as_ref()).err(),
        Some(RuntimeError::ModuleResolution(
            "module absent was not supplied by the host".into()
        ))
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
    let result = Vm::resolve_export(&modules, "left", "ns", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(target(&result).as_str(), "data");
}

impl crate::Vm {
    /// Runs finite export resolver admission, semantics and result contracts.
    #[doc(hidden)]
    pub fn verify_export_resolution_storage_contracts() {
        export_resolution_storage_namespace_result_retains_its_native_owner();
        export_resolution_storage_deferred_result_retains_its_native_owner();
        export_resolution_storage_source_result_retains_its_native_owner();
        export_resolution_storage_fast_results_succeed_at_full_healthy_quota();
        export_resolution_storage_binding_borrows_canonical_key_after_inputs_drop();
        export_resolution_storage_renamed_queries_reenter_the_same_module();
        export_resolution_storage_indirect_and_star_cycles_return_missing();
        export_resolution_storage_diamond_paths_keep_the_same_binding_unambiguous();
        export_resolution_storage_different_slots_and_modules_are_ambiguous();
        export_resolution_storage_explicit_and_default_queries_precede_star_errors();
        export_resolution_storage_equal_owned_namespace_candidates_refund_duplicates();
        export_resolution_storage_namespace_deferred_and_source_kinds_stay_distinct();
        export_resolution_storage_source_targets_ignore_type_qualification();
        export_resolution_storage_typed_indirect_requests_borrow_qualified_keys();
        export_resolution_storage_source_ordered_and_invalid_errors_refund_scratch();
        export_resolution_storage_numeric_refusals_release_partial_walks();
        export_resolution_storage_terminal_failure_precedes_even_fast_results();
        export_resolution_storage_owned_targets_admit_exact_and_refuse_short_budgets();
        export_resolution_storage_owned_results_survive_vm_destruction();
        export_resolution_storage_known_targets_borrow_host_keys();
        export_resolution_storage_real_named_imports_keep_live_binding_updates();
        export_resolution_storage_real_ambiguous_imports_fail_before_evaluation();
        export_resolution_storage_later_host_failure_refunds_an_owned_candidate();
    }
}
