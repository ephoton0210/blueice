// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny module compilation inputs verify native admission and borrowed ASTs.

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
fn invalid_module() -> crate::Module {
    crate::Module {
        body: crate::parse("eval=1; ({field:'tiny'});").unwrap().body,
        imports: Vec::new(),
        exports: Vec::new(),
        requests: Vec::new(),
    }
}

#[cfg(test)]
#[test]
fn module_compilation_input_module_early_error_does_not_clone_the_body() {
    let module = invalid_module();
    let (result, allocations) = allocation_observer::count(|| crate::compile_module(&module));
    assert_eq!(allocations, 0);
    assert_eq!(
        result.err(),
        Some(crate::CompileError::InvalidSyntax(
            "strict code cannot assign to eval or arguments"
        ))
    );
}

#[cfg(test)]
#[test]
fn module_compilation_input_diagnostic_early_error_precedes_clone_and_ids() {
    let module = crate::BlueJsProgramV1::Module(invalid_module());
    let (result, allocations) =
        allocation_observer::count(|| crate::compiler::compile_for_diagnostics(&module));
    assert_eq!(allocations, 0);
    assert_eq!(
        result.err(),
        Some(crate::CompileError::InvalidSyntax(
            "strict code cannot assign to eval or arguments"
        ))
    );
}

#[cfg(test)]
#[test]
fn module_compilation_input_dynamic_refusal_precedes_parse_and_compile() {
    let mut vm = page();
    vm.set_dynamic_module_sources(HashMap::from([(
        "test/tiny.mjs".into(),
        "export const value=1;".into(),
    )]));
    let mut draft = ModuleRegistryDraft::new(ModuleRegistry::default());
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.ensure_dynamic_module_compiled("test/tiny.mjs", &mut draft)
    });
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!draft.contains_key("test/tiny.mjs"));
}

#[cfg(test)]
#[test]
fn module_compilation_input_empty_source_refusal_precedes_parser_metadata() {
    let mut vm = page();
    vm.set_dynamic_module_sources(HashMap::from([("test/empty.mjs".into(), String::new())]));
    let mut draft = ModuleRegistryDraft::new(ModuleRegistry::default());
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.ensure_dynamic_module_compiled("test/empty.mjs", &mut draft)
    });
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!draft.contains_key("test/empty.mjs"));
}

use crate::vm::module_compilation_input::ParsedModuleInput;

