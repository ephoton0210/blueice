// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor brands and prototype changes at the VM/heap boundary.

use super::*;
use crate::{compile, parse};

#[cfg_attr(test, test)]
fn function_realm_validation_rejects_primitives_and_ordinary_objects() {
    let mut vm = Vm::default();
    let object = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(object).unwrap();
    for value in [
        Value::Undefined,
        Value::Null,
        Value::Bool(true),
        Value::Number(42.0),
        Value::String("Function".into()),
        Value::BigInt(42.into()),
        Value::Symbol(JsSymbol::well_known("iterator")),
        Value::Object(object),
    ] {
        assert_eq!(
            vm.validate_function_realm(value),
            Err(RuntimeError::TypeError(
                "constructor must be callable".into()
            ))
        );
    }
    for construct in [false, true] {
        assert_eq!(
            vm.proxy_call(object, Value::Undefined, Vec::new(), construct),
            Err(RuntimeError::TypeError(
                "Proxy target is unavailable".into()
            ))
        );
    }
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn object_operations_reject_a_handle_owned_by_a_different_heap() {
    let mut owner = Vm::default();
    let object = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(object).unwrap();
    let mut vm = Vm::default();
    let key = PropertyName::from("answer");
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(object)));
    for result in [
        vm.object_get_own_property(object, &key).map(|_| ()),
        vm.object_define_own_property(
            object,
            key.clone(),
            crate::property::PropertyDescriptor::default(),
        )
        .map(|_| ()),
        vm.object_delete(object, &key).map(|_| ()),
        vm.object_own_property_keys(object).map(|_| ()),
        vm.object_is_extensible(object).map(|_| ()),
        vm.object_get_prototype(object).map(|_| ()),
        vm.object_set_prototype(object, None).map(|_| ()),
        vm.object_prevent_extensions(object).map(|_| ()),
        vm.validate_function_realm(Value::Object(object))
            .map(|_| ()),
        vm.is_constructor(&Value::Object(object)).map(|_| ()),
        vm.proxy_call(object, Value::Undefined, Vec::new(), false)
            .map(|_| ()),
        vm.ordinary_set_with_receiver(object, &Value::Object(object), &key, &Value::Number(42.0))
            .map(|_| ()),
        vm.get_from_prototype(object, &Value::Object(object), &key)
            .map(|_| ()),
        vm.proxy_get(object, &Value::Object(object), &key)
            .map(|_| ()),
        vm.proxy_has(object, &key).map(|_| ()),
        vm.proxy_set(object, &Value::Object(object), &key, &Value::Number(42.0))
            .map(|_| ()),
        vm.proxy_delete(object, &key).map(|_| ()),
        vm.proxy_own_keys(object).map(|_| ()),
        vm.proxy_get_own_property(object, &key).map(|_| ()),
        vm.proxy_define_own_property(object, key.clone(), PropertyDescriptor::default())
            .map(|_| ()),
        vm.proxy_is_extensible(object).map(|_| ()),
        vm.proxy_get_prototype(object).map(|_| ()),
        vm.proxy_set_prototype(object, None).map(|_| ()),
        vm.proxy_prevent_extensions(object).map(|_| ()),
        vm.typed_array_define_own_property(
            object,
            TypedArrayNumericKey::Index(0),
            PropertyDescriptor::default(),
        )
        .map(|_| ()),
    ] {
        assert_eq!(result, expected);
    }
    assert!(owner.heap.contains(object));
    assert!(vm.stack.is_empty());
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn prototype_changes_preserve_the_target_on_foreign_handle_or_budget_failure() {
    let mut other = Vm::default();
    let foreign = other.heap.alloc_object(None).unwrap();
    let foreign_root = other.heap.root(foreign).unwrap();
    for membrane in [false, true] {
        let mut vm = Vm::default();
        if membrane {
            vm.install_test262_harness().unwrap();
            vm.execute_script(&compile(&parse("$262.createRealm().global").unwrap()).unwrap())
                .unwrap();
        }
        let target = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(target).unwrap();
        assert_eq!(
            vm.object_set_prototype(target, Some(foreign)),
            Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
        );
        assert_eq!(vm.heap.prototype(target).unwrap(), None);
        assert_eq!(
            vm.proxy_set_prototype(target, Some(foreign)),
            Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)))
        );
        assert_eq!(vm.proxy_set_prototype(target, None), Ok(true));
        assert_eq!(vm.proxy_delete(target, &"absent".into()), Ok(true));
        if membrane {
            let candidate = vm.heap.alloc_object(None).unwrap();
            let candidate_root = vm.heap.root(candidate).unwrap();
            vm.remaining_instructions = 0;
            assert_eq!(
                vm.object_set_prototype(target, Some(candidate)),
                Err(RuntimeError::InstructionLimit)
            );
            assert_eq!(vm.heap.prototype(target).unwrap(), None);
            vm.heap.unroot(candidate_root).unwrap();
            let proxy = vm
                .execute_script(&compile(&parse("new Proxy({}, {})").unwrap()).unwrap())
                .unwrap()
                .object_id()
                .unwrap();
            assert_eq!(vm.object_set_prototype(target, Some(proxy)), Ok(true));
            assert_eq!(vm.heap.prototype(target).unwrap(), Some(proxy));
        }
        assert_eq!(vm.proxy_prevent_extensions(target), Ok(true));
        vm.heap.unroot(root).unwrap();
    }
    other.heap.unroot(foreign_root).unwrap();
}

