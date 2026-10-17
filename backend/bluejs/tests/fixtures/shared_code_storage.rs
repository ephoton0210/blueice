// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed small shared-code lifetimes at the dynamic compilation boundary.

use super::*;
use crate::vm::host_scheduling::owned_code_bytes;
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
fn shared_code_storage_child_retains_native_buffers_after_vm_drop() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export function f(){let value='tiny';return value;}");
    let child = root.functions[0].clone();
    let minimum =
        child.runtime_source_bytes() + std::mem::size_of::<Bytecode>() + owned_code_bytes(&child);
    drop(root);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shared_code_storage_sibling_buffers_release_independently_of_source() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(
        &vm,
        "export function a(){return 1;} export function b(){return 2;}",
    );
    let first = root.functions[0].clone();
    let second = root.functions[1].clone();
    drop(root);
    drop(vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    drop(first);
    let after = view.host_scheduling_snapshot().payload_bytes;
    assert!(before > after && after > 0);
    assert!(after >= second.runtime_source_bytes() + owned_code_bytes(&second));
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shared_code_storage_nested_child_retains_only_its_code_and_source() {
    let vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export function outer(){return ()=>7;}");
    let inner = root.functions[0].functions[0].clone();
    let minimum =
        inner.runtime_source_bytes() + std::mem::size_of::<Bytecode>() + owned_code_bytes(&inner);
    drop(root);
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
    drop(inner);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_shared_code_storage_contracts() {
        shared_code_storage_child_retains_native_buffers_after_vm_drop();
        shared_code_storage_sibling_buffers_release_independently_of_source();
        shared_code_storage_nested_child_retains_only_its_code_and_source();
        shared_code_storage_clones_share_one_header_and_buffer_lease();
        shared_code_storage_unbounded_child_attaches_once();
        shared_code_storage_sourceless_header_rejects_foreign_documents();
        shared_code_storage_metadata_copy_admits_coexisting_storage();
        shared_code_storage_metadata_copy_shares_nested_code();
        shared_code_storage_metadata_copy_has_exact_and_one_short_admission();
        shared_code_storage_unique_metadata_edit_needs_no_headroom();
        shared_code_storage_editor_error_refunds_removed_buffers();
        shared_code_storage_terminal_cleanup_can_release_buffers();
        shared_code_storage_runtime_storage_has_no_unchecked_mutable_access();
        shared_code_storage_registry_copies_share_child_storage();
        shared_code_storage_refusal_preserves_existing_code();
        shared_code_storage_public_dynamic_classes_and_generators_keep_working();
    }
}

fn child(vm: &Vm) -> SharedBytecode {
    let root = compiled(vm, "export function some_name(){return 'tiny';}");
    root.functions[0].clone()
}
fn leave_room(vm: &mut Vm, room: usize) -> host_scheduling::SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn shared_code_storage_clones_share_one_header_and_buffer_lease() {
    let vm = page();
    let view = observer(&vm);
    let code = child(&vm);
    let expected = code.retained_bytes() + code.runtime_source_bytes();
    let copy = code.clone();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(copy);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shared_code_storage_unbounded_child_attaches_once() {
    let unbounded = Vm::default();
    let code = child(&unbounded);
    drop(unbounded);
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let budget = vm.host_scheduling.as_ref().unwrap().heap_native_budget();
    code.attach_runtime_sources(&budget).unwrap();
    code.attach_runtime_sources(&budget).unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        code.retained_bytes() + code.runtime_source_bytes()
    );
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn shared_code_storage_sourceless_header_rejects_foreign_documents() {
    let mut vm = page();
    let target = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut code = SharedBytecode::new(Bytecode::empty());
    code.enable_runtime(vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        code.retained_bytes()
    );
    assert_eq!(
        code.attach_runtime_sources(
            &target
                .host_scheduling
                .as_ref()
                .unwrap()
                .heap_native_budget()
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    vm.check_scheduling_failure().unwrap();
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn shared_code_storage_metadata_copy_admits_coexisting_storage() {
    let mut vm = page();
    let original = child(&vm);
    let mut copy = original.clone();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    copy.edit(|code| code.debugger_program_generation = Some(7))
        .unwrap();
    assert_eq!(original.debugger_program_generation, None);
    assert_eq!(copy.debugger_program_generation, Some(7));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        copy.retained_bytes()
    );
    drop(copy);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn shared_code_storage_metadata_copy_shares_nested_code() {
    let mut vm = page();
    let view = observer(&vm);
    let root = compiled(&vm, "export function outer(){return ()=>7;}");
    let original = root.functions[0].clone();
    let inner = original.functions[0].clone();
    let mut copy = original.clone();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    copy.edit(|code| code.debugger_program_generation = Some(7))
        .unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        copy.retained_bytes()
    );
    let inner_and_source = inner.runtime_source_bytes() + inner.retained_bytes();
    drop(root);
    drop(original);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        inner_and_source + copy.retained_bytes()
    );
    drop(copy);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        inner_and_source
    );
    drop(inner);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shared_code_storage_metadata_copy_has_exact_and_one_short_admission() {
    let preview = page();
    let original = child(&preview);
    let mut copy = original.clone();
    let scope = preview.host_scheduling.as_ref().unwrap().child();
    let (result, peak) =
        scope.measure_payload_peak(|| copy.edit(|code| code.debugger_program_generation = Some(7)));
    result.unwrap();
    assert_eq!(peak, copy.retained_bytes());
    drop(copy);
    drop(original);
    drop(preview);
    for accepts in [false, true] {
        let mut vm = page();
        let original = child(&vm);
        let mut copy = original.clone();
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = copy.edit(|code| code.debugger_program_generation = Some(7));
        if accepts {
            result.unwrap();
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        }
        assert_eq!(original.debugger_program_generation, None);
        drop(peer);
    }
}

#[cfg(test)]
#[test]
fn shared_code_storage_copy_refusal_precedes_allocation() {
    let mut vm = page();
    let original = child(&vm);
    let mut copy = original.clone();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| copy.edit(|code| code.debugger_program_generation = Some(7)));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
    assert_eq!(original.debugger_program_generation, None);
    assert_eq!(copy.debugger_program_generation, None);
}

#[cfg_attr(test, test)]
fn shared_code_storage_unique_metadata_edit_needs_no_headroom() {
    let mut vm = page();
    let mut code = child(&vm);
    let _peer = leave_room(&mut vm, 0);
    code.edit(|code| code.debugger_program_generation = Some(7))
        .unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    vm.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn shared_code_storage_editor_error_refunds_removed_buffers() {
    let mut vm = page();
    let mut code = child(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let old_bytes = code.retained_bytes();
    assert!(!code.function_name.is_empty());
    assert_eq!(
        code.edit(|code| {
            code.function_name = String::new();
            Err::<(), _>("editor refused")
        }),
        Ok(Err("editor refused"))
    );
    let released = old_bytes - code.retained_bytes();
    assert!(released > 0);
    assert_eq!(
        before - vm.host_scheduling_snapshot().payload_bytes,
        released
    );
}

#[cfg_attr(test, test)]
fn shared_code_storage_terminal_cleanup_can_release_buffers() {
    let vm = page();
    let mut code = child(&vm);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.host_scheduling.as_ref().unwrap().fail();
    code.edit(|code| code.function_name = String::new())
        .unwrap();
    assert!(vm.host_scheduling_snapshot().payload_bytes < before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn shared_code_storage_runtime_storage_has_no_unchecked_mutable_access() {
    let vm = page();
    let mut code = child(&vm);
    assert!(code.get_mut().is_none());
    let mut ordinary = SharedBytecode::new(Bytecode::empty());
    assert!(ordinary.get_mut().is_some());
}

#[cfg_attr(test, test)]
fn shared_code_storage_registry_copies_share_child_storage() {
    let vm = page();
    let view = observer(&vm);
    let prepared = compiled(&vm, "export function f(){return 1;}");
    let child = prepared.functions[0].clone();
    let source_and_code = child.runtime_source_bytes() + child.retained_bytes();
    let first = ModuleRegistry::new(
        HashMap::from([("entry".into(), prepared.into_unleased())]),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let second = ModuleRegistry::copy_host(&first, vm.host_scheduling.as_ref()).unwrap();
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        first.retained_bytes() + second.retained_bytes() + source_and_code
    );
    let after_first = second.retained_bytes() + source_and_code;
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, after_first);
    drop(second);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        source_and_code
    );
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shared_code_storage_refusal_preserves_existing_code() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut code = SharedBytecode::new(Bytecode::empty());
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        code.enable_runtime(Some(&scope)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(code.bytes().is_empty());
    assert_eq!(
        scope
            .shared_charge(0)
            .err()
            .map(|_| RuntimeError::SchedulingLimit),
        Some(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn shared_code_storage_public_dynamic_classes_and_generators_keep_working() {
    let mut vm = page();
    let view = observer(&vm);
    vm.set_module_loader_context("test/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "test/value.mjs".into(),
        "export class C { value=7; read(){return this.value;} } export function* g(){yield 3;}"
            .into(),
    )]));
    let code = crate::compile(&crate::parse("var result=0; import('./value.mjs').then(ns=>{result=new ns.C().read()+ns.g().next().value;}); 0;").unwrap()).unwrap();
    vm.execute_script(&code).unwrap();
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        vm.lookup_global_name("result").unwrap(),
        Some(Value::Number(10.0))
    );
    vm.collect_realm_garbage().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