#[cfg_attr(test, test)]
fn module_compilation_input_template_fragment_sources_are_admitted_together() {
    let vm = page();
    let source = "export function f(){return 1;} export const value = `${(()=>2)()}`;";
    let input = ParsedModuleInput::parse(source, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(input.bytes(), input.owned_heap_payload_bytes().unwrap());
    input.compile(vm.host_scheduling.as_ref()).unwrap();
}

#[cfg(test)]
#[test]
fn module_compilation_input_metric_uses_supplied_source_scratch() {
    let module = crate::parse_module("function a(){return 1;} function b(){return a();}").unwrap();
    let expected = module.owned_heap_payload_bytes();
    let mut sources = HashSet::with_capacity(4);
    let (actual, allocations) = allocation_observer::count(|| {
        module.payload_with_source_admission(&mut |identity, _| Some(sources.insert(identity)))
    });
    assert_eq!(actual, expected);
    assert_eq!(allocations, 0);
}

#[cfg_attr(test, test)]
fn module_compilation_input_admitted_metric_preserves_all_payload_kinds() {
    for source in [
        "export const value={text:'tiny', n:17n};",
        "function a(){return 1;} export class C {method(){return a();}}",
        "import {a} from './dep.mjs'; export {a};",
    ] {
        let module = crate::parse_module(source).unwrap();
        let mut sources = HashSet::new();
        assert_eq!(
            module.payload_with_source_admission(&mut |identity, _| Some(sources.insert(identity))),
            module.owned_heap_payload_bytes()
        );
    }
}

#[cfg_attr(test, test)]
fn module_compilation_input_mixed_source_metric_counts_both_allocations() {
    let mut module = crate::parse_module("function a(){return 1;}").unwrap();
    let other = crate::parse_module("function b(){return 2;}").unwrap();
    module.body.extend(other.body);
    assert!(module.owned_heap_payload_bytes().unwrap() > 0);
    let mut sources = HashSet::new();
    assert_eq!(
        module.payload_with_source_admission(&mut |identity, _| Some(sources.insert(identity))),
        module.owned_heap_payload_bytes()
    );
    assert_eq!(sources.len(), 2);
}

#[cfg_attr(test, test)]
fn module_compilation_input_parsed_owner_counts_actual_retained_capacity() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let input = ParsedModuleInput::parse(
        "export function outer(x){return ()=>x;}",
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let expected = input.owned_heap_payload_bytes().unwrap();
    assert_eq!(input.bytes(), expected);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        expected
    );
    drop(input);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_compilation_input_ast_owner_outlives_its_vm() {
    let vm = page();
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let input = ParsedModuleInput::parse(
        "export function outer(x){return ()=>x;}",
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let bytes = input.bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_compilation_input_ast_exact_and_one_short_room() {
    let source = "export const value={field:'tiny'};";
    let preview = ParsedModuleInput::parse(source, None).unwrap();
    let bytes = preview.bytes();
    assert!(bytes >= crate::parser::module_parse_input_bytes(source).unwrap());
    drop(preview);
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let result = ParsedModuleInput::parse(source, vm.host_scheduling.as_ref());
        if accepts {
            let input = result.unwrap();
            assert_eq!(input.bytes(), bytes);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(input);
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_compilation_input_source_scratch_peak_has_exact_and_one_short_admission() {
    let source = "export function f(){return 1;} export const value = `${(()=>2)()}`;";
    let preview = page();
    let scope = preview.host_scheduling.as_ref().unwrap().child();
    let (result, peak) =
        scope.measure_payload_peak(|| ParsedModuleInput::parse(source, Some(&scope)));
    let input = result.unwrap();
    let retained = input.bytes();
    assert!(
        peak > retained,
        "source deduplication backing coexists with the AST"
    );
    drop(input);
    drop(preview);
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = ParsedModuleInput::parse(source, vm.host_scheduling.as_ref());
        if accepts {
            let input = result.unwrap();
            assert_eq!(input.bytes(), retained);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES - peak + retained
            );
            drop(input);
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_compilation_input_ast_and_root_code_owners_coexist_before_handoff() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let input =
        ParsedModuleInput::parse("export const value=1;", vm.host_scheduling.as_ref()).unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| input.compile(Some(&scope)));
    let code = result.unwrap();
    assert_eq!(peak, code.bytes());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + code.bytes()
    );
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_compilation_input_root_refusal_keeps_registry_unpublished_and_refunds_ast() {
    let source = "export const value=1;";
    let preview = ParsedModuleInput::parse(source, None).unwrap();
    let bytes = preview.bytes();
    drop(preview);
    let mut vm = page();
    vm.set_dynamic_module_sources(HashMap::from([("test/tiny.mjs".into(), source.into())]));
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut draft = ModuleRegistryDraft::new(ModuleRegistry::default());
    let peer = leave_room(&mut vm, bytes);
    assert_eq!(
        vm.ensure_dynamic_module_compiled("test/tiny.mjs", &mut draft),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!draft.contains_key("test/tiny.mjs"));
    drop(peer);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_compilation_input_syntax_error_refunds_input_and_preserves_registry() {
    let mut vm = page();
    vm.set_dynamic_module_sources(HashMap::from([(
        "test/bad.mjs".into(),
        "export const =;".into(),
    )]));
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut draft = ModuleRegistryDraft::new(ModuleRegistry::default());
    assert!(matches!(
        vm.ensure_dynamic_module_compiled("test/bad.mjs", &mut draft),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert!(!draft.contains_key("test/bad.mjs"));
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_compilation_input_existing_and_missing_sources_need_no_parse_quota() {
    let mut vm = page();
    let code = crate::compile_module(&crate::parse_module("export const x=1;").unwrap()).unwrap();
    let registry = ModuleRegistry::new(
        HashMap::from([("test/present.mjs".into(), code)]),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let mut draft = ModuleRegistryDraft::new(registry);
    let _peer = leave_room(&mut vm, 0);
    vm.ensure_dynamic_module_compiled("test/present.mjs", &mut draft)
        .unwrap();
    vm.ensure_dynamic_module_compiled("test/missing.mjs", &mut draft)
        .unwrap();
    assert!(draft.contains_key("test/present.mjs"));
    assert!(!draft.contains_key("test/missing.mjs"));
    vm.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn module_compilation_input_unbounded_ast_attaches_before_compilation() {
    let mut vm = page();
    let input = ParsedModuleInput::parse("export const value=1;", None).unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let code = input.compile(vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + code.bytes()
    );
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_compilation_input_foreign_ast_refuses_before_compiler_entry() {
    let mut source_vm = page();
    let target_vm = page();
    let before = source_vm.host_scheduling_usage().unwrap().payload_bytes;
    let input =
        ParsedModuleInput::parse("export const value=1;", source_vm.host_scheduling.as_ref())
            .unwrap();
    assert_eq!(
        input.compile(target_vm.host_scheduling.as_ref()).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        source_vm.host_scheduling_usage().unwrap().payload_bytes,
        before
    );
    assert_eq!(
        target_vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_compilation_input_owned_ast_cannot_escape_to_unbounded_compilation() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let input =
        ParsedModuleInput::parse("export const value=1;", vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        input.compile(None).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_compilation_input_terminal_scope_refuses_before_compiler_entry() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let input =
        ParsedModuleInput::parse("export const value=1;", vm.host_scheduling.as_ref()).unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        input.compile(vm.host_scheduling.as_ref()).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn module_compilation_input_public_dynamic_module_keeps_imports_exports_await_and_source() {
    let mut vm = page();
    let dependency =
        crate::compile_module(&crate::parse_module("export const v=4;").unwrap()).unwrap();
    vm.set_module_loader_context(
        "test/main.js",
        HashMap::from([("test/dep.mjs".into(), dependency)]),
    );
    vm.set_dynamic_module_sources(HashMap::from([("test/entry.mjs".into(), "import {v} from './dep.mjs'; export function read(){return v;} await Promise.resolve(0); export const fragment=`${(()=>2)()}`; export const result=read()+1;".into())]));
    execute(&mut vm, "var loaded=0, sourceMatches=false; import('./entry.mjs').then(ns=>{loaded=ns.result; sourceMatches=ns.read.toString().includes('function read()') && ns.fragment==='2';}); 0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(execute(&mut vm, "loaded;"), Value::Number(5.0));
    assert_eq!(execute(&mut vm, "sourceMatches;"), Value::Bool(true));
    assert!(vm.module_registry.contains_key("test/entry.mjs"));
}

#[cfg_attr(test, test)]
fn module_compilation_input_borrowed_early_error_preserves_input_for_reuse() {
    let module = invalid_module();
    let before = module.clone();
    for _ in 0..2 {
        assert_eq!(
            crate::compile_module(&module).err(),
            Some(crate::CompileError::InvalidSyntax(
                "strict code cannot assign to eval or arguments"
            ))
        );
        assert_eq!(module, before);
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_module_compilation_input_contracts() {
        module_compilation_input_template_fragment_sources_are_admitted_together();
        module_compilation_input_admitted_metric_preserves_all_payload_kinds();
        module_compilation_input_mixed_source_metric_counts_both_allocations();
        module_compilation_input_parsed_owner_counts_actual_retained_capacity();
        module_compilation_input_ast_owner_outlives_its_vm();
        module_compilation_input_ast_exact_and_one_short_room();
        module_compilation_input_source_scratch_peak_has_exact_and_one_short_admission();
        module_compilation_input_ast_and_root_code_owners_coexist_before_handoff();
        module_compilation_input_root_refusal_keeps_registry_unpublished_and_refunds_ast();
        module_compilation_input_syntax_error_refunds_input_and_preserves_registry();
        module_compilation_input_existing_and_missing_sources_need_no_parse_quota();
        module_compilation_input_unbounded_ast_attaches_before_compilation();
        module_compilation_input_foreign_ast_refuses_before_compiler_entry();
        module_compilation_input_owned_ast_cannot_escape_to_unbounded_compilation();
        module_compilation_input_terminal_scope_refuses_before_compiler_entry();
        module_compilation_input_public_dynamic_module_keeps_imports_exports_await_and_source();
        module_compilation_input_borrowed_early_error_preserves_input_for_reuse();
    }
}
