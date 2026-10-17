// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny dynamic-function inputs and code outlive the constructor independently.

use super::*;

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

#[cfg_attr(test, test)]
fn dynamic_function_storage_code_keeps_its_lease_after_vm_drop() {
    let mut vm = page();
    let view = observer(&vm);
    let function = vm
        .function_constructor(&[Value::String("return 1;".into())])
        .unwrap()
        .object_id()
        .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    let source = code.source_text.allocation().unwrap().len();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= source + code.retained_bytes());
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_all_four_kinds_have_source_owners() {
    for kind in [
        DynamicFunctionKind::Normal,
        DynamicFunctionKind::Async,
        DynamicFunctionKind::Generator,
        DynamicFunctionKind::AsyncGenerator,
    ] {
        let mut vm = page();
        let body = if matches!(
            kind,
            DynamicFunctionKind::Generator | DynamicFunctionKind::AsyncGenerator
        ) {
            "yield 1;"
        } else {
            "return 1;"
        };
        let function = vm
            .dynamic_function_constructor(&[Value::String(body.into())], kind)
            .unwrap()
            .object_id()
            .unwrap();
        let code = vm.heap.closure_code(function).unwrap().unwrap();
        assert!(code.runtime_source_bytes() > 0);
    }
}

#[cfg(test)]
#[test]
fn dynamic_function_storage_empty_refusal_precedes_parse_and_compile() {
    let mut vm = page();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let (result, allocations) = allocation_observer::count(|| vm.function_constructor(&[]));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_dynamic_function_storage_contracts() {
        dynamic_function_storage_code_keeps_its_lease_after_vm_drop();
        dynamic_function_storage_all_four_kinds_have_source_owners();
        dynamic_function_storage_ast_outlives_vm();
        dynamic_function_storage_original_and_nested_sources_are_independent();
        dynamic_function_storage_foreign_configuration_refuses();
        dynamic_function_storage_unbounded_ast_attaches_before_configuration();
        dynamic_function_storage_encoded_text_matches_legacy_encoding();
        dynamic_function_storage_text_growth_has_exact_and_one_short_admission();
        dynamic_function_storage_text_outlives_vm();
        dynamic_function_storage_terminal_text_reuse_refuses();
        dynamic_function_storage_foreign_text_reuse_refuses();
        dynamic_function_storage_capture_scratch_outlives_vm();
        dynamic_function_storage_collected_function_refunds_code_and_source();
        dynamic_function_storage_timer_keeps_code_until_delivery();
        dynamic_function_storage_promise_keeps_code_until_delivery();
        dynamic_function_storage_public_four_kind_execution();
        dynamic_function_storage_throwing_prototype_getter_cleans_up();
        dynamic_function_storage_coercion_order_and_html_source_are_preserved();
        dynamic_function_storage_malformed_wrapper_refunds_inputs();
        dynamic_function_storage_existing_constructor_code_attaches_on_activation();
        dynamic_function_storage_constructor_peak_has_exact_and_one_short_admission();
    }
}

