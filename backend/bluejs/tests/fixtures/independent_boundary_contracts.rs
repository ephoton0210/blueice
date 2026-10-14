// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Embedding, byte codec and calendar ingress through genuine producers.
use super::*;
use crate::{compile, parse};

type Factory = fn(&[HostValue]) -> Result<Option<HostObjectKey>, HostFunctionError>;
type Method = fn(HostObjectKey, &[HostValue]) -> Result<HostValue, HostFunctionError>;
type Pair = fn(HostObjectKey, HostObjectKey) -> Result<(), HostFunctionError>;

fn factory(args: &[HostValue]) -> Result<Option<HostObjectKey>, HostFunctionError> {
    match args {
        [HostValue::Number(n)] if *n < 0.0 => Err(HostFunctionError::new("host refusal")),
        [HostValue::Number(n)] => Ok(Some(HostObjectKey::new(7, 3, *n as u64))),
        [] => Ok(Some(HostObjectKey::new(7, 3, 1))),
        _ => Ok(None),
    }
}
fn method(key: HostObjectKey, args: &[HostValue]) -> Result<HostValue, HostFunctionError> {
    match args {
        [HostValue::Bool(false)] => Err(HostFunctionError::new("host refusal")),
        [HostValue::Number(value)] if *value == 99.0 => Ok(HostValue::String("long".into())),
        [HostValue::String(text)] => Ok(HostValue::String(text.clone())),
        _ => Ok(HostValue::Number(key.object() as f64)),
    }
}
fn pair(parent: HostObjectKey, child: HostObjectKey) -> Result<(), HostFunctionError> {
    if parent.object() == child.object() {
        Err(HostFunctionError::new("same-key child refused"))
    } else {
        Ok(())
    }
}
fn code(source: &str) -> Bytecode {
    compile(&parse(source).unwrap()).unwrap()
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&code(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}
fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}
fn configured(nursery_capacity: usize) -> (Vm, HostObjectFamily, Value) {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("findNode", 1, family, factory as Factory)
        .unwrap();
    vm.install_host_object_method(family, "read", 1, method as Method)
        .unwrap();
    vm.install_host_object_pair_method(family, "pair", 1, pair as Pair)
        .unwrap();
    vm.install_host_object_accessor(family, "attribute", method as Method, method as Method)
        .unwrap();
    vm.install_host_click_event_methods(family).unwrap();
    let wrapper = execute(&mut vm, "globalThis.node = findNode(1); node");
    (vm, family, wrapper)
}

