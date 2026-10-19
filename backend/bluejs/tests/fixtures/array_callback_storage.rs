// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny common Array callback ownership, cancellation, and GC contracts.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn buffered_string() -> Value {
    Value::String(JsString::from_code_units(vec![0x61; 32]))
}

fn arguments(vm: &Vm, value: Value, object: ObjectId) -> TransportVector<Value> {
    let owner = vm.argument_list_owner().unwrap();
    vm.array_callback_arguments(
        NativeValue::adopt(value, owner.as_ref()).unwrap(),
        0,
        object,
    )
    .unwrap()
}

fn retained(arguments: &TransportVector<Value>) -> usize {
    arguments.retained_slots() + host_scheduling::values_bytes(arguments)
}

const METHODS: [ArrayCallbackMethod; 9] = [
    ArrayCallbackMethod::ForEach,
    ArrayCallbackMethod::Filter,
    ArrayCallbackMethod::Map,
    ArrayCallbackMethod::Every,
    ArrayCallbackMethod::Some,
    ArrayCallbackMethod::Find {
        reverse: false,
        return_index: false,
    },
    ArrayCallbackMethod::Find {
        reverse: false,
        return_index: true,
    },
    ArrayCallbackMethod::Find {
        reverse: true,
        return_index: false,
    },
    ArrayCallbackMethod::Find {
        reverse: true,
        return_index: true,
    },
];

fn loaned_call(
    source: &Vm,
    neutral: &mut Vm,
    receiver: &Value,
    callback: &Value,
    method: ArrayCallbackMethod,
) -> Result<Value, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = RealmCallRoots::new(2, owner.as_ref())?;
    roots.push(receiver.object_id().unwrap());
    roots.push(callback.object_id().unwrap());
    neutral.with_realm_call_roots(roots, |vm| {
        vm.array_callback_method(receiver, callback, &Value::Undefined, method)
    })
}