fn leave_room(vm: &mut Vm, room: usize) -> host_scheduling::SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_ast_outlives_vm() {
    let vm = page();
    let view = observer(&vm);
    let input = ParsedFunctionInput::parse_wrapper(
        "function anonymous(){return 1;}",
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let bytes = input.bytes();
    assert_eq!(bytes, input.owned_heap_payload_bytes().unwrap());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_original_and_nested_sources_are_independent() {
    let vm = page();
    let view = observer(&vm);
    let mut input = ParsedFunctionInput::parse_wrapper(
        "function anonymous(a=()=>1){return ()=>2;}",
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let original = "function anonymous(a=()=>1<!-- original\n){return ()=>2;}";
    input
        .configure(
            DYNAMIC_FUNCTION_BINDING,
            Some(original),
            vm.host_scheduling.as_ref(),
        )
        .unwrap();
    let code = input
        .compile_function(DYNAMIC_FUNCTION_BINDING, vm.host_scheduling.as_ref())
        .unwrap();
    assert_eq!(code.source_text.as_str(), Some(original));
    assert_eq!(code.functions.len(), 2);
    let nested = code.functions[1].clone();
    assert!(!std::sync::Arc::ptr_eq(
        code.source_text.allocation().unwrap(),
        nested.source_text.allocation().unwrap()
    ));
    drop(vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    drop(code);
    assert!(view.host_scheduling_snapshot().payload_bytes < before);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        nested.runtime_source_bytes() + nested.retained_bytes()
    );
    drop(nested);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_foreign_configuration_refuses() {
    let source = page();
    let target = page();
    let mut input = ParsedFunctionInput::parse_wrapper(
        "function anonymous(){return 1;}",
        source.host_scheduling.as_ref(),
    )
    .unwrap();
    assert_eq!(
        input.configure(
            DYNAMIC_FUNCTION_BINDING,
            None,
            target.host_scheduling.as_ref()
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    source.check_scheduling_failure().unwrap();
    assert_eq!(
        target.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_unbounded_ast_attaches_before_configuration() {
    let vm = page();
    let view = observer(&vm);
    let mut input =
        ParsedFunctionInput::parse_wrapper("function anonymous(){return 1;}", None).unwrap();
    input
        .configure(DYNAMIC_FUNCTION_BINDING, None, vm.host_scheduling.as_ref())
        .unwrap();
    let code = input
        .compile_function(DYNAMIC_FUNCTION_BINDING, vm.host_scheduling.as_ref())
        .unwrap();
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_encoded_text_matches_legacy_encoding() {
    for value in [
        JsString::from("ascii"),
        JsString::from("π😀"),
        JsString::from_code_units(vec![0xd800, 65, 0xdc00]),
        JsString::from("\u{10f000}\u{10f800}"),
    ] {
        let vm = page();
        let expected = crate::source_encoding::encode(&value);
        assert_eq!(
            crate::source_encoding::encoded_bytes(&value),
            Some(expected.len())
        );
        let mut text = NativeText::empty();
        text.append_encoded(value, vm.host_scheduling.as_ref())
            .unwrap();
        assert_eq!(text.as_ref(), expected);
    }
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_text_growth_has_exact_and_one_short_admission() {
    let preview = page();
    let mut text = NativeText::parts(&["a"], preview.host_scheduling.as_ref()).unwrap();
    let scope = preview.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| text.append("bcdef", Some(&scope)));
    result.unwrap();
    assert!(peak >= 6);
    drop(text);
    drop(preview);
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let mut text = NativeText::parts(&["a"], vm.host_scheduling.as_ref()).unwrap();
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = text.append("bcdef", vm.host_scheduling.as_ref());
        if accepts {
            result.unwrap();
            assert_eq!(text.as_ref(), "abcdef");
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(text.as_ref(), "a");
        }
        drop(text);
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_text_outlives_vm() {
    let vm = page();
    let view = observer(&vm);
    let text = NativeText::parts(&["tiny"], vm.host_scheduling.as_ref()).unwrap();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= text.len());
    drop(text);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_terminal_text_reuse_refuses() {
    let vm = page();
    let mut text = NativeText::parts(&["tiny"], vm.host_scheduling.as_ref()).unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        text.append("", vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(text.as_ref(), "tiny");
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_foreign_text_reuse_refuses() {
    let source = page();
    let target = page();
    let mut text = NativeText::parts(&["tiny"], source.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        text.append("", target.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(text.as_ref(), "tiny");
    source.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_capture_scratch_outlives_vm() {
    let vm = page();
    let view = observer(&vm);
    let captures = NativeCaptures::new(2, vm.host_scheduling.as_ref()).unwrap();
    let bytes = captures.values.capacity() * std::mem::size_of::<ObjectId>();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(captures);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_collected_function_refunds_code_and_source() {
    let mut vm = page();
    let function = vm
        .function_constructor(&[Value::String("return 1;".into())])
        .unwrap()
        .object_id()
        .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(code);
    let root = vm.heap.root(function).unwrap();
    vm.collect_realm_garbage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes - vm.heap.collector_storage_bytes();
    vm.heap.unroot(root).unwrap();
    vm.collect_realm_garbage().unwrap();
    let after = vm.host_scheduling_snapshot().payload_bytes - vm.heap.collector_storage_bytes();
    assert_eq!(before - after, bytes);
}

fn queued_function(timer: bool) {
    let mut vm = page();
    if timer {
        vm.install_host_timer_functions().unwrap();
    }
    let function = execute(
        &mut vm,
        "var result=0; var callback=Function('result+=1;'); callback;",
    )
    .object_id()
    .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    let source = std::sync::Arc::downgrade(code.source_text.allocation().unwrap());
    assert!(code.runtime_source_bytes() > 0);
    drop(code);
    execute(
        &mut vm,
        if timer {
            "setTimeout(callback,1);callback=undefined;0;"
        } else {
            "Promise.resolve().then(callback);callback=undefined;0;"
        },
    );
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(function));
    if timer {
        assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    } else {
        vm.run_promise_jobs_bounded(4).unwrap();
    }
    assert_eq!(execute(&mut vm, "result;"), Value::Number(1.0));
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(function));
    assert!(source.upgrade().is_none());
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_timer_keeps_code_until_delivery() {
    queued_function(true);
}
#[cfg_attr(test, test)]
fn dynamic_function_storage_promise_keeps_code_until_delivery() {
    queued_function(false);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_public_four_kind_execution() {
    let mut vm = page();
    execute(&mut vm, "var result=Function('return 1;')(); var GF=Object.getPrototypeOf(function*(){}).constructor; result+=GF('yield 2;')().next().value; var AF=Object.getPrototypeOf(async function(){}).constructor; AF('return 3;')().then(v=>result+=v); var AGF=Object.getPrototypeOf(async function*(){}).constructor; AGF('yield 4;')().next().then(r=>result+=r.value);0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(execute(&mut vm, "result;"), Value::Number(10.0));
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_throwing_prototype_getter_cleans_up() {
    let mut vm = page();
    let view = observer(&vm);
    assert_eq!(execute(&mut vm, "var n=new Proxy(function(){},{get(t,k){if(k==='prototype')throw 42;return Reflect.get(t,k);}}); var error=0;try{Reflect.construct(Function,['return 1;'],n);}catch(e){error=e;}error;"), Value::Number(42.0));
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_coercion_order_and_html_source_are_preserved() {
    let mut vm = page();
    assert_eq!(execute(&mut vm, "var calls=[]; var f=Function({toString(){calls.push('p');return 'π<!-- 註解\\n';}},{toString(){calls.push('b');return 'return π+1;';}}); calls.join(',')==='p,b' && f(4)===5 && f.toString().includes('<!-- 註解');"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_malformed_wrapper_refunds_inputs() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert!(matches!(
        vm.function_constructor(&[Value::String("} function f() {".into())]),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_existing_constructor_code_attaches_on_activation() {
    let mut vm = Vm::default();
    let function = execute(&mut vm, "var existing=Function('return 1;'); existing;")
        .object_id()
        .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn dynamic_function_storage_constructor_peak_has_exact_and_one_short_admission() {
    let mut preview = page();
    preview.function_prototype().unwrap();
    let scope = preview.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| {
        preview.function_constructor(&[Value::String("return 1;".into())])
    });
    result.unwrap();
    assert!(peak > 0);
    drop(preview);
    for accepts in [false, true] {
        let mut vm = page();
        vm.function_prototype().unwrap();
        let view = observer(&vm);
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = vm.function_constructor(&[Value::String("return 1;".into())]);
        if accepts {
            result.unwrap();
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        drop(vm);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}