#[cfg_attr(test, test)]
fn heap_accessor_records_require_a_callable_setter_at_vm_dispatch() {
    let mut vm = Vm::default();
    let target = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(target).unwrap();
    let key = PropertyName::from("answer");
    // The heap accepts Value-bearing accessor records; executable
    // callability is a VM contract checked when the accessor is invoked.
    assert!(vm
        .heap
        .define_own_property(
            target,
            key.clone(),
            PropertyDescriptor {
                set: Some(Value::Number(7.0)),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            }
        )
        .unwrap());
    assert_eq!(
        vm.ordinary_set_with_receiver(target, &Value::Object(target), &key, &Value::Number(42.0)),
        Err(RuntimeError::TypeError(
            "property setter is not callable".into()
        ))
    );
    assert_eq!(
        vm.heap
            .get_own_property_descriptor(target, &key)
            .unwrap()
            .unwrap()
            .set,
        Some(Value::Number(7.0))
    );
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn accessor_calls_preserve_uncatchable_instruction_refusal_and_vm_reusability() {
    let mut vm = Vm::default();
    let receiver = vm
        .execute_script(
            &compile(&parse("({get answer() {return 42;}, set answer(value) {}})").unwrap())
                .unwrap(),
        )
        .unwrap();
    let object = receiver.object_id().unwrap();
    let root = vm.heap.root(object).unwrap();
    let key = PropertyName::from("answer");
    vm.remaining_instructions = 0;
    assert_eq!(
        vm.get_from_prototype(object, &receiver, &key),
        Err(RuntimeError::InstructionLimit)
    );
    vm.remaining_instructions = 0;
    assert_eq!(
        vm.ordinary_set_with_receiver(object, &receiver, &key, &Value::Number(7.0)),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(vm.stack.is_empty());
    assert_eq!(vm.call_depth, 0);
    vm.remaining_instructions = vm.config.instruction_budget;
    assert_eq!(
        vm.get_from_prototype(object, &receiver, &key),
        Ok(Value::Number(42.0))
    );
    assert_eq!(
        vm.ordinary_set_with_receiver(object, &receiver, &key, &Value::Number(7.0)),
        Ok(true)
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn object_callbacks_materialize_errors_only_while_another_realm_is_active() {
    use crate::vm::realm_reentrancy::{has_foreign_caller, register_active};

    let mut vm = Vm::default();
    let getter = vm
        .execute_script(
            &compile(
                &parse("Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let root = vm.heap.root(getter.object_id().unwrap()).unwrap();
    let tag = vm.object_prototype.heap;
    assert!(!has_foreign_caller(tag));
    let own_guard = register_active(&mut vm);
    assert!(!has_foreign_caller(tag));
    assert!(matches!(
        vm.call_object_callback(getter.clone(), Value::Undefined, Vec::new()),
        Err(RuntimeError::TypeError(_))
    ));
    let mut caller = Vm::default();
    let caller_guard = register_active(&mut caller);
    let nested_guard = register_active(&mut vm);
    assert!(has_foreign_caller(tag));
    vm.remaining_instructions = 0;
    assert_eq!(
        vm.call_object_callback(getter.clone(), Value::Undefined, Vec::new()),
        Err(RuntimeError::InstructionLimit)
    );
    vm.remaining_instructions = vm.config.instruction_budget;
    let Err(RuntimeError::Thrown(error)) =
        vm.call_object_callback(getter.clone(), Value::Undefined, Vec::new())
    else {
        panic!("a cross-Realm native call must retain the callee's error object");
    };
    assert_eq!(error.object_id().unwrap().heap, tag);
    assert_eq!(
        vm.get_property(&error, &"name".into()),
        Ok(Value::String("TypeError".into()))
    );
    drop(nested_guard);
    drop(caller_guard);
    assert!(!has_foreign_caller(tag));
    drop(own_guard);
    assert!(!has_foreign_caller(tag));
    assert!(matches!(
        vm.call_object_callback(getter, Value::Undefined, Vec::new()),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(vm.stack.is_empty());
    assert_eq!(vm.call_depth, 0);
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn reflective_lazy_initialization_keeps_its_receiver_alive_and_restores_roots() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    // No permanent root is needed: the object-operation boundary owns the
    // receiver while initializing the Function constructor for this key.
    assert!(matches!(
        vm.object_get_own_property(object, &"constructor".into()),
        Ok(None)
    ));
    assert!(vm.heap.contains(object));
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    assert!(!vm.heap.contains(object));
}

impl Vm {
    /// Runs constructor and heap ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_object_boundary_contracts() {
        lazy_deletion_and_foreign_prototype_errors_preserve_objects();
        lazy_object_property_and_intrinsic_refusals_preserve_resource_errors();
        foreign_prototype_fallback_uses_cached_intrinsics_without_unrelated_allocation();
        function_realm_validation_rejects_primitives_and_ordinary_objects();
        object_operations_reject_a_handle_owned_by_a_different_heap();
        prototype_changes_preserve_the_target_on_foreign_handle_or_budget_failure();
        heap_accessor_records_require_a_callable_setter_at_vm_dispatch();
        accessor_calls_preserve_uncatchable_instruction_refusal_and_vm_reusability();
        object_callbacks_materialize_errors_only_while_another_realm_is_active();
        reflective_lazy_initialization_keeps_its_receiver_alive_and_restores_roots();
    }
}

#[cfg_attr(test, test)]
fn lazy_object_property_and_intrinsic_refusals_preserve_resource_errors() {
    type ObjectBuilder = fn(&mut Vm) -> Result<ObjectId, RuntimeError>;
    let builders: &[(&str, ObjectBuilder)] = &[
        ("cold ordinary constructor descriptor", |vm| {
            let object = vm.object_prototype;
            vm.object_get_own_property(object, &"constructor".into())?;
            Ok(object)
        }),
        ("cold revocable Proxy", |vm| {
            let object = Value::Object(vm.object_prototype);
            vm.proxy_revocable(&[object.clone(), object])
                .map(|value| value.object_id().unwrap())
        }),
        ("Object prototype descriptor", |vm| {
            let object = vm.object_prototype;
            vm.object_get_own_property(object, &"hasOwnProperty".into())?;
            Ok(object)
        }),
        ("Object prototype definition", |vm| {
            let object = vm.object_prototype;
            vm.object_define_own_property(
                object,
                "hasOwnProperty".into(),
                PropertyDescriptor::data(Value::Number(42.0), true, true, true),
            )?;
            Ok(object)
        }),
        ("Object prototype deletion", |vm| {
            let object = vm.object_prototype;
            vm.object_delete(object, &"hasOwnProperty".into())?;
            Ok(object)
        }),
        ("Function prototype constructor descriptor", |vm| {
            let object = vm.function_prototype()?;
            vm.object_get_own_property(object, &"constructor".into())?;
            Ok(object)
        }),
        ("descriptor record data growth", |vm| {
            vm.descriptor_object(&PropertyDescriptor::data(
                Value::String("x".repeat(4096).into()),
                true,
                true,
                true,
            ))
            .map(|value| value.object_id().unwrap())
        }),
        ("AsyncFunction intrinsic", |vm| {
            vm.intrinsic_prototype("AsyncFunction")
                .map(|value| value.object_id().unwrap())
        }),
        ("GeneratorFunction intrinsic", |vm| {
            vm.intrinsic_prototype("GeneratorFunction")
                .map(|value| value.object_id().unwrap())
        }),
        ("AsyncGeneratorFunction intrinsic", |vm| {
            vm.intrinsic_prototype("AsyncGeneratorFunction")
                .map(|value| value.object_id().unwrap())
        }),
        ("Intl intrinsic", |vm| {
            vm.intrinsic_prototype("Intl.NumberFormat")
                .map(|value| value.object_id().unwrap())
        }),
        ("Temporal intrinsic", |vm| {
            vm.intrinsic_prototype("Temporal.PlainDate")
                .map(|value| value.object_id().unwrap())
        }),
    ];
    for &(name, builder) in builders {
        Vm::verify_intrinsic_allocation_boundary(name, builder);
    }
}

#[cfg_attr(test, test)]
fn foreign_prototype_fallback_uses_cached_intrinsics_without_unrelated_allocation() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let target = vm.execute_script(&compile(&parse(
        "globalThis.other = $262.createRealm().global; globalThis.Target = other.eval('(function Target() {})'); Target.prototype = 7; Target"
    ).unwrap()).unwrap()).unwrap();
    let expected = vm
        .execute_script(&compile(&parse("other.Object.prototype").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap();
    let root = vm.heap.root(expected).unwrap();
    let unknown = vm.heap.alloc_object(None).unwrap();
    let unknown_root = vm.heap.root(unknown).unwrap();
    let globals = vm.globals.clone();
    vm.new_target = target;
    vm.heap.allow_only(0);
    assert_eq!(
        vm.constructor_prototype_for(vm.object_prototype, None),
        Ok(expected)
    );
    assert_eq!(vm.constructor_prototype_for(unknown, None), Ok(unknown));
    assert_eq!(vm.globals, globals);
    assert!(vm.stack.is_empty());
    vm.new_target = Value::Undefined;
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(unknown_root).unwrap();
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn lazy_deletion_and_foreign_prototype_errors_preserve_objects() {
    for source in [
        "var child = $262.createRealm().global; var prototype = child.eval('globalThis.observed = 0; new Proxy({}, {getOwnPropertyDescriptor() {observed++; throw 7;}})'); var receiver = Object.create(prototype); receiver.answer = 42; receiver.answer === 42 && child.observed === 0 ? 42 : 0;",
        "var child = $262.createRealm().global; var prototype = child.eval('globalThis.observed = 0; new Proxy({}, {getPrototypeOf() {observed++; throw 7;}})'); var receiver = Object.create(prototype); receiver.answer = 42; receiver.answer === 42 && child.observed === 0 ? 42 : 0;",
        "var child = $262.createRealm().global; var prototype = child.eval('new Proxy({}, {set() {throw 7;}})'); var receiver = Object.create(prototype); try {receiver.answer = 42; 0;} catch (error) {error === 7 ? 42 : 0;}",
        "var target = {answer:42}; var proxy = new Proxy(target, {deleteProperty() {return true;}}); Reflect.deleteProperty(proxy, 'answer') && target.answer === 42 ? 42 : 0;",
        "var buffer = new ArrayBuffer(8, {maxByteLength:16}); var prototype = new Uint8Array(buffer); var target = Object.create(prototype); var value = {valueOf() {buffer.resize(0); return 7;}}; Reflect.set(target, '0', value, prototype) && prototype.length === 0 ? 42 : 0;",
        "var prototype = new Uint8Array([7]); var target = Object.create(prototype); var reads = 0; var value = {valueOf() {reads++; return 42;}}; var accepted = true; for (var key of ['-0','NaN','Infinity','0.5']) {accepted = Reflect.set(target,key,value,prototype) && accepted;} accepted && reads === 4 && prototype[0] === 7 ? 42 : 0;",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        assert_eq!(vm.execute_script(&compile(&parse(source).unwrap()).unwrap()), Ok(Value::Number(42.0)), "{source}");
        assert!(vm.stack.is_empty());
        assert_eq!(vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()), Ok(Value::Number(42.0)));
    }
    for (source, key) in [
        ("Object.create(Iterator.prototype)", "flatMap"),
        (
            "globalThis.child = $262.createRealm().global; Object.create(child.eval('({})'))",
            "answer",
        ),
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        let receiver = vm
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        let target = receiver.object_id().unwrap();
        let root = vm.heap.root(target).unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.ordinary_set_with_receiver(target, &receiver, &key.into(), &Value::Number(42.0)),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })),
            "{source}"
        );
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.ordinary_set_with_receiver(target, &receiver, &key.into(), &Value::Number(42.0)),
            Ok(true)
        );
        assert_eq!(
            vm.heap.get_own(target, key).unwrap(),
            Some(Value::Number(42.0))
        );
        vm.heap.unroot(root).unwrap();
    }
    let mut vm = Vm::default();
    let owner = vm
        .execute_script(&compile(&parse("Iterator.prototype").unwrap()).unwrap())
        .unwrap()
        .object_id()
        .unwrap();
    let root = vm.heap.root(owner).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.object_delete(owner, &"flatMap".into()),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(vm.object_delete(owner, &"flatMap".into()), Ok(true));
    vm.heap.unroot(root).unwrap();

    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let candidate = vm
        .execute_script(
            &compile(
                &parse("$262.createRealm().global.eval('Object.create({answer:42})')").unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
        .object_id()
        .unwrap();
    let candidate_root = vm.heap.root(candidate).unwrap();
    let target = vm.heap.alloc_object(None).unwrap();
    let target_root = vm.heap.root(target).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    // The child's fresh ordinary prototype has not crossed the membrane.
    // Walking it must allocate a facade in this heap before publishing it.
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.object_set_prototype(target, Some(candidate)),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.heap.prototype(target).unwrap(), None);
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(vm.object_set_prototype(target, Some(candidate)), Ok(true));
    assert_eq!(vm.heap.prototype(target).unwrap(), Some(candidate));
    assert!(vm.stack.is_empty());
    vm.heap.unroot(target_root).unwrap();
    vm.heap.unroot(candidate_root).unwrap();
}