#[cfg_attr(test, test)]
fn array_callback_storage_arguments_outlive_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "[1];").object_id().unwrap();
    let arguments = arguments(&vm, buffered_string(), object);
    let bytes = retained(&arguments);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_callback_storage_neutral_early_return_checks_original_owner() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Bool(true))
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "var receiver=[1],callback=()=>markTerminal();receiver;",
    );
    let callback = neutral.lookup_global_name("callback").unwrap().unwrap();
    let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots.push(receiver.object_id().unwrap());
    roots.push(callback.object_id().unwrap());
    let result = neutral.with_realm_call_roots(roots, |vm| {
        vm.array_some(&receiver, &callback, &Value::Undefined)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(neutral.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn array_callback_storage_find_keeps_original_object_across_delete_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(run(&mut vm,"var source=[{tag:7}];var result=source.find(()=>{delete source[0];$262.gc();return true;});result.tag===7;"),Value::Bool(true));
}

#[cfg_attr(test, test)]
fn array_callback_storage_holes_and_directions_preserve_callback_order() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[,2,,4];function visits(method){var log='';source[method]((value,index)=>{log+=index;return method==='every';});return log;}
        ['forEach','map','filter','every','some'].every(method=>visits(method)==='13')&&
        ['find','findIndex'].every(method=>visits(method)==='0123')&&['findLast','findLastIndex'].every(method=>visits(method)==='3210');
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_callback_storage_filter_retains_original_object_across_delete_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(run(&mut vm,"var source=[{tag:7}];var result=source.filter(()=>{delete source[0];$262.gc();return true;});result[0].tag===7;"),Value::Bool(true));
}

#[cfg_attr(test, test)]
fn array_callback_storage_argument_moves_conserve_actual_credit() {
    let mut vm = page();
    let object = run(&mut vm, "[1];").object_id().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let arguments = arguments(&vm, buffered_string(), object);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + retained(&arguments)
    );
    drop(arguments);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn array_callback_storage_neutral_builder_keeps_the_input_owner() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "[1];").object_id().unwrap();
    let arguments = neutral
        .array_callback_arguments(
            NativeValue::adopt(buffered_string(), owner.as_ref()).unwrap(),
            0,
            object,
        )
        .unwrap();
    let bytes = retained(&arguments);
    assert!(neutral.host_scheduling.is_none());
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_callback_storage_root_loan_does_not_install_document_policy() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "[1];").object_id().unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(object);
    let arguments = neutral
        .with_realm_call_roots(roots, |vm| Ok(arguments(vm, buffered_string(), object)))
        .unwrap();
    let bytes = retained(&arguments);
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_callback_storage_all_methods_check_a_terminal_callback_owner() {
    for method in METHODS {
        let source = page();
        let view = observer(&source);
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let mut neutral = Vm::default();
        let selected = !matches!(method, ArrayCallbackMethod::Every);
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Bool(selected))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            "var receiver=[1],callback=()=>markTerminal();receiver;",
        );
        let callback = neutral.lookup_global_name("callback").unwrap().unwrap();
        assert_eq!(
            loaned_call(&source, &mut neutral, &receiver, &callback, method),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
        assert!(neutral.stack.is_empty());
        drop(source);
        drop(neutral);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn array_callback_storage_terminal_length_getter_never_enters_the_callback() {
    for method in METHODS {
        let source = page();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let entered = Rc::new(std::cell::Cell::new(false));
        let output = entered.clone();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Number(1.0))
            })
            .unwrap();
        neutral
            .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
                output.set(true);
                Ok(crate::HostValue::Bool(true))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            "var receiver={get length(){return markTerminal();},0:1},callback=()=>observe();receiver;",
        );
        let callback = neutral.lookup_global_name("callback").unwrap().unwrap();
        assert_eq!(
            loaned_call(&source, &mut neutral, &receiver, &callback, method),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!entered.get());
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_callback_storage_terminal_index_getter_never_enters_the_callback() {
    for method in METHODS {
        let source = page();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let entered = Rc::new(std::cell::Cell::new(false));
        let output = entered.clone();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Number(1.0))
            })
            .unwrap();
        neutral
            .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
                output.set(true);
                Ok(crate::HostValue::Bool(true))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            "var receiver={length:1,get 0(){return markTerminal();}},callback=()=>observe();receiver;",
        );
        let callback = neutral.lookup_global_name("callback").unwrap().unwrap();
        assert_eq!(
            loaned_call(&source, &mut neutral, &receiver, &callback, method),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!entered.get());
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_callback_storage_callback_throw_restores_the_outer_roots_and_stack() {
    for method in METHODS {
        let mut vm = page();
        let view = observer(&vm);
        let receiver = run(
            &mut vm,
            "var receiver=[1],callback=()=>{throw 7;};receiver;",
        );
        let callback = vm.lookup_global_name("callback").unwrap().unwrap();
        let owner = vm.argument_list_owner().unwrap();
        let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
        roots.push(receiver.object_id().unwrap());
        vm.stack.push(Value::Number(9.0));
        vm.with_realm_call_roots(roots.clone(), |vm| {
            assert_eq!(
                vm.array_callback_method(&receiver, &callback, &Value::Undefined, method),
                Err(RuntimeError::Thrown(Value::Number(7.0)))
            );
            assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &roots));
            assert_eq!(vm.stack, vec![Value::Number(9.0)]);
            Ok(())
        })
        .unwrap();
        assert!(vm.realm_call_roots.is_none());
        drop(roots);
        drop(owner);
        drop(vm);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn array_callback_storage_length_conversion_roots_callback_and_this_arg() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let receiver = run(
        &mut vm,
        r#"
        var receiver={0:2,get length(){return {valueOf(){callback=null;context=null;$262.gc();return 1;}};}};
        var callback=function(value){'use strict';return this.tag+value;},context={tag:5};receiver;
    "#,
    );
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    let context = vm.lookup_global_name("context").unwrap().unwrap();
    let result = vm.array_map(&receiver, &callback, &context).unwrap();
    assert_eq!(
        vm.heap.get(result.object_id().unwrap(), "0").unwrap(),
        Value::Number(7.0)
    );
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_callback_storage_public_live_mutations_preserve_captured_length() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var methods=['forEach','map','filter','every','some','find','findIndex','findLast','findLastIndex'];
        methods.every(method=>{
            var reverse=method==='findLast'||method==='findLastIndex',source=[1,2,3],log='';
            source[method]((value,index,array)=>{
                log+=index+':'+value+';';
                if(reverse&&index===2){array[1]=7;array.push(9);}
                if(!reverse&&index===0){array[1]=7;array.push(9);}
                return method==='every';
            });
            return log===(reverse?'2:3;1:7;0:1;':'0:1;1:7;2:3;');
        });
    "#,
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_callback_storage_public_proxy_has_and_get_order_is_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function visit(method){var log='',source=new Proxy({length:2,1:4},{
            get(target,key){log+='g'+key+';';return Reflect.get(target,key);},
            has(target,key){log+='h'+key+';';return Reflect.has(target,key);}
        });Array.prototype[method].call(source,()=>method==='every');return log;}
        ['forEach','map','filter','every','some'].every(method=>visit(method)==='glength;h0;h1;g1;')&&
        ['find','findIndex'].every(method=>visit(method)==='glength;g0;g1;')&&
        ['findLast','findLastIndex'].every(method=>visit(method)==='glength;g1;g0;');
    "#,
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_callback_storage_map_result_survives_species_definition_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1],keys='',flags=true;source.constructor={[Symbol.species]:function(length){
            return new Proxy({length},{get defineProperty(){
                $262.gc();return function(target,key,descriptor){
                    keys+=key;flags=flags&&descriptor.writable&&descriptor.enumerable&&descriptor.configurable;
                    $262.gc();return Reflect.defineProperty(target,key,descriptor);
                };
            }});
        }};
        var result=source.map(()=>({tag:7}));result[0].tag===7&&result.length===1&&keys==='0'&&flags;
    "#,
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_callback_storage_filter_selection_survives_species_definition_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[{tag:7}];source.constructor={[Symbol.species]:function(){
            return new Proxy({},{get defineProperty(){
                $262.gc();return function(target,key,descriptor){$262.gc();return Reflect.defineProperty(target,key,descriptor);};
            }});
        }};
        var result=source.filter(()=>{delete source[0];$262.gc();return true;});result[0].tag===7;
    "#,
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_callback_storage_species_definition_checks_the_original_owner() {
    for method in [ArrayCallbackMethod::Map, ArrayCallbackMethod::Filter] {
        let source = page();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Bool(true))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            r#"
            var receiver=[1],callback=()=>true;receiver.constructor={[Symbol.species]:function(){
                return new Proxy({},{defineProperty(){return markTerminal();}});
            }};receiver;
        "#,
        );
        let callback = neutral.lookup_global_name("callback").unwrap().unwrap();
        assert_eq!(
            loaned_call(&source, &mut neutral, &receiver, &callback, method),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
        assert!(neutral.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn array_callback_storage_species_definition_throw_does_not_leak_roots() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['map','filter'].every(method=>{
            var source=[1],marker={tag:7};source.constructor={[Symbol.species]:function(){
                return new Proxy({},{defineProperty(){$262.gc();throw marker;}});
            }};
            try{source[method](()=>true);}catch(error){return error===marker;}return false;
        });
    "#,
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_callback_storage_species_getter_keeps_fresh_constructor_values_rooted() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1,2],log='';Object.defineProperty(source,'constructor',{get(){return {
            get [Symbol.species](){log+='s';$262.gc();return function(length){log+=length;$262.gc();return {length};};}
        };}});
        var mapped=source.map(value=>value+1),filtered=source.filter(value=>value===2);
        log==='s2s0'&&mapped.length===2&&mapped[0]===2&&mapped[1]===3&&filtered[0]===2;
    "#,
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_callback_storage_terminal_species_getter_never_enters_the_callback() {
    for method in [ArrayCallbackMethod::Map, ArrayCallbackMethod::Filter] {
        let source = page();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let entered = Rc::new(std::cell::Cell::new(false));
        let output = entered.clone();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Undefined)
            })
            .unwrap();
        neutral
            .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
                output.set(true);
                Ok(crate::HostValue::Bool(true))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            "var receiver=[1],callback=()=>observe();Object.defineProperty(receiver,'constructor',{get(){return markTerminal();}});receiver;",
        );
        let callback = neutral.lookup_global_name("callback").unwrap().unwrap();
        assert_eq!(
            loaned_call(&source, &mut neutral, &receiver, &callback, method),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!entered.get());
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_callback_storage_shared_species_callers_preserve_result_lengths() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1,2],log='';source.constructor={[Symbol.species]:function(length){log+=length;return [];}};
        var flat=source.flat(),flatMapped=source.flatMap(value=>[value]),joined=source.concat([3]),sliced=source.slice(0,1),removed=source.splice(0,1);
        log==='00011'&&flat.join(',')==='1,2'&&flatMapped.join(',')==='1,2'&&joined.join(',')==='1,2,3'&&sliced.join(',')==='1'&&removed.join(',')==='1'&&source.join(',')==='2';
    "#,
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_callback_storage_empty_results_and_callback_validation_order_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[],count=0,callback=()=>{count++;};
        var values=source.forEach(callback)===undefined&&source.map(callback).length===0&&source.filter(callback).length===0&&
            source.every(callback)===true&&source.some(callback)===false&&source.find(callback)===undefined&&source.findIndex(callback)===-1&&
            source.findLast(callback)===undefined&&source.findLastIndex(callback)===-1&&count===0;
        values&&['forEach','map','filter','every','some','find','findIndex','findLast','findLastIndex'].every(method=>{
            var log='',receiver={get length(){log+='l';return 0;}};
            try{Array.prototype[method].call(receiver,0);}catch(error){return error instanceof TypeError&&log==='l';}return false;
        });
    "#,
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_callback_storage_callbacks_finish_tail_calls_and_preserve_error_realms() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function select(value){'use strict';return value===2;}
        function predicate(value){'use strict';return select(value);}
        function finish(value){'use strict';return value+3;}
        function mapper(value){'use strict';return finish(value);}
        function fail(){'use strict';throw 7;}
        function throws(){'use strict';return fail();}
        var caught=false;try{[1].map(throws);}catch(error){caught=error===7;}
        var tailCalls=[1,2].some(predicate)&&[1,2].find(predicate)===2&&[1,2].findLastIndex(predicate)===1&&[1].map(mapper)[0]===4&&caught;
        var methods=['forEach','map','filter','every','some','find','findIndex','findLast','findLastIndex'];
        var child=$262.createRealm(),foreignError=child.global.TypeError;
        var foreignGetter=child.evalScript("Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;");
        var forward=methods.every(method=>{
            try{[1][method](foreignGetter);}catch(error){return error instanceof foreignError&&!(error instanceof TypeError);}return false;
        });
        child.global.parentGetter=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;
        child.global.parentError=TypeError;
        var reverse=child.evalScript("['forEach','map','filter','every','some','find','findIndex','findLast','findLastIndex'].every(method=>{try{[1][method](parentGetter);}catch(error){return error instanceof parentError&&!(error instanceof TypeError);}return false;});");
        tailCalls&&forward&&reverse;
    "#,
        ),
        Value::Bool(true)
    );
    assert!(vm.pending_tail_call.is_none());
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_callback_storage_promise_job_keeps_selected_values_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        r#"
        var result=0;Promise.resolve().then(()=>{
            var source=[{tag:7}];var filtered=source.filter(()=>{delete source[0];$262.gc();return true;});
            result=filtered.map(value=>{$262.gc();return value.tag;})[0];
        });0;
    "#,
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(run(&mut vm, "result;"), Value::Number(7.0));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_callback_storage_manual_timer_and_its_promise_keep_selected_values() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        "delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;",
    );
    vm.install_host_timer_functions().unwrap();
    run(
        &mut vm,
        r#"
        var result=0;setTimeout(()=>{
            var source=[{tag:7}],selected=source.find(()=>{delete source[0];$262.gc();return true;});
            Promise.resolve().then(()=>{result=[selected].map(value=>{$262.gc();return value.tag;})[0];});
        },1);0;
    "#,
    );
    assert_eq!(vm.next_host_timer_due(), Some(1));
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::Number(7.0));
    assert!(vm.realm_call_roots.is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_array_callback_storage_contracts() {
        array_callback_storage_arguments_outlive_the_vm();
        array_callback_storage_neutral_early_return_checks_original_owner();
        array_callback_storage_find_keeps_original_object_across_delete_and_gc();
        array_callback_storage_holes_and_directions_preserve_callback_order();
        array_callback_storage_filter_retains_original_object_across_delete_and_gc();
        array_callback_storage_argument_moves_conserve_actual_credit();
        array_callback_storage_neutral_builder_keeps_the_input_owner();
        array_callback_storage_root_loan_does_not_install_document_policy();
        array_callback_storage_all_methods_check_a_terminal_callback_owner();
        array_callback_storage_terminal_length_getter_never_enters_the_callback();
        array_callback_storage_terminal_index_getter_never_enters_the_callback();
        array_callback_storage_callback_throw_restores_the_outer_roots_and_stack();
        array_callback_storage_length_conversion_roots_callback_and_this_arg();
        array_callback_storage_public_live_mutations_preserve_captured_length();
        array_callback_storage_public_proxy_has_and_get_order_is_preserved();
        array_callback_storage_map_result_survives_species_definition_gc();
        array_callback_storage_filter_selection_survives_species_definition_gc();
        array_callback_storage_species_definition_checks_the_original_owner();
        array_callback_storage_species_definition_throw_does_not_leak_roots();
        array_callback_storage_species_getter_keeps_fresh_constructor_values_rooted();
        array_callback_storage_terminal_species_getter_never_enters_the_callback();
        array_callback_storage_shared_species_callers_preserve_result_lengths();
        array_callback_storage_empty_results_and_callback_validation_order_are_preserved();
        array_callback_storage_callbacks_finish_tail_calls_and_preserve_error_realms();
        array_callback_storage_promise_job_keeps_selected_values_through_gc();
        array_callback_storage_manual_timer_and_its_promise_keep_selected_values();
    }
}