#[cfg_attr(test, test)]
fn host_registration_validation_and_minted_native_ingress_are_exact() {
    for nursery in [256, 1] {
        let (mut vm, family, wrapper) = configured(nursery);
        let key = HostObjectKey::new(7, 3, 1);
        assert!(key.matches_owner(7, 3));
        assert!(!key.matches_owner(8, 3));
        assert!(!key.matches_owner(7, 4));
        assert_eq!(key.object(), 1);
        for name in ["bad name", "read"] {
            assert!(matches!(
                vm.install_host_object_method(family, name, 0, method as Method),
                Err(RuntimeError::TypeError(_))
            ));
        }
        for name in ["bad name", "pair"] {
            assert!(matches!(
                vm.install_host_object_pair_method(family, name, 1, pair as Pair),
                Err(RuntimeError::TypeError(_))
            ));
        }
        for name in ["bad name", "attribute"] {
            assert!(matches!(
                vm.install_host_object_accessor(family, name, method as Method, method as Method),
                Err(RuntimeError::TypeError(_))
            ));
        }
        assert!(matches!(
            vm.install_host_click_event_methods(family),
            Err(RuntimeError::TypeError(_))
        ));
        let partial = vm.create_host_object_family().unwrap();
        vm.install_host_object_method(partial, "removeEventListener", 0, method as Method)
            .unwrap();
        assert!(matches!(
            vm.install_host_click_event_methods(partial),
            Err(RuntimeError::TypeError(_))
        ));
        for name in ["bad name", "findNode"] {
            assert!(matches!(
                vm.install_host_object_factory(name, 1, family, factory as Factory),
                Err(RuntimeError::TypeError(_))
            ));
        }
        let mut other = Vm::default();
        assert!(matches!(
            other.install_host_object_method(family, "read", 1, method as Method),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            other.install_host_object_pair_method(family, "pair", 1, pair as Pair),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            other.install_host_object_accessor(
                family,
                "attribute",
                method as Method,
                method as Method
            ),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            other.install_host_click_event_methods(family),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            other.install_host_object_factory("foreign", 1, family, factory as Factory),
            Err(RuntimeError::TypeError(_))
        ));
        let owner = vm.install_host_object("document").unwrap();
        assert!(matches!(
            other.install_host_object_factory_method(
                owner,
                "foreign",
                1,
                family,
                factory as Factory
            ),
            Err(RuntimeError::TypeError(_))
        ));
        vm.install_host_object_factory_method(owner, "find", 1, family, factory as Factory)
            .unwrap();
        for source in [
            "findNode(-1)",
            "node.pair(1)",
            "node.read(false)",
            "new findNode()",
            "new node.read()",
            "document.find.call({})",
        ] {
            assert!(
                matches!(
                    vm.execute_script(&code(source)),
                    Err(RuntimeError::TypeError(_))
                ),
                "{source}"
            );
        }
        for (source, expected) in [
            ("findNode('missing')", Value::Null),
            ("findNode(1) === node", Value::Bool(true)),
            ("node.pair(findNode(2)) === findNode(2)", Value::Bool(true)),
        ] {
            assert_eq!(execute(&mut vm, source), expected);
        }
        // Read actual installed native tags rather than inventing registrations.
        for callee in ["findNode", "node.read", "node.pair"] {
            let function = execute(&mut vm, callee).object_id().unwrap();
            let tag = vm.heap.native_function(function).unwrap().unwrap();
            assert!(matches!(
                vm.native_call(tag, wrapper.clone(), vec![wrapper.clone()], true),
                Err(RuntimeError::TypeError(_))
            ));
        }
        // Explicit internal selector ingress preserves defensive registration errors.
        assert!(matches!(
            vm.host_object_factory_call(u32::MAX, Value::Undefined, &[], false),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            vm.host_object_method_call(u32::MAX, wrapper.clone(), &[], false),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            vm.host_object_pair_method_call(
                u32::MAX,
                wrapper.clone(),
                std::slice::from_ref(&wrapper),
                false
            ),
            Err(RuntimeError::TypeError(_))
        ));
        let callback = execute(&mut vm, "(() => {})");
        assert!(matches!(
            vm.host_click_listener_call(
                u32::MAX,
                wrapper.clone(),
                &[Value::String("click".into()), callback],
                false,
                true
            ),
            Err(RuntimeError::TypeError(_))
        ));
        assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn host_cold_installers_and_dispatch_refusals_restore_publication_and_allow_retry() {
    for nursery in [256, 1] {
        for operation in 0..8 {
            let mut extra = 0;
            let mut refusals = 0;
            loop {
                assert!(
                    extra <= 16 * 1024 * 1024,
                    "operation {operation} exceeds bounded headroom"
                );
                let (mut vm, family) = if operation >= 6 {
                    let (mut vm, family, _) = configured(nursery);
                    execute(&mut vm, "node.addEventListener('click', () => {});");
                    (vm, Some(family))
                } else {
                    let mut vm = Vm::new(VmConfig {
                        heap: HeapConfig {
                            nursery_capacity: nursery,
                            ..HeapConfig::default()
                        },
                        ..VmConfig::default()
                    })
                    .unwrap();
                    let family = if operation == 0 {
                        None
                    } else {
                        Some(vm.create_host_object_family().unwrap())
                    };
                    if operation == 5 {
                        vm.global("globalThis").unwrap();
                    }
                    (vm, family)
                };
                vm.stack.push(Value::Number(17.0));
                let before = vm.stack.clone();
                collect(&mut vm);
                vm.remaining_instructions = vm.config.instruction_budget;
                vm.heap.allow_only(extra);
                let result = match operation {
                    0 => vm.create_host_object_family().map(|_| ()),
                    1 => vm.install_host_object_method(
                        family.unwrap(),
                        "newMethod",
                        0,
                        method as Method,
                    ),
                    2 => vm.install_host_object_pair_method(
                        family.unwrap(),
                        "newPair",
                        1,
                        pair as Pair,
                    ),
                    3 => vm.install_host_object_accessor(
                        family.unwrap(),
                        "newAccessor",
                        method as Method,
                        method as Method,
                    ),
                    4 => vm.install_host_click_event_methods(family.unwrap()),
                    5 => vm.install_host_object_factory(
                        "newFactory",
                        0,
                        family.unwrap(),
                        factory as Factory,
                    ),
                    6 => vm
                        .dispatch_host_click(family.unwrap(), HostObjectKey::new(7, 3, 1))
                        .map(|_| ()),
                    7 => vm
                        .host_object_factory_call(0, Value::Undefined, &[Value::Number(2.0)], false)
                        .map(|_| ()),
                    _ => unreachable!(),
                };
                assert_eq!(vm.stack, before, "operation {operation}, {extra}");
                let completed = result.is_ok();
                match result {
                    Ok(()) => (),
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {
                        refusals += 1;
                        extra = vm.heap.next_allocation_headroom(extra);
                    }
                    Err(error) => panic!("operation {operation}, {extra}: {error:?}"),
                }
                vm.heap.allow_only(16 * 1024 * 1024);
                if operation == 4 && !completed {
                    vm.install_host_click_event_methods(family.unwrap())
                        .unwrap();
                }
                vm.stack.clear();
                assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
                if completed {
                    assert!(refusals > 0, "operation {operation}");
                    break;
                }
            }
        }
        let mut cold = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: nursery,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        let family = cold.create_host_object_family().unwrap();
        collect(&mut cold);
        cold.heap.allow_only(0);
        assert!(matches!(
            cold.install_host_object_factory("newFactory", 0, family, factory as Factory),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
        ));
        cold.heap.allow_only(16 * 1024 * 1024);
        cold.install_host_object_factory("newFactory", 0, family, factory as Factory)
            .unwrap();
        assert_eq!(
            execute(&mut cold, "newFactory() === newFactory()"),
            Value::Bool(true)
        );
    }
}

#[cfg_attr(test, test)]
fn host_listener_mutation_fatal_refusal_and_exact_event_receivers_are_preserved() {
    for nursery in [256, 1] {
        let (mut vm, family, wrapper) = configured(nursery);
        execute(&mut vm,"globalThis.order=''; globalThis.later=()=>{order+='b'}; node.addEventListener('click', e=>{order+='a';node.removeEventListener('click',later);try {e.preventDefault(1)} catch (_) {} try {e.preventDefault.call({})} catch (_) {} e.preventDefault();});node.addEventListener('click',later);");
        assert_eq!(
            vm.dispatch_host_click(family, HostObjectKey::new(7, 3, 1)),
            Ok(true)
        );
        assert_eq!(execute(&mut vm, "order"), Value::String("a".into()));
        assert_eq!(
            execute(&mut vm, "node.removeEventListener('click',later)"),
            Value::Undefined
        );
        let mut owner = Vm::default();
        let foreign = owner.heap.alloc_object(None).unwrap();
        let root = owner.heap.root(foreign).unwrap();
        let add = execute(&mut vm, "node.addEventListener")
            .object_id()
            .unwrap();
        let tag = vm.heap.native_function(add).unwrap().unwrap();
        assert_eq!(
            vm.native_call(
                tag,
                wrapper,
                vec![Value::String("click".into()), Value::Object(foreign)],
                false
            ),
            Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
        );
        owner.heap.unroot(root).unwrap();
        assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
        let (mut exhausted, family, _) = configured(nursery);
        execute(
            &mut exhausted,
            "node.addEventListener('click',()=>{while(true){}});",
        );
        exhausted.config.instruction_budget = 100;
        assert_eq!(
            exhausted.dispatch_host_click(family, HostObjectKey::new(7, 3, 1)),
            Err(RuntimeError::InstructionLimit)
        );
        exhausted.config.instruction_budget = VmConfig::default().instruction_budget;
        assert_eq!(execute(&mut exhausted, "21+21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn host_root_refusals_leave_families_wrappers_and_listener_snapshots_unpublished() {
    let mut vm = Vm::default();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.create_host_object_family(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.host_object_families.is_empty());
    assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
    for available in [0, 1, 2] {
        let (mut vm, family, wrapper) = configured(1);
        let callbacks=execute(&mut vm,"globalThis.first=()=>{};globalThis.second=()=>{};node.addEventListener('click',first);node.addEventListener('click',second);[first,second]").object_id().unwrap();
        let first = vm.heap.get_own(callbacks, "0").unwrap().unwrap();
        let second = vm.heap.get_own(callbacks, "1").unwrap().unwrap();
        let callback_ids = [first.object_id().unwrap(), second.object_id().unwrap()];
        let add = execute(&mut vm, "node.addEventListener")
            .object_id()
            .unwrap();
        let NativeFunction::HostClickListenerAdd(index) =
            vm.heap.native_function(add).unwrap().unwrap()
        else {
            panic!("actual listener tag")
        };
        vm.heap.allow_root_registrations(available);
        assert_eq!(
            vm.dispatch_host_click(family, HostObjectKey::new(7, 3, 1)),
            Err(RuntimeError::Heap(HeapError::IdExhausted))
        );
        for callback in [first, second] {
            vm.host_click_listener_call(
                index,
                wrapper.clone(),
                &[Value::String("click".into()), callback],
                false,
                false,
            )
            .unwrap();
        }
        execute(&mut vm, "first=undefined;second=undefined;");
        collect(&mut vm);
        assert!(
            callback_ids.iter().all(|id| !vm.heap.contains(*id)),
            "snapshot root rollback {available}"
        );
        assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
    }
    let (mut vm, _, wrapper) = configured(1);
    let add = execute(&mut vm, "node.addEventListener")
        .object_id()
        .unwrap();
    let NativeFunction::HostClickListenerAdd(index) =
        vm.heap.native_function(add).unwrap().unwrap()
    else {
        panic!("actual listener tag")
    };
    let callback = execute(&mut vm, "()=>{}");
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.host_click_listener_call(
            index,
            wrapper,
            &[Value::String("click".into()), callback],
            false,
            true
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.host_click_listeners.is_empty());
    assert_eq!(
        vm.host_object_factory_call(0, Value::Undefined, &[Value::Number(2.0)], false),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
}

#[cfg_attr(test, test)]
fn host_pause_listener_quota_and_callback_string_limit_remain_real_boundaries() {
    let (mut vm, family, wrapper) = configured(1);
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            crate::BlueJsSourceIdentity::new("page:///host-pause.js", "sha256:host-pause").unwrap(),
            &crate::BlueJsProgramV1::Script(parse("21+21").unwrap()),
        )
        .unwrap();
    let program = registry.get(handle).unwrap().bytecode().clone();
    vm.execute_script_until_debugger_pause(&program, 0).unwrap();
    assert!(matches!(
        vm.dispatch_host_click(family, HostObjectKey::new(7, 3, 1)),
        Err(RuntimeError::Unsupported(_))
    ));
    vm.resume_debugger_execution().unwrap();
    let native = execute(&mut vm, "node.read").object_id().unwrap();
    let NativeFunction::HostObjectMethod(index) = vm.heap.native_function(native).unwrap().unwrap()
    else {
        panic!("actual method tag")
    };
    vm.config.max_string_bytes = 3;
    assert_eq!(
        vm.host_object_method_call(index, wrapper.clone(), &[Value::Number(99.0)], false),
        Err(RuntimeError::StringLimit { limit: 3 })
    );
    vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
    let add = execute(&mut vm, "node.addEventListener")
        .object_id()
        .unwrap();
    let NativeFunction::HostClickListenerAdd(index) =
        vm.heap.native_function(add).unwrap().unwrap()
    else {
        panic!("actual listener tag")
    };
    let program = code("()=>{}");
    let mut first = None;
    for _ in 0..host_objects::MAX_HOST_CLICK_LISTENERS_PER_FAMILY {
        let callback = vm.execute_script(&program).unwrap();
        vm.host_click_listener_call(
            index,
            wrapper.clone(),
            &[Value::String("click".into()), callback.clone()],
            false,
            true,
        )
        .unwrap();
        first.get_or_insert(callback);
    }
    let next = vm.execute_script(&program).unwrap();
    assert!(matches!(
        vm.host_click_listener_call(
            index,
            wrapper.clone(),
            &[Value::String("click".into()), next.clone()],
            false,
            true
        ),
        Err(RuntimeError::RangeError(_))
    ));
    vm.host_click_listener_call(
        index,
        wrapper.clone(),
        &[Value::String("click".into()), first.unwrap()],
        false,
        false,
    )
    .unwrap();
    vm.host_click_listener_call(
        index,
        wrapper,
        &[Value::String("click".into()), next],
        false,
        true,
    )
    .unwrap();
    assert_eq!(
        vm.host_click_listeners.len(),
        host_objects::MAX_HOST_CLICK_LISTENERS_PER_FAMILY
    );
    assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn uint8_codecs_preserve_cold_nursery_roots_constructor_guards_and_foreign_ingress() {
    for nursery in [256, 1] {
        for (tag, input) in [
            (NativeFunction::Uint8ArrayFromHex, "00ff"),
            (NativeFunction::Uint8ArrayFromBase64, "AP8="),
        ] {
            let mut extra = 0;
            let mut refusals = 0;
            loop {
                assert!(extra <= 16 * 1024 * 1024);
                let mut vm = Vm::new(VmConfig {
                    heap: HeapConfig {
                        nursery_capacity: nursery,
                        ..HeapConfig::default()
                    },
                    ..VmConfig::default()
                })
                .unwrap();
                vm.remaining_instructions = vm.config.instruction_budget;
                vm.stack.push(Value::Number(17.0));
                collect(&mut vm);
                vm.heap.allow_only(extra);
                let result = vm.native_call(
                    tag,
                    Value::Undefined,
                    vec![Value::String(input.into())],
                    false,
                );
                assert_eq!(vm.stack, vec![Value::Number(17.0)]);
                let completed = match result {
                    Ok(value) => {
                        let id = value.object_id().unwrap();
                        let root = vm.heap.root(id).unwrap();
                        assert_eq!(
                            vm.heap.typed_array_index_value(id, 0).unwrap(),
                            Some(Value::Number(0.0))
                        );
                        assert_eq!(
                            vm.heap.typed_array_index_value(id, 1).unwrap(),
                            Some(Value::Number(255.0))
                        );
                        vm.heap.unroot(root).unwrap();
                        true
                    }
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {
                        refusals += 1;
                        extra = vm.heap.next_allocation_headroom(extra);
                        false
                    }
                    Err(error) => panic!("cold codec {tag:?} at {extra}: {error:?}"),
                };
                vm.heap.allow_only(16 * 1024 * 1024);
                vm.stack.clear();
                assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
                if completed {
                    assert!(refusals > 0);
                    break;
                }
            }
        }
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: nursery,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        let receiver = execute(&mut vm, "globalThis.bytes=new Uint8Array([0,255]);bytes");
        for callee in [
            "Uint8Array.fromHex",
            "Uint8Array.fromBase64",
            "bytes.toHex",
            "bytes.toBase64",
            "bytes.setFromHex",
            "bytes.setFromBase64",
        ] {
            let id = execute(&mut vm, callee).object_id().unwrap();
            let tag = vm.heap.native_function(id).unwrap().unwrap();
            assert!(matches!(
                vm.native_call(tag, receiver.clone(), vec![], true),
                Err(RuntimeError::TypeError(_))
            ));
        }
        let mut other = Vm::default();
        let foreign = other.heap.alloc_object(None).unwrap();
        let root = other.heap.root(foreign).unwrap();
        let id = execute(&mut vm, "bytes.toHex").object_id().unwrap();
        let tag = vm.heap.native_function(id).unwrap().unwrap();
        assert_eq!(
            vm.native_call(tag, Value::Object(foreign), vec![], false),
            Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
        );
        other.heap.unroot(root).unwrap();
        assert_eq!(
            execute(&mut vm, "bytes.toBase64({omitPadding:{}})"),
            Value::String("AP8".into())
        );
        vm.config.max_string_bytes = 3;
        assert_eq!(
            vm.call_native(Value::Object(id), receiver, vec![], false),
            Err(RuntimeError::StringLimit { limit: 3 })
        );
        vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
        assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn calendar_identifier_preserves_foreign_heap_refusal_and_own_slot_fast_path() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign).unwrap();
    let mut vm = Vm::default();
    assert_eq!(
        vm.temporal_calendar_identifier(&Value::Object(foreign)),
        Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
    );
    let calendar=execute(&mut vm,"globalThis.calendarReads=0;let date=new Temporal.PlainDate(2024,10,8);Object.defineProperty(date,'calendar',{get(){calendarReads++;throw 17}});Object.defineProperty(date,'calendarId',{get(){calendarReads++;throw 19}});globalThis.calendarDate=date;date");
    assert_eq!(
        vm.temporal_calendar_identifier(&calendar),
        Ok("iso8601".into())
    );
    assert_eq!(execute(&mut vm, "calendarReads"), Value::Number(0.0));
    assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
    owner.heap.unroot(root).unwrap();
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_independent_boundary_contracts() {
        host_registration_validation_and_minted_native_ingress_are_exact();
        host_cold_installers_and_dispatch_refusals_restore_publication_and_allow_retry();
        host_listener_mutation_fatal_refusal_and_exact_event_receivers_are_preserved();
        host_root_refusals_leave_families_wrappers_and_listener_snapshots_unpublished();
        host_pause_listener_quota_and_callback_string_limit_remain_real_boundaries();
        uint8_codecs_preserve_cold_nursery_roots_constructor_guards_and_foreign_ingress();
        calendar_identifier_preserves_foreign_heap_refusal_and_own_slot_fast_path();
    }
}
