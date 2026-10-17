// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny ShadowRealm compilation owners and dispatch/metadata boundaries.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 10_000;
    vm
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn realm(vm: &mut Vm) -> Value {
    let realm = execute(vm, "new ShadowRealm()");
    vm.heap.root(realm.object_id().unwrap()).unwrap();
    realm
}
fn returned_code(vm: &mut Vm) -> crate::vm::SharedBytecode {
    let realm = realm(vm);
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    let wrapper = vm
        .shadow_realm_evaluate(realm, &Value::String("(function kept(){return 1;})".into()))
        .unwrap()
        .object_id()
        .unwrap();
    let target = vm.shadow_wrapped_functions[&wrapper].target;
    let child = record.vm.borrow();
    child.heap.closure_code(target).unwrap().unwrap()
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_returned_code_retains_native_source() {
    let mut vm = page();
    let code = returned_code(&mut vm);
    assert!(code.runtime_source_bytes() > 0);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_code_and_source_outlive_the_realm_tree() {
    let mut vm = page();
    let view = observer(&vm);
    let code = returned_code(&mut vm);
    let minimum = code.retained_bytes() + code.source_text.allocation().unwrap().len();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg(test)]
#[test]
fn shadow_eval_storage_refusal_precedes_dispatch_source_copy() {
    let mut counts = [0; 2];
    for (index, dispatch) in [false, true].into_iter().enumerate() {
        let mut vm = page();
        let realm = realm(&mut vm);
        let source = Value::String("0".into());
        let arguments = vec![source.clone()];
        let _peer = leave_room(&mut vm, 0);
        let (result, allocations) = allocation_observer::count(|| {
            if dispatch {
                vm.native_call(NativeFunction::ShadowRealmEvaluate, realm, arguments, false)
            } else {
                vm.shadow_realm_evaluate(realm, &source)
            }
        });
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        counts[index] = allocations;
    }
    // Both entries retain the same prior GC work; dispatch adds no UTF-16 copy.
    assert_eq!(counts[0], counts[1]);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_metadata_refusal_keeps_its_fatal_identity() {
    let mut source = Vm::default();
    let target = execute(&mut source, "var target=function(){}; Object.defineProperty(target,'length',{get(){return {};}}); target;")
        .object_id().unwrap();
    let root = source.heap.root(target).unwrap();
    source
        .with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    let limit = source.heap.allow_only(0);
    let mut destination = Vm::default();
    assert_eq!(
        destination.shadow_wrapped_function_create(&mut source, target),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(destination.shadow_wrapped_functions.is_empty());
    source.heap.unroot(root).unwrap();
}

fn prepared(source: &str, scope: Option<&SchedulingScope>) -> PreparedEvalCode {
    ParsedEvalInput::parse_script(source, scope)
        .unwrap()
        .compile_with(scope, crate::compile)
        .unwrap()
}
fn related_child(caller: &Vm) -> Vm {
    let mut child = Vm {
        host_scheduling: caller.host_scheduling.as_ref().map(SchedulingScope::child),
        bounded_host_microtask_checkpoints: caller.bounded_host_microtask_checkpoints,
        ..Vm::default()
    };
    child
        .heap
        .inherit_document_heap_budget(&caller.heap)
        .unwrap();
    child
}

#[cfg(test)]
#[test]
fn shadow_eval_storage_neutral_caller_checks_child_before_parsing() {
    let mut caller = Vm::default();
    let realm = realm(&mut caller);
    let record = caller.shadow_realms[&realm.object_id().unwrap()].clone();
    let peer = {
        let mut child = record.vm.borrow_mut();
        child.use_bounded_host_microtask_checkpoints().unwrap();
        execute(&mut child, "let preserved=7;0;");
        leave_room(&mut child, 0)
    };
    let source = Value::String("globalThis.ran=1;0;".into());
    let (result, allocations) =
        allocation_observer::count(|| caller.shadow_realm_evaluate(realm, &source));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
    let child = record.vm.borrow();
    assert!(child.global_bindings.contains_key("preserved"));
    assert!(!child.global_bindings.contains_key("ran"));
    drop(peer);
}

#[cfg(test)]
#[test]
fn shadow_eval_storage_active_child_account_refuses_before_parsing() {
    let mut caller = Vm::default();
    let realm = realm(&mut caller);
    let record = caller.shadow_realms[&realm.object_id().unwrap()].clone();
    let mut child = record.vm.borrow_mut();
    child.use_bounded_host_microtask_checkpoints().unwrap();
    execute(&mut child, "let preserved=7;0;");
    let guard = register_active(&mut child).unwrap();
    let peer = leave_room(&mut child, 0);
    let source = Value::String("globalThis.ran=1;0;".into());
    let (result, allocations) =
        allocation_observer::count(|| caller.shadow_realm_evaluate(realm, &source));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
    assert!(child.global_bindings.contains_key("preserved"));
    drop(guard);
    drop(peer);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_public_foreign_record_refuses_before_compilation() {
    let mut owner = page();
    let realm = realm(&mut owner);
    owner
        .shadow_realm_evaluate(realm.clone(), &Value::String("let preserved=7;0;".into()))
        .unwrap();
    let record = owner.shadow_realms[&realm.object_id().unwrap()].clone();
    let mut caller = page();
    let instance = caller.heap.alloc_object(None).unwrap();
    caller.heap.root(instance).unwrap();
    caller.adopt_shadow_realm(instance, record.clone());
    assert_eq!(
        caller.shadow_realm_evaluate(
            Value::Object(instance),
            &Value::String("globalThis.ran=1;0;".into())
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(record.vm.borrow().global_bindings.contains_key("preserved"));
    assert!(caller.check_scheduling_failure().is_ok());
    assert!(caller.stack.is_empty());
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_fresh_lexicals_globals_and_error_boundaries_stay_distinct() {
    let mut vm = page();
    let realm = realm(&mut vm);
    for (source, value) in [
        ("let local=1;var shared=7;local;", 1.0),
        ("let local=2;shared+local;", 9.0),
    ] {
        assert_eq!(
            vm.shadow_realm_evaluate(realm.clone(), &Value::String(source.into())),
            Ok(Value::Number(value))
        );
    }
    assert!(matches!(
        vm.shadow_realm_evaluate(realm.clone(), &Value::String("(".into())),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert_eq!(
        vm.shadow_realm_evaluate(realm.clone(), &Value::String("throw 7;".into())),
        Err(RuntimeError::TypeError(String::new()))
    );
    assert!(matches!(
        vm.shadow_realm_evaluate(realm, &Value::Number(1.0)),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.shadow_realm_evaluate(Value::Undefined, &Value::String("(".into())),
        Err(RuntimeError::TypeError(_))
    ));
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_busy_record_preserves_parse_error_precedence() {
    let mut vm = page();
    let realm = realm(&mut vm);
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    let _held = record.vm.borrow_mut();
    assert!(matches!(
        vm.shadow_realm_evaluate(realm, &Value::String("(".into())),
        Err(RuntimeError::SyntaxError(_))
    ));
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_active_account_views_follow_guard_nesting() {
    let mut first = page();
    let first_tag = first.object_prototype.heap;
    let first_scope = first.host_scheduling.as_ref().unwrap().child();
    let mut second = page();
    let second_tag = second.object_prototype.heap;
    let second_scope = second.host_scheduling.as_ref().unwrap().child();
    let outer = register_active(&mut first).unwrap();
    let inner = register_active(&mut second).unwrap();
    assert!(active_scheduling_scope(first_tag)
        .unwrap()
        .same_document(&first_scope));
    assert!(active_scheduling_scope(second_tag)
        .unwrap()
        .same_document(&second_scope));
    drop(inner);
    assert!(active_scheduling_scope(second_tag).is_none());
    assert!(active_scheduling_scope(first_tag).is_some());
    drop(outer);
    assert!(active_scheduling_scope(first_tag).is_none());
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_root_attachment_has_exact_and_one_short_admission() {
    let vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut code = prepared("(function kept(){return 1;})", None);
    let (result, peak) = scope.measure_payload_peak(|| code.attach(&scope));
    result.unwrap();
    assert!(peak > host_scheduling::owned_code_bytes(&code));
    drop(code);
    for accepts in [false, true] {
        let mut vm = page();
        let mut code = prepared("(function kept(){return 1;})", None);
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = code.attach(vm.host_scheduling.as_ref().unwrap());
        if accepts {
            result.unwrap();
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        }
        drop(code);
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_repeated_attachment_keeps_one_owner() {
    let vm = page();
    let mut code = prepared("(function kept(){return 1;})", vm.host_scheduling.as_ref());
    let scope = vm.host_scheduling.as_ref().unwrap();
    code.attach(scope).unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    code.attach(scope).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_foreign_document_refuses_before_context_changes() {
    for reentrant in [false, true] {
        let mut caller = page();
        let mut child = page();
        execute(&mut child, "let preserved=7;0;");
        let mut code = prepared("globalThis.ran=1;0;", caller.host_scheduling.as_ref());
        assert_eq!(
            Vm::run_prepared_evaluate(&mut caller, &mut child, &mut code, reentrant),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(child.global_bindings.contains_key("preserved"));
        assert!(!child.global_bindings.contains_key("ran"));
        assert!(child.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_terminal_child_keeps_its_prior_lexicals() {
    let mut caller = page();
    let mut child = related_child(&caller);
    execute(&mut child, "let preserved=7;0;");
    let mut code = prepared("globalThis.ran=1;0;", caller.host_scheduling.as_ref());
    child.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        Vm::run_prepared_evaluate(&mut caller, &mut child, &mut code, false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(child.global_bindings.contains_key("preserved"));
    assert!(!child.global_bindings.contains_key("ran"));
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_existing_neutral_code_attaches_when_child_is_bounded() {
    let mut vm = Vm::default();
    let code = returned_code(&mut vm);
    let record = vm.shadow_realms.values().next().unwrap().clone();
    let view = {
        let mut child = record.vm.borrow_mut();
        child.use_bounded_host_microtask_checkpoints().unwrap();
        child.use_bounded_host_microtask_checkpoints().unwrap();
        observer(&child)
    };
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(record);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_neutral_caller_compiles_against_child_account() {
    let mut vm = Vm::default();
    let realm = realm(&mut vm);
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    let view = {
        let mut child = record.vm.borrow_mut();
        child.use_bounded_host_microtask_checkpoints().unwrap();
        observer(&child)
    };
    let wrapper = vm
        .shadow_realm_evaluate(realm, &Value::String("(function kept(){return 1;})".into()))
        .unwrap()
        .object_id()
        .unwrap();
    let target = vm.shadow_wrapped_functions[&wrapper].target;
    let code = record
        .vm
        .borrow()
        .heap
        .closure_code(target)
        .unwrap()
        .unwrap();
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(record);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_neutral_child_retains_caller_compilation_lease() {
    let mut vm = Vm::default();
    let realm = realm(&mut vm);
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let wrapper = vm
        .shadow_realm_evaluate(realm, &Value::String("(function kept(){return 1;})".into()))
        .unwrap()
        .object_id()
        .unwrap();
    let target = vm.shadow_wrapped_functions[&wrapper].target;
    let code = record
        .vm
        .borrow()
        .heap
        .closure_code(target)
        .unwrap()
        .unwrap();
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(record);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_public_reentrant_evaluation_preserves_live_locals() {
    let mut vm = page();
    assert_eq!(execute(&mut vm,"var r=new ShadowRealm();var outer=r.evaluate('(function(callback){let kept=41;callback();return kept+1;})');outer(()=>r.evaluate('let inner=3;(function(){return inner;})'));"),Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_reentrant_gc_restores_saved_owners() {
    let mut caller = page();
    let mut child = related_child(&caller);
    child.install_test262_harness().unwrap();
    execute(&mut child, "let preserved=7;0;");
    let object = child.heap.alloc_object(None).unwrap();
    child.stack.push(Value::Object(object));
    child.parameter_eval_env = Some(object);
    child.kept_weak_objects.push(object);
    child.call_depth = 3;
    child.inherited_with_depth = 2;
    child.remaining_instructions = 41;
    let pointer = child.stack.as_ptr();
    let mut code = prepared(
        "let preserved=9;$262.gc();42;",
        caller.host_scheduling.as_ref(),
    );
    assert_eq!(
        Vm::run_prepared_evaluate(&mut caller, &mut child, &mut code, true),
        Ok(Value::Number(42.0))
    );
    assert_eq!(child.stack.as_ptr(), pointer);
    assert_eq!(child.stack, vec![Value::Object(object)]);
    assert_eq!(child.parameter_eval_env, Some(object));
    assert_eq!(child.kept_weak_objects, vec![object]);
    assert_eq!(child.call_depth, 3);
    assert_eq!(child.inherited_with_depth, 2);
    assert_eq!(child.remaining_instructions, 41);
    assert_eq!(
        child.lookup_global_name("preserved").unwrap(),
        Some(Value::Number(7.0))
    );
    assert!(child.heap.contains(object));
}

fn queued(timer: bool) {
    let mut vm = page();
    let realm = realm(&mut vm);
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    if timer {
        record
            .vm
            .borrow_mut()
            .install_host_timer_functions()
            .unwrap();
    }
    vm.shadow_realm_evaluate(
        realm,
        &Value::String("var result=0;globalThis.callback=function(){result+=1;};0;".into()),
    )
    .unwrap();
    let (function, source) = {
        let mut child = record.vm.borrow_mut();
        let function = child
            .lookup_global_name("callback")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        let code = child.heap.closure_code(function).unwrap().unwrap();
        assert!(code.runtime_source_bytes() > 0);
        let source = std::sync::Arc::downgrade(code.source_text.allocation().unwrap());
        execute(
            &mut child,
            if timer {
                "setTimeout(callback,1);callback=undefined;0;"
            } else {
                "var pending=Promise.withResolvers();pending.promise.then(callback);callback=undefined;0;"
            },
        );
        (function, source)
    };
    vm.collect_realm_garbage().unwrap();
    assert!(record.vm.borrow().heap.contains(function));
    {
        let mut child = record.vm.borrow_mut();
        if timer {
            assert_eq!(child.run_due_host_timers(1).unwrap().tasks_run, 1);
        } else {
            execute(&mut child, "pending.resolve();pending=undefined;0;");
            child.run_promise_jobs_bounded(4).unwrap();
        }
        assert_eq!(execute(&mut child, "result;"), Value::Number(1.0));
    }
    vm.collect_realm_garbage().unwrap();
    assert!(!record.vm.borrow().heap.contains(function));
    assert!(source.upgrade().is_none());
}
#[cfg_attr(test, test)]
fn shadow_eval_storage_timer_keeps_code_until_delivery_and_gc() {
    queued(true);
}
#[cfg_attr(test, test)]
fn shadow_eval_storage_promise_keeps_code_until_delivery_and_gc() {
    queued(false);
}

#[cfg_attr(test, test)]
fn shadow_eval_storage_utf16_input_keeps_lone_surrogates_and_reserved_characters() {
    let mut vm = page();
    let realm = realm(&mut vm);
    for expected in [
        JsString::from_code_units(vec![0xd800, 65, 0xdc00, 0xd83d, 0xde00]),
        JsString::from("\u{10f000}\u{10f800}"),
    ] {
        let mut units = vec![39];
        units.extend_from_slice(expected.as_code_units());
        units.push(39);
        let source = Value::String(JsString::from_code_units(units));
        assert_eq!(
            vm.shadow_realm_evaluate(realm.clone(), &source),
            Ok(Value::String(expected))
        );
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_shadow_eval_storage_contracts() {
        shadow_eval_storage_returned_code_retains_native_source();
        shadow_eval_storage_code_and_source_outlive_the_realm_tree();
        shadow_eval_storage_metadata_refusal_keeps_its_fatal_identity();
        shadow_eval_storage_public_foreign_record_refuses_before_compilation();
        shadow_eval_storage_fresh_lexicals_globals_and_error_boundaries_stay_distinct();
        shadow_eval_storage_busy_record_preserves_parse_error_precedence();
        shadow_eval_storage_active_account_views_follow_guard_nesting();
        shadow_eval_storage_root_attachment_has_exact_and_one_short_admission();
        shadow_eval_storage_repeated_attachment_keeps_one_owner();
        shadow_eval_storage_foreign_document_refuses_before_context_changes();
        shadow_eval_storage_terminal_child_keeps_its_prior_lexicals();
        shadow_eval_storage_existing_neutral_code_attaches_when_child_is_bounded();
        shadow_eval_storage_neutral_caller_compiles_against_child_account();
        shadow_eval_storage_neutral_child_retains_caller_compilation_lease();
        shadow_eval_storage_public_reentrant_evaluation_preserves_live_locals();
        shadow_eval_storage_reentrant_gc_restores_saved_owners();
        shadow_eval_storage_timer_keeps_code_until_delivery_and_gc();
        shadow_eval_storage_promise_keeps_code_until_delivery_and_gc();
        shadow_eval_storage_utf16_input_keeps_lone_surrogates_and_reserved_characters();
    }
}
