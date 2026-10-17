// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny source lifetimes across compilation, closure sharing and VM teardown.

use super::*;
use crate::vm::module_compilation_input::ParsedModuleInput;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn compiled(vm: &Vm, source: &str) -> module_code_storage::PreparedModuleCode {
    ParsedModuleInput::parse(source, vm.host_scheduling.as_ref())
        .unwrap()
        .compile(vm.host_scheduling.as_ref())
        .unwrap()
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_child_keeps_source_after_root_and_vm_drop() {
    let vm = page();
    let view = observer(&vm);
    let source = "export function f(){return 1;}";
    let root = compiled(&vm, source);
    let child = root.functions[0].clone();
    drop(root);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= source.len());
    assert_eq!(child.source_text.as_str(), Some("function f(){return 1;}"));
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_child_clones_share_one_source_lease() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export function f(){return 1;}");
    let child = root.functions[0].clone();
    drop(root);
    drop(vm);
    let retained = view.host_scheduling_snapshot().payload_bytes;
    assert!(retained > 0);
    let other = child.clone();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(other);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_template_sources_release_independently() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(
        &vm,
        "export function f(){return 1;} export const fragment=`${(()=>2)()}`;",
    );
    assert_eq!(root.functions.len(), 2);
    let whole = root.functions[0].clone();
    let fragment = root.functions[1].clone();
    drop(root);
    drop(vm);
    let both = view.host_scheduling_snapshot().payload_bytes;
    assert!(both > 0);
    drop(whole);
    let one = view.host_scheduling_snapshot().payload_bytes;
    assert!(one > 0 && one < both);
    assert_eq!(fragment.source_text.as_str(), Some("()=>2"));
    drop(fragment);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_compiled_source_ownership_contracts() {
        compiled_source_ownership_child_keeps_source_after_root_and_vm_drop();
        compiled_source_ownership_child_clones_share_one_source_lease();
        compiled_source_ownership_template_sources_release_independently();
        compiled_source_ownership_nested_child_keeps_one_source();
        compiled_source_ownership_bytecode_copies_share_source();
        compiled_source_ownership_empty_module_retains_no_source();
        compiled_source_ownership_unbounded_code_attaches_once();
        compiled_source_ownership_foreign_child_cannot_enter_registry();
        compiled_source_ownership_host_compiled_code_keeps_cross_document_reuse();
        compiled_source_ownership_compile_peak_exact_and_one_short();
        compiled_source_ownership_heap_activation_finds_detached_closure();
        compiled_source_ownership_heap_activation_finds_generator();
        compiled_source_ownership_heap_rejects_foreign_code_before_allocation();
        compiled_source_ownership_collected_closure_refunds_last_source();
        compiled_source_ownership_terminal_cached_attachment_still_refuses();
        compiled_source_ownership_lookup_growth_refunds_only_temporary_backing();
        compiled_source_ownership_timer_keeps_callback_until_delivery();
        compiled_source_ownership_promise_job_keeps_callback_until_delivery();
    }
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_nested_child_keeps_one_source() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export function outer(){return ()=>3;}");
    let child = root.functions[0].functions[0].clone();
    let bytes = child.runtime_source_bytes() + child.retained_bytes();
    assert!(bytes > 0);
    drop(root);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_bytecode_copies_share_source() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export function f(){return 1;}");
    let child = (*root.functions[0]).clone();
    let bytes = child.runtime_source_bytes();
    let copy = child.clone();
    drop(root);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_empty_module_retains_no_source() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "");
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, root.bytes());
    drop(root);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_unbounded_code_attaches_once() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut code = ParsedModuleInput::parse("export function f(){return 1;}", None)
        .unwrap()
        .compile(None)
        .unwrap();
    let bytes = code.bytes()
        + code.functions[0].runtime_source_bytes()
        + code.functions[0].retained_bytes();
    code.attach(vm.host_scheduling.as_ref()).unwrap();
    code.attach(vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_foreign_child_cannot_enter_registry() {
    let source_vm = page();
    let target_vm = page();
    let root = compiled(&source_vm, "export function f(){return 1;}");
    let child = (*root.functions[0]).clone();
    assert_eq!(
        ModuleRegistry::new(
            HashMap::from([("child".into(), child)]),
            target_vm.host_scheduling.as_ref()
        )
        .err(),
        Some(RuntimeError::SchedulingLimit)
    );
    source_vm.check_scheduling_failure().unwrap();
    assert_eq!(
        target_vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_host_compiled_code_keeps_cross_document_reuse() {
    let first = page();
    let second = page();
    let code =
        crate::compile_module(&crate::parse_module("export function f(){return 1;}").unwrap())
            .unwrap();
    let a = ModuleRegistry::new(
        HashMap::from([("a".into(), code.clone())]),
        first.host_scheduling.as_ref(),
    )
    .unwrap();
    let b = ModuleRegistry::new(
        HashMap::from([("a".into(), code)]),
        second.host_scheduling.as_ref(),
    )
    .unwrap();
    a.attach(first.host_scheduling.as_ref().unwrap()).unwrap();
    b.attach(second.host_scheduling.as_ref().unwrap()).unwrap();
}

fn leave_room(vm: &mut Vm, room: usize) -> host_scheduling::SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg(test)]
#[test]
fn compiled_source_ownership_header_refusal_precedes_owner_allocation() {
    let mut vm = page();
    let input = ParsedModuleInput::parse(
        "export function f(){return 1;}",
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| input.compile(vm.host_scheduling.as_ref()));
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_compile_peak_exact_and_one_short() {
    let source = "export function f(){return 1;} export const fragment=`${(()=>2)()}`;";
    let preview = page();
    let input = ParsedModuleInput::parse(source, preview.host_scheduling.as_ref()).unwrap();
    let scope = preview.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| input.compile(Some(&scope)));
    drop(result.unwrap());
    drop(preview);
    assert!(peak > 0);
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let input = ParsedModuleInput::parse(source, vm.host_scheduling.as_ref()).unwrap();
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = input.compile(vm.host_scheduling.as_ref());
        if accepts {
            drop(result.unwrap());
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

fn unbounded_child() -> SharedBytecode {
    let root = ParsedModuleInput::parse("export function f(){return 1;}", None)
        .unwrap()
        .compile(None)
        .unwrap();
    root.functions[0].clone()
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_heap_activation_finds_detached_closure() {
    let mut vm = Vm::default();
    let child = unbounded_child();
    let bytes = child.runtime_source_bytes() + child.retained_bytes();
    let prototype = vm.function_prototype().unwrap();
    vm.heap
        .alloc_closure(child.clone(), Vec::new(), Value::Undefined, prototype)
        .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_heap_activation_finds_generator() {
    let mut vm = Vm::default();
    let child = unbounded_child();
    let bytes = child.runtime_source_bytes() + child.retained_bytes();
    let prototype = vm.heap.alloc_object(None).unwrap();
    vm.heap
        .alloc_generator(
            crate::heap::GeneratorState::Start {
                code: child.clone(),
                captures: Vec::new(),
                callee: Value::Undefined,
                receiver: Value::Undefined,
                args: Vec::new(),
                home: None,
                with_objects: Vec::new(),
            },
            prototype,
        )
        .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_heap_rejects_foreign_code_before_allocation() {
    let source = page();
    let mut target = page();
    let root = compiled(&source, "export function f(){return 1;}");
    let prototype = target.function_prototype().unwrap();
    let before = target.heap.stats();
    assert_eq!(
        target.heap.alloc_closure(
            root.functions[0].clone(),
            Vec::new(),
            Value::Undefined,
            prototype
        ),
        Err(HeapError::CodeSourceStorageLimit)
    );
    assert_eq!(target.heap.stats(), before);
    assert_eq!(
        target.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_collected_closure_refunds_last_source() {
    let mut vm = page();
    let code = compiled(&vm, "export function f(){return 1;}");
    let bytes = code.functions[0].runtime_source_bytes() + code.functions[0].retained_bytes();
    let prototype = vm.function_prototype().unwrap();
    let closure = vm
        .heap
        .alloc_closure(
            code.functions[0].clone(),
            Vec::new(),
            Value::Undefined,
            prototype,
        )
        .unwrap();
    let root = vm.heap.root(closure).unwrap();
    drop(code);
    vm.collect_realm_garbage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes - vm.heap.collector_storage_bytes();
    vm.heap.unroot(root).unwrap();
    vm.collect_realm_garbage().unwrap();
    let after = vm.host_scheduling_snapshot().payload_bytes - vm.heap.collector_storage_bytes();
    assert_eq!(before - after, bytes);
    assert!(!vm.heap.contains(closure));
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_terminal_cached_attachment_still_refuses() {
    let vm = page();
    let root = compiled(&vm, "export function f(){return 1;}");
    let scope = vm.host_scheduling.as_ref().unwrap();
    scope.fail();
    assert_eq!(
        root.attach_runtime_sources(&scope.heap_native_budget()),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_lookup_growth_refunds_only_temporary_backing() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export const a=`${(()=>1)()}`, b=`${(()=>2)()}`, c=`${(()=>3)()}`; export function f(){return 4;}");
    let children = root.functions.clone();
    assert_eq!(children.len(), 4);
    let identities: HashSet<_> = children
        .iter()
        .map(|code| code.source_text.allocation().unwrap().as_ptr() as usize)
        .collect();
    assert_eq!(identities.len(), 4);
    let bytes: usize = children
        .iter()
        .map(|code| code.runtime_source_bytes() + code.retained_bytes())
        .sum();
    drop(root);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(children);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn queued_callback(timer: bool) {
    let mut vm = page();
    if timer {
        vm.install_host_timer_functions().unwrap();
    }
    execute(&mut vm, "var callback, result=0;");
    let root = compiled(&vm, "export function cb(){result+=1;}");
    let source = std::sync::Arc::downgrade(root.functions[0].source_text.allocation().unwrap());
    let prototype = vm.function_prototype().unwrap();
    let callback = vm
        .heap
        .alloc_closure(
            root.functions[0].clone(),
            Vec::new(),
            Value::Undefined,
            prototype,
        )
        .unwrap();
    assert!(vm
        .set_global_binding("callback", Value::Object(callback))
        .unwrap());
    drop(root);
    execute(
        &mut vm,
        if timer {
            "setTimeout(callback,1); callback=undefined; 0;"
        } else {
            "Promise.resolve().then(callback); callback=undefined; 0;"
        },
    );
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(callback));
    assert!(source.upgrade().is_some());
    if timer {
        assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    } else {
        vm.run_promise_jobs_bounded(4).unwrap();
    }
    assert_eq!(execute(&mut vm, "result;"), Value::Number(1.0));
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(callback));
    assert!(source.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_timer_keeps_callback_until_delivery() {
    queued_callback(true);
}

#[cfg_attr(test, test)]
fn compiled_source_ownership_promise_job_keeps_callback_until_delivery() {
    queued_callback(false);
}
