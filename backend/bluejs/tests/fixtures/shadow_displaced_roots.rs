// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reentrant realm setup rolls back frame and lexical roots before script entry.

use super::*;

#[cfg_attr(test, test)]
fn displaced_root_ownership_reentrant_extra_registration_restores_every_saved_owner() {
    let compile = |source| crate::compile(&crate::parse(source).unwrap()).unwrap();
    let mut caller = Vm::default();
    let mut child = Vm::default();
    child.use_bounded_host_microtask_checkpoints().unwrap();
    child.execute_script(&compile("let outer=7;0;")).unwrap();
    let first = child.with_roots(|heap| heap.alloc_object(None)).unwrap();
    child.stack.push(Value::Object(first));
    let env = child.with_roots(|heap| heap.alloc_object(None)).unwrap();
    child.parameter_eval_env = Some(env);
    child.kept_weak_objects.push(first);
    child.call_depth = 3;
    child.remaining_instructions = 41;
    child.inherited_with_depth = 2;
    let pointer = child.stack.as_ptr();
    let frame = child.suspend_module_execution();
    let root_count = Vm::suspended_execution_references(&frame).count();
    child.restore_module_execution(frame);
    child.heap.allow_root_registrations(root_count as u64);
    assert_eq!(
        Vm::run_evaluate(
            &mut caller,
            &mut child,
            &compile("globalThis.bodyRuns=1;0;"),
            true
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(child.stack.as_ptr(), pointer);
    assert_eq!(child.stack, vec![Value::Object(first)]);
    assert_eq!(child.parameter_eval_env, Some(env));
    assert_eq!(child.kept_weak_objects, vec![first]);
    assert_eq!(child.call_depth, 3);
    assert_eq!(child.remaining_instructions, 41);
    assert_eq!(child.inherited_with_depth, 2);
    assert_eq!(
        child.lookup_global_name("outer").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(child.lookup_global_name("bodyRuns").unwrap(), None);
    assert_eq!(child.reserved_global_binding_slots, 0);
    assert_eq!(child.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_public_evaluate_preserves_fatal_rooting_errors() {
    let compile = |source| crate::compile(&crate::parse(source).unwrap()).unwrap();
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let realm = vm.execute_script(&compile("new ShadowRealm();")).unwrap();
    let record = vm
        .shadow_realms
        .get(&realm.object_id().unwrap())
        .unwrap()
        .clone();
    record.vm.borrow_mut().heap.allow_root_registrations(0);
    assert_eq!(
        vm.shadow_realm_evaluate(realm, Value::String("({answer:42});".into())),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
}

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn child_with_saved_owners() -> (Vm, ObjectId, ObjectId, ObjectId) {
    let mut child = Vm::default();
    child.use_bounded_host_microtask_checkpoints().unwrap();
    child.install_test262_harness().unwrap();
    child.execute_script(&compile("let outer=7;0;")).unwrap();
    let stack = child.with_roots(|heap| heap.alloc_object(None)).unwrap();
    child.stack.push(Value::Object(stack));
    let env = child.with_roots(|heap| heap.alloc_object(None)).unwrap();
    child.parameter_eval_env = Some(env);
    let weak = child.with_roots(|heap| heap.alloc_object(None)).unwrap();
    child.kept_weak_objects.push(weak);
    child.call_depth = 3;
    child.remaining_instructions = 41;
    child.inherited_with_depth = 2;
    (child, stack, env, weak)
}
fn assert_saved_owners(child: &mut Vm, stack: ObjectId, env: ObjectId, weak: ObjectId) {
    assert_eq!(child.stack, vec![Value::Object(stack)]);
    assert_eq!(child.parameter_eval_env, Some(env));
    assert_eq!(child.kept_weak_objects, vec![weak]);
    assert_eq!(child.call_depth, 3);
    assert_eq!(child.remaining_instructions, 41);
    assert_eq!(child.inherited_with_depth, 2);
    assert_eq!(
        child.lookup_global_name("outer").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(child.reserved_global_binding_slots, 0);
    assert_eq!(child.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_reentrant_gc_restores_all_saved_owners() {
    let mut caller = Vm::default();
    let (mut child, stack, env, weak) = child_with_saved_owners();
    let pointer = child.stack.as_ptr();
    assert_eq!(
        Vm::run_evaluate(
            &mut caller,
            &mut child,
            &compile("let outer=9;$262.gc();globalThis.bodyRuns=1;42;"),
            true
        ),
        Ok(Value::Number(42.0))
    );
    assert_eq!(child.stack.as_ptr(), pointer);
    assert_saved_owners(&mut child, stack, env, weak);
    assert!(child.heap.contains(stack) && child.heap.contains(env) && child.heap.contains(weak));
    assert_eq!(
        child.lookup_global_name("bodyRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    child.stack.clear();
    child.parameter_eval_env = None;
    child.kept_weak_objects.clear();
    child
        .with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    assert!(!child.heap.contains(stack) && !child.heap.contains(env) && !child.heap.contains(weak));
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_reentrant_common_failure_restores_lexical_context() {
    let mut caller = Vm::default();
    let (mut child, stack, env, weak) = child_with_saved_owners();
    child.heap.allow_root_registrations(0);
    assert_eq!(
        Vm::run_evaluate(
            &mut caller,
            &mut child,
            &compile("globalThis.bodyRuns=1;0;"),
            true
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_saved_owners(&mut child, stack, env, weak);
    assert_eq!(child.lookup_global_name("bodyRuns").unwrap(), None);
}

#[cfg_attr(test, test)]
fn displaced_root_ownership_reentrant_weak_failure_releases_both_root_prefixes() {
    let mut caller = Vm::default();
    let (mut child, stack, env, weak) = child_with_saved_owners();
    let frame = child.suspend_module_execution();
    let roots = Vm::suspended_execution_references(&frame).count();
    child.restore_module_execution(frame);
    child.heap.allow_root_registrations(roots as u64 + 1);
    assert_eq!(
        Vm::run_evaluate(
            &mut caller,
            &mut child,
            &compile("globalThis.bodyRuns=1;0;"),
            true
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_saved_owners(&mut child, stack, env, weak);
    assert_eq!(child.lookup_global_name("bodyRuns").unwrap(), None);
    child.stack.clear();
    child.parameter_eval_env = None;
    child.kept_weak_objects.clear();
    child
        .with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    assert!(!child.heap.contains(stack) && !child.heap.contains(env) && !child.heap.contains(weak));
}

impl Vm {
    pub(in super::super) fn verify_shadow_displaced_root_contracts() {
        displaced_root_ownership_reentrant_extra_registration_restores_every_saved_owner();
        displaced_root_ownership_public_evaluate_preserves_fatal_rooting_errors();
        displaced_root_ownership_reentrant_gc_restores_all_saved_owners();
        displaced_root_ownership_reentrant_common_failure_restores_lexical_context();
        displaced_root_ownership_reentrant_weak_failure_releases_both_root_prefixes();
    }
}
