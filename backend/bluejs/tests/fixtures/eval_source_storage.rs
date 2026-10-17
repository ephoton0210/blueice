// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny direct/indirect eval code and input lifetimes.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 10_000;
    vm.global("globalThis").unwrap();
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn retained_eval(indirect: bool) {
    let mut vm = page();
    let view = observer(&vm);
    let source = Value::String("(function kept(){return 1;})".into());
    let value = if indirect {
        vm.indirect_eval(&source)
    } else {
        vm.direct_eval(&source)
    }
    .unwrap();
    let code = vm
        .heap
        .closure_code(value.object_id().unwrap())
        .unwrap()
        .unwrap();
    let minimum = code.retained_bytes() + code.source_text.allocation().unwrap().len();
    drop(vm);
    assert!(view.host_scheduling_snapshot().payload_bytes >= minimum);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn eval_source_storage_direct_code_outlives_vm() {
    retained_eval(false);
}
#[cfg_attr(test, test)]
fn eval_source_storage_indirect_code_outlives_vm() {
    retained_eval(true);
}

#[cfg(test)]
fn empty_refusal(indirect: bool) {
    let mut vm = page();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let source = Value::String("".into());
    let (result, allocations) = allocation_observer::count(|| {
        if indirect {
            vm.indirect_eval(&source)
        } else {
            vm.direct_eval(&source)
        }
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
}
#[cfg(test)]
#[test]
fn eval_source_storage_direct_refusal_precedes_parse_and_compile() {
    empty_refusal(false);
}
#[cfg(test)]
#[test]
fn eval_source_storage_indirect_refusal_precedes_parse_and_compile() {
    empty_refusal(true);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_eval_source_storage_contracts() {
        eval_source_storage_direct_code_outlives_vm();
        eval_source_storage_indirect_code_outlives_vm();
        eval_source_storage_ast_and_root_owners_have_separate_lifetimes();
        eval_source_storage_foreign_ast_refuses_before_compile();
        eval_source_storage_unbounded_ast_attaches_to_code();
        eval_source_storage_non_string_inputs_need_no_code_budget();
        eval_source_storage_template_sources_release_independently();
        eval_source_storage_compiler_environment_outlives_vm();
        eval_source_storage_environment_has_exact_and_one_short_admission();
        eval_source_storage_capture_capacity_is_released_before_guest_code();
        eval_source_storage_direct_and_indirect_keep_scope();
        eval_source_storage_strict_and_private_contexts_keep_working();
        eval_source_storage_syntax_error_refunds_input();
        eval_source_storage_throw_restores_caller_bindings();
        eval_source_storage_timer_keeps_code_until_delivery();
        eval_source_storage_promise_keeps_code_until_delivery();
        eval_source_storage_collected_function_refunds_native_code();
        eval_source_storage_existing_unbounded_code_attaches_once();
    }
}

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn compile_input(vm: &Vm, source: &str) -> crate::vm::eval_compilation_storage::PreparedEvalCode {
    ParsedEvalInput::parse_script(source, vm.host_scheduling.as_ref())
        .unwrap()
        .compile_with(vm.host_scheduling.as_ref(), |program| {
            crate::compiler::compile_eval(
                program,
                &[],
                &[],
                &[],
                crate::compiler::EvalContext::default(),
                crate::CompileLimits::default(),
            )
        })
        .unwrap()
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
fn eval_source_storage_ast_and_root_owners_have_separate_lifetimes() {
    let vm = page();
    let view = observer(&vm);
    let input = ParsedEvalInput::parse_eval(
        "function f(){return 1;} f;",
        false,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    assert_eq!(input.bytes(), input.owned_heap_payload_bytes().unwrap());
    let code = input
        .compile_with(vm.host_scheduling.as_ref(), |program| {
            crate::compiler::compile_eval(
                program,
                &[],
                &[],
                &[],
                crate::compiler::EvalContext::default(),
                crate::CompileLimits::default(),
            )
        })
        .unwrap();
    let child = code.functions[0].clone();
    let child_bytes = child.retained_bytes() + child.runtime_source_bytes();
    let root_bytes = host_scheduling::owned_code_bytes(&code);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        child_bytes + root_bytes
    );
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, child_bytes);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn eval_source_storage_foreign_ast_refuses_before_compile() {
    let source = page();
    let target = page();
    let input = ParsedEvalInput::parse_script("0;", source.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        input
            .compile_with(target.host_scheduling.as_ref(), |_| panic!(
                "foreign input must not enter the compiler"
            ))
            .err(),
        Some(RuntimeError::SchedulingLimit)
    );
    source.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn eval_source_storage_unbounded_ast_attaches_to_code() {
    let vm = page();
    let view = observer(&vm);
    let input = ParsedEvalInput::parse_script("(function(){return 1;});", None).unwrap();
    let code = input
        .compile_with(vm.host_scheduling.as_ref(), crate::compile)
        .unwrap();
    let child = code.functions[0].clone();
    let expected = child.retained_bytes() + child.runtime_source_bytes();
    drop(code);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(child);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn eval_source_storage_non_string_inputs_need_no_code_budget() {
    let mut vm = page();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(vm.direct_eval(&Value::Number(7.0)), Ok(Value::Number(7.0)));
    assert_eq!(vm.indirect_eval(&Value::Bool(true)), Ok(Value::Bool(true)));
    vm.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn eval_source_storage_template_sources_release_independently() {
    let vm = page();
    let view = observer(&vm);
    let code = compile_input(&vm, "function f(){return 1;} `${(()=>2)()}`;");
    assert_eq!(code.functions.len(), 2);
    let main = code.functions[0].clone();
    let fragment = code.functions[1].clone();
    assert!(!std::sync::Arc::ptr_eq(
        main.source_text.allocation().unwrap(),
        fragment.source_text.allocation().unwrap()
    ));
    drop(code);
    drop(vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    drop(main);
    assert!(view.host_scheduling_snapshot().payload_bytes < before);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        fragment.retained_bytes() + fragment.runtime_source_bytes()
    );
    drop(fragment);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

fn binding() -> Binding {
    Binding {
        name: "x".into(),
        mutable: true,
        strict_immutable: false,
        lexical: false,
        catch_parameter: false,
        eval_var: false,
    }
}
type EnvironmentParts = (
    Vec<(String, Binding, u32)>,
    HashMap<u32, ObjectId>,
    Vec<String>,
    Vec<String>,
);
fn environment_parts(vm: &mut Vm) -> EnvironmentParts {
    let cell = vm.heap.alloc_object(None).unwrap();
    (
        vec![("x".into(), binding(), 0)],
        HashMap::from([(0, cell)]),
        vec!["x".into()],
        vec!["blocked".into()],
    )
}
fn environment_bytes(parts: &EnvironmentParts) -> usize {
    parts.0.capacity() * std::mem::size_of::<(String, Binding, u32)>()
        + parts
            .0
            .iter()
            .map(|(name, binding, _)| name.capacity() + binding.name.capacity())
            .sum::<usize>()
        + parts.1.capacity() * std::mem::size_of::<(u32, ObjectId)>()
        + parts.2.capacity() * std::mem::size_of::<String>()
        + parts.2.iter().map(String::capacity).sum::<usize>()
        + parts.3.capacity() * std::mem::size_of::<String>()
        + parts.3.iter().map(String::capacity).sum::<usize>()
}

#[cfg_attr(test, test)]
fn eval_source_storage_compiler_environment_outlives_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let parts = environment_parts(&mut vm);
    let bytes = environment_bytes(&parts);
    let environment = EvalCompileEnvironment::adopt(
        parts.0,
        parts.1,
        parts.2,
        parts.3,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(environment);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn eval_source_storage_environment_has_exact_and_one_short_admission() {
    for accepts in [false, true] {
        let mut vm = page();
        let parts = environment_parts(&mut vm);
        let bytes = environment_bytes(&parts);
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let result = EvalCompileEnvironment::adopt(
            parts.0,
            parts.1,
            parts.2,
            parts.3,
            vm.host_scheduling.as_ref(),
        );
        if accepts {
            drop(result.unwrap());
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
    }
}

#[cfg_attr(test, test)]
fn eval_source_storage_capture_capacity_is_released_before_guest_code() {
    let mut observed = Vec::new();
    for capacity in [1, 4] {
        let mut vm = page();
        let view = observer(&vm);
        let output = Rc::new(RefCell::new(Vec::new()));
        let writer = output.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            writer
                .borrow_mut()
                .push(view.host_scheduling_snapshot().payload_bytes);
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let cell = vm.heap.alloc_object(None).unwrap();
        vm.heap.set(cell, "value", Value::Number(7.0)).unwrap();
        let root = vm.heap.root(cell).unwrap();
        let code = crate::compiler::compile_eval(
            &crate::parse_eval("observe(); x;", false).unwrap(),
            &[("x".into(), binding(), 0)],
            &[],
            &[],
            crate::compiler::EvalContext::default(),
            crate::CompileLimits::default(),
        )
        .unwrap();
        let mut captures = Vec::with_capacity(capacity);
        captures.push(cell);
        let captures = NativeCaptures::adopt(captures, vm.host_scheduling.as_ref()).unwrap();
        assert_eq!(
            vm.execute_eval_owned(&code, captures, false).unwrap(),
            Value::Number(7.0)
        );
        vm.heap.unroot(root).unwrap();
        observed.push(output.borrow().clone());
    }
    assert_eq!(observed[0].len(), 1);
    assert_eq!(observed[0], observed[1]);
}

#[cfg_attr(test, test)]
fn eval_source_storage_direct_and_indirect_keep_scope() {
    let mut vm = page();
    assert_eq!(execute(&mut vm, "var x=9; function outer(){let x=4;let direct=eval('(function(){return x;})');let indirect=(0,eval)('(function(){return x;})');return direct()+indirect();}outer();"), Value::Number(13.0));
}

#[cfg_attr(test, test)]
fn eval_source_storage_strict_and_private_contexts_keep_working() {
    let mut vm = page();
    assert_eq!(execute(&mut vm, "class Base{read(){return 2;}}class Child extends Base{#x=5;read(){return eval('this.#x+super.read()');}}new Child().read();"), Value::Number(7.0));
    assert_eq!(execute(&mut vm, "function strict(){'use strict';let x=3;return eval('(function(){return x;})')();}strict();"), Value::Number(3.0));
}

#[cfg_attr(test, test)]
fn eval_source_storage_syntax_error_refunds_input() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert!(matches!(
        vm.direct_eval(&Value::String("let = ;".into())),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(matches!(
        vm.indirect_eval(&Value::String("let = ;".into())),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn eval_source_storage_throw_restores_caller_bindings() {
    let mut vm = page();
    assert_eq!(
        execute(
            &mut vm,
            "function outer(){let x=4;try{eval('throw 7');}catch(e){return x+e;}}outer();"
        ),
        Value::Number(11.0)
    );
    assert_eq!(
        execute(
            &mut vm,
            "var value=8;try{(0,eval)('throw 3');}catch(e){value+=e;}value;"
        ),
        Value::Number(11.0)
    );
}

fn queued_eval(indirect: bool) {
    let mut vm = page();
    if !indirect {
        vm.install_host_timer_functions().unwrap();
    }
    let function = execute(
        &mut vm,
        if indirect {
            "var result=0;var callback=(0,eval)('(function(){result+=1;})');callback;"
        } else {
            "var result=0;var callback=eval('(function(){result+=1;})');callback;"
        },
    )
    .object_id()
    .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    assert!(code.runtime_source_bytes() > 0);
    let source = std::sync::Arc::downgrade(code.source_text.allocation().unwrap());
    drop(code);
    execute(
        &mut vm,
        if indirect {
            "Promise.resolve().then(callback);callback=undefined;0;"
        } else {
            "setTimeout(callback,1);callback=undefined;0;"
        },
    );
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(function));
    if indirect {
        vm.run_promise_jobs_bounded(4).unwrap();
    } else {
        assert_eq!(vm.run_due_host_timers(1).unwrap().tasks_run, 1);
    }
    assert_eq!(execute(&mut vm, "result;"), Value::Number(1.0));
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(function));
    assert!(source.upgrade().is_none());
}
#[cfg_attr(test, test)]
fn eval_source_storage_timer_keeps_code_until_delivery() {
    queued_eval(false);
}
#[cfg_attr(test, test)]
fn eval_source_storage_promise_keeps_code_until_delivery() {
    queued_eval(true);
}

#[cfg_attr(test, test)]
fn eval_source_storage_collected_function_refunds_native_code() {
    let mut vm = page();
    let function = vm
        .indirect_eval(&Value::String("(function(){return 1;})".into()))
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

#[cfg_attr(test, test)]
fn eval_source_storage_existing_unbounded_code_attaches_once() {
    let mut vm = Vm::default();
    let function = execute(
        &mut vm,
        "var existing=eval('(function(){return 1;})');existing;",
    )
    .object_id()
    .unwrap();
    let code = vm.heap.closure_code(function).unwrap().unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let bytes = code.retained_bytes() + code.runtime_source_bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
