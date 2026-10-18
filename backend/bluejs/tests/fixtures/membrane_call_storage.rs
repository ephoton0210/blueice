// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed membrane calls observe ownership through legitimate ancestor callbacks.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "var realm=$262.createRealm();var invoke=realm.evalScript('(text,callback)=>callback()');0;");
    vm
}
fn invoke(vm: &mut Vm, args: Vec<Value>) -> Value {
    let callee = vm.lookup_global_name("invoke").unwrap().unwrap();
    vm.call_native(callee, Value::Undefined, args, false)
        .unwrap()
}
fn probe(vm: &mut Vm) -> Value {
    vm.lookup_global_name("probe").unwrap().unwrap()
}

#[cfg_attr(test, test)]
fn membrane_call_storage_forward_reuses_the_incoming_vector() {
    let mut vm = page();
    let caller = vm.object_prototype.heap;
    let child = vm.test262_realms.values().next().unwrap().vm.heap();
    let mut args = Vec::with_capacity(7);
    let address = args.as_ptr() as usize;
    let capacity = args.capacity();
    vm.install_host_function("probe", 0, move |_: &[HostValue]| {
        let same = realm_reentrancy::with_active_vm(caller, child, |vm| {
            Ok(vm.arguments.as_ptr() as usize == address && vm.arguments.capacity() == capacity)
        })
        .unwrap();
        Ok(HostValue::Bool(same))
    })
    .unwrap();
    args.push(Value::String("ab".into()));
    args.push(probe(&mut vm));
    assert_eq!(invoke(&mut vm, args), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn membrane_call_storage_forward_moves_the_primitive_buffer() {
    let mut vm = page();
    let caller = vm.object_prototype.heap;
    let child = vm.test262_realms.values().next().unwrap().vm.heap();
    let text = JsString::from("ab");
    let address = text.as_code_units().as_ptr() as usize;
    vm.install_host_function("probe", 0, move |_: &[HostValue]| {
        let same = realm_reentrancy::with_active_vm(caller, child, |vm| {
            let Value::String(text) = &vm.arguments[0] else {
                panic!("text argument")
            };
            Ok(text.as_code_units().as_ptr() as usize == address)
        })
        .unwrap();
        Ok(HostValue::Bool(same))
    })
    .unwrap();
    let callback = probe(&mut vm);
    assert_eq!(
        invoke(&mut vm, vec![Value::String(text), callback]),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn membrane_call_storage_forward_releases_the_source_entry_counter() {
    let mut vm = page();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("probe", 0, move |_: &[HostValue]| {
        Ok(HostValue::Number(counter.get() as f64))
    })
    .unwrap();
    let mut args = Vec::with_capacity(7);
    args.push(Value::String("ab".into()));
    args.push(probe(&mut vm));
    assert_eq!(invoke(&mut vm, args), Value::Number(0.0));
}

#[cfg_attr(test, test)]
fn membrane_call_storage_reverse_releases_the_source_entry_counter() {
    let mut vm = page();
    run(&mut vm, "var empty=realm.evalScript('(callback)=>callback()');var withArg=realm.evalScript('(callback)=>callback(\"ab\")');0;");
    let counter = vm
        .test262_realms
        .values()
        .next()
        .unwrap()
        .vm
        .borrow()
        .displaced_execution_control_bytes
        .clone();
    vm.install_host_function("reverseCredit", 0, move |_: &[HostValue]| {
        Ok(HostValue::Number(counter.get() as f64))
    })
    .unwrap();
    let baseline = run(&mut vm, "empty(function(){return reverseCredit();});");
    let with_input = run(&mut vm, "withArg(function(text){return reverseCredit();});");
    assert_eq!(with_input, baseline);
}

fn assert_finished(vm: &Vm) {
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.pending_tail_call.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    for realm in vm.test262_realms.values() {
        let child = realm.vm.borrow();
        assert!(child.realm_call_roots.is_none());
        assert!(child.pending_tail_call.is_none());
        assert_eq!(child.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn membrane_call_storage_forward_finishes_strict_tail_calls() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var f=realm.evalScript('(function(n){"use strict";return Math.abs(n);})');
        f(-7);
    "#
        ),
        Value::Number(7.0)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_reverse_finishes_tail_calls_across_callback_gc() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var f=realm.evalScript('(function(callback){"use strict";return callback("ab");})');
        f(function(text){"use strict";$262.gc();return Math.abs(-text.length);});
    "#
        ),
        Value::Number(2.0)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_source_roots_keep_earlier_transports_through_conversion_gc() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var f=realm.evalScript('(first,second)=>first()+second[0]');
        var second=[];
        Object.defineProperty(second,'0',{get:function(){f=null;realm=null;$262.gc();return 2;}});
        f(()=>7,second);
    "#
        ),
        Value::Number(9.0)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_conversion_throw_restores_roots_and_credit() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var f=realm.evalScript('(first,second)=>first()+second[0]');
        var second=[];
        Object.defineProperty(second,'0',{get:function(){throw 42;}});
        var caught=false;
        try { f(()=>7,second); } catch(error) { caught=error===42; }
        caught;
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
    vm.collect_realm_garbage().unwrap();
}

#[cfg_attr(test, test)]
fn membrane_call_storage_forward_constructor_preserves_the_callee_prototype() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var C=realm.evalScript('(class C{constructor(text){this.text=text;}})');
        var object=new C('ab');
        object.text==='ab' && Object.getPrototypeOf(object)===C.prototype;
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_forward_constructor_preserves_the_callers_new_target() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var D=realm.evalScript('Date');
        function N(){}
        var object=Reflect.construct(D,[0],N);
        Object.getPrototypeOf(object)===N.prototype;
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_reverse_constructor_restores_the_parent_object() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var construct=realm.evalScript('(Ctor)=>new Ctor("ab")');
        class C{constructor(text){this.text=text;}}
        var object=construct(C);
        object instanceof C && object.text==='ab';
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_throws_keep_their_original_error_realms() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var childError=realm.evalScript('()=>{throw new RangeError("child");}');
        var forward=false;
        try { childError(); } catch(error) { forward=error instanceof realm.global.RangeError && !(error instanceof RangeError); }
        var reverse=realm.evalScript('(callback,ParentError)=>{try{callback();}catch(error){return error instanceof ParentError && !(error instanceof RangeError);}}');
        forward && reverse(()=>{throw new RangeError('parent');},RangeError);
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_borrowed_native_routes_preserve_local_array_operands() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var join=realm.evalScript('Array.prototype.join');
        var map=realm.evalScript('Array.prototype.map');
        join.call([1,2],'-')==='1-2'
          && Reflect.apply(join,[3,4],[':'])==='3:4'
          && map.call([1,2],value=>value+1).join('-')==='2-3';
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_typed_array_call_routes_preserve_local_slots() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var slice=realm.evalScript('Uint8Array.prototype.slice');
        var set=realm.evalScript('Uint8Array.prototype.set');
        var local=new Uint8Array([1,2]);
        var result=slice.call(local,1);
        set.call(local,[5],1);
        result[0]===2 && local[1]===5;
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_reexports_preserve_identity_across_sibling_realms() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var sibling=$262.createRealm();
        var first=realm.evalScript('(value)=>value');
        var second=sibling.evalScript('(value)=>value');
        var value=realm.evalScript('({answer:7})');
        first(value)===value && second(value)===value;
    "#
        ),
        Value::Bool(true)
    );
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_neutral_calls_preserve_their_captured_policy() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var realm=$262.createRealm();
        var call=realm.evalScript('(function(callback){"use strict";return callback("ab");})');
        call(function(text){"use strict";return text.length;});
    "#
        ),
        Value::Number(2.0)
    );
    assert!(vm.host_scheduling.is_none());
    assert!(vm
        .test262_realms
        .values()
        .all(|realm| realm.vm.borrow().host_scheduling.is_none()));
    assert_finished(&vm);
}

#[cfg_attr(test, test)]
fn membrane_call_storage_explicit_terminal_failure_unwinds_both_directions() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        r#"var call=realm.evalScript('(function(callback){"use strict";return callback();})');0;"#,
    );
    let callee = vm.lookup_global_name("call").unwrap().unwrap();
    let callback = vm.lookup_global_name("markTerminal").unwrap().unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, vec![callback], false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_finished(&vm);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_membrane_call_storage_contracts() {
        membrane_call_storage_forward_reuses_the_incoming_vector();
        membrane_call_storage_forward_moves_the_primitive_buffer();
        membrane_call_storage_forward_releases_the_source_entry_counter();
        membrane_call_storage_reverse_releases_the_source_entry_counter();
        membrane_call_storage_forward_finishes_strict_tail_calls();
        membrane_call_storage_reverse_finishes_tail_calls_across_callback_gc();
        membrane_call_storage_source_roots_keep_earlier_transports_through_conversion_gc();
        membrane_call_storage_conversion_throw_restores_roots_and_credit();
        membrane_call_storage_forward_constructor_preserves_the_callee_prototype();
        membrane_call_storage_forward_constructor_preserves_the_callers_new_target();
        membrane_call_storage_reverse_constructor_restores_the_parent_object();
        membrane_call_storage_throws_keep_their_original_error_realms();
        membrane_call_storage_borrowed_native_routes_preserve_local_array_operands();
        membrane_call_storage_typed_array_call_routes_preserve_local_slots();
        membrane_call_storage_reexports_preserve_identity_across_sibling_realms();
        membrane_call_storage_neutral_calls_preserve_their_captured_policy();
        membrane_call_storage_explicit_terminal_failure_unwinds_both_directions();
    }
}
