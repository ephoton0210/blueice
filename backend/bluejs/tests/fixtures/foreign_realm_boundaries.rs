// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Membrane ingress and allocation refusals use live objects and real realms.

use super::*;
use crate::{compile, parse};

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn membrane_argument_exports_reject_handles_owned_by_another_live_heap() {
    let mut owner = Vm::default();
    let other = owner.heap.alloc_object(None).unwrap();
    let owner_root = owner.heap.root(other).unwrap();
    let value = Value::Object(other);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    let realm = *vm.test262_realms.keys().next().unwrap();
    let object = execute(
        &mut vm,
        "child.eval('({value:7, set setter(value) {}, next(value) {return {done:true,value};}})')",
    )
    .object_id()
    .unwrap();
    let object_root = vm.heap.root(object).unwrap();
    let expected = RuntimeError::Heap(HeapError::InvalidObject(other));
    for result in [
        vm.test262_import_foreign_value(realm, value.clone())
            .map(|_| ()),
        vm.test262_transport_value(realm, value.clone()).map(|_| ()),
        vm.test262_foreign_set_prototype(object, Some(other))
            .map(|_| ()),
        vm.test262_foreign_set_with_receiver(object, &value, &"value".into(), &Value::Number(42.0))
            .map(|_| ()),
        vm.test262_foreign_set_with_receiver(
            object,
            &Value::Object(object),
            &"value".into(),
            &value,
        )
        .map(|_| ()),
        vm.test262_foreign_define_own_property(
            object,
            "newValue".into(),
            PropertyDescriptor::data(value.clone(), true, true, true),
        )
        .map(|_| ()),
        vm.test262_foreign_set(object, &"value".into(), &value),
        vm.test262_foreign_set(object, &"setter".into(), &value),
        vm.test262_foreign_next(&Value::Object(object), "next", std::slice::from_ref(&value))
            .map(|_| ()),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    let array = execute(&mut vm, "child.eval('new Uint8Array([7,42])')");
    let array_root = vm.heap.root(array.object_id().unwrap()).unwrap();
    assert_eq!(
        vm.test262_foreign_typed_array_native_call(
            NativeFunction::TypedArraySet,
            array,
            vec![value.clone()],
            false
        ),
        Err(expected.clone())
    );
    let buffer = execute(&mut vm, "child.eval('new ArrayBuffer(8)')");
    let buffer_root = vm.heap.root(buffer.object_id().unwrap()).unwrap();
    assert_eq!(
        vm.test262_foreign_array_buffer_native_call(
            NativeFunction::ArrayBufferSlice,
            buffer,
            vec![value],
            false
        ),
        Err(expected)
    );
    let (_, target, _, _) = vm.test262_foreign_reference(object).unwrap();
    assert_eq!(
        vm.test262_foreign_typed_array_native_call(
            NativeFunction::TypedArrayLength,
            Value::Object(object),
            Vec::new(),
            false
        ),
        Err(RuntimeError::from(HeapError::InvalidInternalSlot(target)))
    );
    assert!(vm.stack.is_empty() && vm.test262_realms[&realm].vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    vm.heap.unroot(buffer_root).unwrap();
    vm.heap.unroot(array_root).unwrap();
    vm.heap.unroot(object_root).unwrap();
    owner.heap.unroot(owner_root).unwrap();
}

#[cfg_attr(test, test)]
fn a_fresh_child_harness_preserves_its_actual_configuration_limit_errors() {
    let mut completed = false;
    let mut refusals = 0;
    for bytes in [
        1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288,
    ] {
        let mut config = VmConfig::default();
        config.heap.max_heap_bytes = bytes;
        config.heap.major_threshold_bytes = bytes;
        let mut vm = match Vm::new(config) {
            Ok(vm) => vm,
            Err(HeapError::HeapLimitExceeded { limit }) => {
                assert_eq!(limit, bytes);
                continue;
            }
            Err(error) => panic!("VM construction with {bytes} bytes: {error:?}"),
        };
        match vm.test262_create_realm() {
            Ok(value) => {
                assert!(vm.heap.contains(value.object_id().unwrap()));
                completed = true;
            }
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })) => {
                assert_eq!(limit, bytes);
                refusals += 1;
            }
            Err(error) => panic!("realm construction with {bytes} bytes: {error:?}"),
        }
        assert!(vm.stack.is_empty());
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
        if completed {
            break;
        }
    }
    assert!(completed && refusals > 0);
}

#[cfg_attr(test, test)]
fn non_constructor_intrinsics_have_no_constructor_prototype_fallback() {
    for function in [
        NativeFunction::Empty,
        NativeFunction::Symbol,
        NativeFunction::Proxy,
        NativeFunction::BigInt,
        NativeFunction::ArrayAt,
    ] {
        assert_eq!(foreign_constructor_intrinsic(function), None);
    }
}

#[cfg_attr(test, test)]
fn staged_foreign_buffer_clones_preserve_allocation_and_root_refusals() {
    for (source, shared) in [
        ("new ArrayBuffer(8)", false),
        ("new ArrayBuffer(8, {maxByteLength:8192})", false),
        ("new ArrayBuffer(8).transferToImmutable()", false),
        ("new SharedArrayBuffer(8, {maxByteLength:8192})", true),
    ] {
        for warm in [false, true] {
            let mut extra = 0;
            let mut complete = false;
            let mut refused = 0;
            while extra <= 16 * 1024 * 1024 {
                let mut vm = Vm::default();
                vm.install_test262_harness().unwrap();
                execute(&mut vm, "globalThis.child = $262.createRealm().global;");
                let wrapper = execute(&mut vm, &format!("child.eval('{source}')"))
                    .object_id()
                    .unwrap();
                let root = vm.heap.root(wrapper).unwrap();
                if warm {
                    vm.buffer_prototype(if shared {
                        "SharedArrayBuffer"
                    } else {
                        "ArrayBuffer"
                    })
                    .unwrap();
                }
                vm.with_roots(|heap| {
                    heap.collect_major();
                    Ok(())
                })
                .unwrap();
                let limit = vm.heap.allow_only(extra);
                match vm.test262_foreign_buffer_clone(wrapper) {
                    Ok(Some(buffer)) => {
                        assert_eq!(vm.heap.buffer_byte_length(buffer).unwrap(), 8);
                        assert_eq!(vm.heap.buffer_is_shared(buffer).unwrap(), shared);
                        if !shared {
                            assert_eq!(vm.test262_foreign_buffer_clone(wrapper), Ok(Some(buffer)));
                        }
                        complete = true;
                    }
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                        assert_eq!(actual, limit);
                        assert!(vm.test262_foreign_buffer_mirrors.is_empty());
                        refused += 1;
                        extra = vm.heap.next_allocation_headroom(extra);
                    }
                    result => {
                        panic!("buffer clone {source}, warm={warm}, headroom={extra}: {result:?}")
                    }
                }
                assert!(vm.stack.is_empty());
                vm.heap.allow_only(16 * 1024 * 1024);
                assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
                vm.heap.unroot(root).unwrap();
                if complete {
                    break;
                }
            }
            assert!(complete && refused > 0, "{source}, warm={warm}");
        }
        if !shared {
            let mut vm = Vm::default();
            vm.install_test262_harness().unwrap();
            execute(&mut vm, "globalThis.child = $262.createRealm().global;");
            let wrapper = execute(&mut vm, &format!("child.eval('{source}')"))
                .object_id()
                .unwrap();
            vm.buffer_prototype("ArrayBuffer").unwrap();
            vm.heap.allow_root_registrations(0);
            assert_eq!(
                vm.test262_foreign_buffer_clone(wrapper),
                Err(RuntimeError::Heap(HeapError::IdExhausted))
            );
            assert!(vm.test262_foreign_buffer_mirrors.is_empty());
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
        }
    }
}

#[cfg_attr(test, test)]
fn foreign_mirror_growth_refusal_keeps_the_original_backing_and_metadata() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    let wrapper = execute(
        &mut vm,
        "child.eval('globalThis.buffer = new ArrayBuffer(8, {maxByteLength:8192}); buffer')",
    )
    .object_id()
    .unwrap();
    let (realm, target, _, _) = vm.test262_foreign_reference(wrapper).unwrap();
    let mirror = vm.test262_foreign_buffer_clone(wrapper).unwrap().unwrap();
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .resize_array_buffer(target, 4096)
        .unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.test262_refresh_foreign_buffer_mirrors(realm, target),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.heap.buffer_byte_length(mirror).unwrap(), 8);
    assert_eq!(
        vm.test262_foreign_buffer_mirrors[&(mirror, realm)].target,
        target
    );
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.test262_refresh_foreign_buffer_mirrors(realm, target)
        .unwrap();
    assert_eq!(vm.heap.buffer_byte_length(mirror).unwrap(), 4096);
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn cold_destination_buffer_transport_keeps_lazy_prototype_refusals() {
    for source in [
        "new ArrayBuffer(8)",
        "new SharedArrayBuffer(8)",
        "new Uint8Array([7,42])",
    ] {
        let mut extra = 0;
        let mut complete = false;
        let mut refused = 0;
        while extra <= 16 * 1024 * 1024 {
            let mut vm = Vm::default();
            vm.install_test262_harness().unwrap();
            execute(&mut vm, "globalThis.child = $262.createRealm().global;");
            let realm = *vm.test262_realms.keys().next().unwrap();
            let value = execute(&mut vm, source);
            let root = vm.heap.root(value.object_id().unwrap()).unwrap();
            let limit = vm
                .test262_realms
                .get_mut(&realm)
                .unwrap()
                .vm
                .heap
                .allow_only(extra);
            match vm.test262_transport_value(realm, value) {
                Ok(value) => {
                    assert!(vm.test262_realms[&realm]
                        .vm
                        .heap
                        .contains(value.object_id().unwrap()));
                    complete = true;
                }
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit);
                    assert!(vm.test262_realms[&realm].imported_sources.is_empty());
                    refused += 1;
                    extra = vm.test262_realms[&realm]
                        .vm
                        .heap
                        .next_allocation_headroom(extra);
                }
                result => panic!("cold destination {source}, headroom={extra}: {result:?}"),
            }
            let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
            assert!(child.stack.is_empty());
            child.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(execute(child, "21 + 21"), Value::Number(42.0));
            assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
            vm.heap.unroot(root).unwrap();
            if complete {
                break;
            }
        }
        assert!(complete && refused > 0, "{source}");
    }
}

#[cfg_attr(test, test)]
fn detached_and_out_of_bounds_typed_array_transports_reject_actual_views() {
    for detach in [false, true] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        execute(&mut vm, "globalThis.child = $262.createRealm().global; globalThis.buffer = new ArrayBuffer(8, {maxByteLength:16}); globalThis.view = new Uint8Array(buffer, 4, 4);");
        let realm = *vm.test262_realms.keys().next().unwrap();
        let view = execute(&mut vm, "view");
        let buffer = execute(&mut vm, "buffer").object_id().unwrap();
        if detach {
            vm.heap.detach_array_buffer(buffer).unwrap();
        } else {
            vm.heap.resize_array_buffer(buffer, 0).unwrap();
        }
        assert_eq!(
            vm.test262_transport_value(realm, view),
            Err(RuntimeError::TypeError(
                "TypedArray source is detached or out of bounds".into()
            ))
        );
        assert!(vm.stack.is_empty());
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_foreign_realm_boundary_contracts() {
        detached_local_mirrors_and_other_realm_host_hooks_preserve_buffer_ownership();
        foreign_native_fill_preserves_post_callback_mirror_growth_refusal();
        foreign_completion_and_detachment_preserve_real_resource_boundaries();
        owned_membrane_prototypes_and_detachment_preserve_real_exceptions();
        membrane_argument_exports_reject_handles_owned_by_another_live_heap();
        a_fresh_child_harness_preserves_its_actual_configuration_limit_errors();
        non_constructor_intrinsics_have_no_constructor_prototype_fallback();
        staged_foreign_buffer_clones_preserve_allocation_and_root_refusals();
        foreign_mirror_growth_refusal_keeps_the_original_backing_and_metadata();
        cold_destination_buffer_transport_keeps_lazy_prototype_refusals();
        detached_and_out_of_bounds_typed_array_transports_reject_actual_views();
        shadow_reexports_and_foreign_apply_preserve_destination_refusals();
        buffer_transport_records_retain_both_heaps_across_collection_and_flush();
        foreign_internal_methods_preserve_actual_target_exceptions();
        foreign_error_materialization_and_snapshot_roots_preserve_refusals();
    }
}

#[cfg_attr(test, test)]
fn detached_local_mirrors_and_other_realm_host_hooks_preserve_buffer_ownership() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global; globalThis.other = $262.createRealm().global;");
    let facade = execute(&mut vm, "child.eval('new ArrayBuffer(8)')")
        .object_id()
        .unwrap();
    let root = vm.heap.root(facade).unwrap();
    let (realm, target, ..) = vm.test262_foreign_reference(facade).unwrap();
    let mirror = vm.test262_foreign_buffer_clone(facade).unwrap().unwrap();
    vm.heap.detach_array_buffer(mirror).unwrap();
    vm.test262_refresh_foreign_buffer_mirrors(realm, target)
        .unwrap();
    assert!(vm.heap.buffer_is_detached(mirror).unwrap());
    assert!(!vm.test262_realms[&realm]
        .vm
        .heap
        .buffer_is_detached(target)
        .unwrap());
    let detach = execute(&mut vm, "other.$262.detachArrayBuffer")
        .object_id()
        .unwrap();
    let detach_root = vm.heap.root(detach).unwrap();
    assert_eq!(
        vm.test262_foreign_call(detach, Value::Undefined, vec![Value::Object(facade)], false),
        Ok(Value::Undefined)
    );
    assert!(vm.test262_realms[&realm]
        .vm
        .heap
        .buffer_is_detached(target)
        .unwrap());
    for warm in [false, true] {
        let local = execute(&mut vm, "new ArrayBuffer(8)").object_id().unwrap();
        let local_root = vm.heap.root(local).unwrap();
        let (hook_realm, ..) = vm.test262_foreign_reference(detach).unwrap();
        if warm {
            vm.test262_transport_value(hook_realm, Value::Object(local))
                .unwrap();
        }
        assert_eq!(
            vm.test262_foreign_call(detach, Value::Undefined, vec![Value::Object(local)], false),
            Ok(Value::Undefined)
        );
        assert!(vm.heap.buffer_is_detached(local).unwrap());
        vm.heap.unroot(local_root).unwrap();
    }
    for source in [
        "var ctor = child.eval('(function(){this.answer=42;})'); var Target = function() {}; Reflect.construct(ctor, [], Target).answer",
        "var ctor = child.eval('(function(){this.answer=42;}).bind(null)'); var Target = function() {}; Reflect.construct(ctor, [], Target).answer",
        "var remote = child.eval('new ArrayBuffer(8)'); other.$262.detachArrayBuffer(remote); remote.byteLength === 0 ? 42 : 0",
    ] {assert_eq!(execute(&mut vm, source), Value::Number(42.0), "{source}");}
    vm.heap.unroot(detach_root).unwrap();
    vm.heap.unroot(root).unwrap();
    assert!(vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn owned_membrane_prototypes_and_detachment_preserve_real_exceptions() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    for source in [
        "new Proxy({}, {getOwnPropertyDescriptor(){throw 7;}})",
        "new Proxy({}, {getPrototypeOf(){throw 7;}})",
    ] {
        let wrapper = execute(&mut vm, &format!("child.eval('{source}')"))
            .object_id()
            .unwrap();
        let wrapper_root = vm.heap.root(wrapper).unwrap();
        let local = vm.heap.alloc_object(Some(wrapper)).unwrap();
        let local_root = vm.heap.root(local).unwrap();
        let thrown = Err(RuntimeError::Thrown(Value::Number(7.0)));
        if source.contains("getOwnPropertyDescriptor") {
            assert_eq!(
                vm.test262_foreign_get_own_property(wrapper, &"missing".into())
                    .map(|_| ()),
                thrown
            );
        } else {
            assert_eq!(
                vm.test262_foreign_get_prototype(wrapper).map(|_| ()),
                thrown
            );
        }
        // Proxy [[HasProperty]] and [[Set]] without those traps delegate to
        // the target, so descriptor/prototype traps must remain unobserved.
        assert_eq!(vm.has_property(wrapper, &"missing".into()), Ok(false));
        assert_eq!(
            vm.ordinary_set_with_receiver(
                local,
                &Value::Object(local),
                &"missing".into(),
                &Value::Number(42.0)
            ),
            Ok(true)
        );
        assert_eq!(vm.object_set_prototype(local, Some(wrapper)), Ok(true));
        vm.heap.unroot(local_root).unwrap();
        vm.heap.unroot(wrapper_root).unwrap();
    }
    for trap in ["getOwnPropertyDescriptor", "getPrototypeOf"] {
        let argument = if trap == "getOwnPropertyDescriptor" {
            ", \"missing\""
        } else {
            ""
        };
        let script = format!(
            r#"child.eval('globalThis.read = object => Object.{trap}(object{argument})'); var object = new Proxy({{}}, {{{trap}(){{throw 7;}}}}); var caught = false; try {{child.read(object);}} catch (error) {{caught = error === 7;}} caught"#
        );
        assert_eq!(execute(&mut vm, &script), Value::Bool(true));
    }
    let mut owner = Vm::default();
    let foreign = owner
        .execute_script(&compile(&parse("()=>42").unwrap()).unwrap())
        .unwrap();
    let foreign_root = owner.heap.root(foreign.object_id().unwrap()).unwrap();
    let call = execute(&mut vm, "child.Function.prototype.call")
        .object_id()
        .unwrap();
    let call_root = vm.heap.root(call).unwrap();
    let apply = execute(&mut vm, "child.Function.prototype.apply");
    assert_eq!(
        vm.test262_foreign_call(call, apply, vec![foreign.clone()], false),
        Err(RuntimeError::Heap(HeapError::InvalidObject(
            foreign.object_id().unwrap()
        )))
    );
    vm.heap.unroot(call_root).unwrap();
    owner.heap.unroot(foreign_root).unwrap();

    let realm = *vm.test262_realms.keys().next().unwrap();
    let immutable = execute(&mut vm, "new ArrayBuffer(8).transferToImmutable()")
        .object_id()
        .unwrap();
    let immutable_root = vm.heap.root(immutable).unwrap();
    vm.test262_transport_value(realm, Value::Object(immutable))
        .unwrap();
    assert_eq!(
        vm.test262_detach_local_buffer_mirrors(immutable),
        Err(RuntimeError::from(HeapError::ImmutableArrayBuffer))
    );
    let detach = execute(&mut vm, "child.$262.detachArrayBuffer")
        .object_id()
        .unwrap();
    let detach_root = vm.heap.root(detach).unwrap();
    assert_eq!(
        vm.test262_foreign_call(
            detach,
            Value::Undefined,
            vec![Value::Object(immutable)],
            false
        ),
        Err(RuntimeError::from(HeapError::ImmutableArrayBuffer))
    );
    let remote = execute(
        &mut vm,
        "child.eval('new ArrayBuffer(8).transferToImmutable()')",
    );
    assert_eq!(
        vm.test262_foreign_call(detach, Value::Undefined, vec![remote], false),
        Err(RuntimeError::from(HeapError::ImmutableArrayBuffer))
    );
    assert_eq!(vm.heap.buffer_byte_length(immutable).unwrap(), 8);
    vm.heap.unroot(detach_root).unwrap();
    vm.heap.unroot(immutable_root).unwrap();

    let buffer = execute(&mut vm, "new ArrayBuffer(8,{maxByteLength:16})")
        .object_id()
        .unwrap();
    let root = vm.heap.root(buffer).unwrap();
    let imported = vm
        .test262_transport_value(realm, Value::Object(buffer))
        .unwrap()
        .object_id()
        .unwrap();
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .resize_array_buffer(imported, 16)
        .unwrap();
    vm.test262_sync_foreign_buffer_mirrors(realm);
    vm.heap.detach_array_buffer(buffer).unwrap();
    vm.test262_sync_foreign_buffer_mirrors(realm);
    vm.test262_refresh_foreign_buffer_mirrors(realm, imported)
        .unwrap();
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .detach_array_buffer(imported)
        .unwrap();
    vm.test262_detach_local_buffer_mirrors(buffer).unwrap();
    vm.heap.unroot(root).unwrap();
    assert_eq!(execute(&mut vm, "var Derived = child.eval('(class extends null {constructor(){return 7;}}).bind(null)'); var failed = false; try {new Derived();} catch(error) {failed = error.name === 'TypeError';} failed"), Value::Bool(true));
    assert!(vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn foreign_internal_methods_preserve_actual_target_exceptions() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    let wrapper = execute(&mut vm, "child.eval('new Proxy({}, {getPrototypeOf() {throw 7;}, getOwnPropertyDescriptor() {throw 7;}, setPrototypeOf() {throw 7;}})')").object_id().unwrap();
    for result in [
        vm.test262_foreign_get_prototype(wrapper).map(|_| ()),
        vm.test262_foreign_get_own_property(wrapper, &"value".into())
            .map(|_| ()),
        vm.test262_foreign_set_prototype(wrapper, None).map(|_| ()),
    ] {
        assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
    }
    let wrapper = execute(
        &mut vm,
        "child.eval('({get next() {throw 7;}, get value() {throw 7;}})')",
    )
    .object_id()
    .unwrap();
    assert_eq!(
        vm.test262_foreign_next(&Value::Object(wrapper), "next", &[]),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    let helper = execute(&mut vm, "$262.detachArrayBuffer");
    assert_eq!(
        vm.test262_foreign_set(wrapper, &"value".into(), &helper),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn foreign_error_materialization_and_snapshot_roots_preserve_refusals() {
    for child_limit in [false, true] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        execute(&mut vm, "globalThis.child = $262.createRealm().global;");
        let realm = *vm.test262_realms.keys().next().unwrap();
        vm.test262_realms
            .get_mut(&realm)
            .unwrap()
            .vm
            .global("TypeError")
            .unwrap();
        let limit = if child_limit {
            vm.test262_realms
                .get_mut(&realm)
                .unwrap()
                .vm
                .heap
                .allow_only(0)
        } else {
            vm.heap.allow_only(0)
        };
        assert_eq!(
            vm.test262_create_error_in_realm(
                realm,
                RuntimeError::TypeError("actual native failure".into())
            ),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.test262_realms
            .get_mut(&realm)
            .unwrap()
            .vm
            .heap
            .allow_only(16 * 1024 * 1024);
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    let realm = *vm.test262_realms.keys().next().unwrap();
    let thrown = execute(
        &mut vm.test262_realms.get_mut(&realm).unwrap().vm,
        "({answer:42})",
    );
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.test262_foreign_completion(realm, Err::<bool, _>(RuntimeError::Thrown(thrown))),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));

    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    let realm = *vm.test262_realms.keys().next().unwrap();
    let buffer = execute(&mut vm, "new ArrayBuffer(8)");
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .buffer_prototype("ArrayBuffer")
        .unwrap();
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .allow_root_registrations(0);
    assert_eq!(
        vm.test262_transport_value(realm, buffer),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(
        vm.test262_realms[&realm].imported_sources.is_empty()
            && vm.test262_foreign_buffer_mirrors.is_empty()
    );
    assert!(vm.stack.is_empty());
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shadow_reexports_and_foreign_apply_preserve_destination_refusals() {
    for warm in [false, true] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        execute(&mut vm, "globalThis.first = $262.createRealm().global; globalThis.second = $262.createRealm().global;");
        let wrapper = execute(&mut vm, "first.eval('new ShadowRealm()')")
            .object_id()
            .unwrap();
        let (source, target, _, _) = vm.test262_foreign_reference(wrapper).unwrap();
        let second = execute(&mut vm, "second").object_id().unwrap();
        let (destination, _, _, _) = vm.test262_foreign_reference(second).unwrap();
        if warm {
            vm.test262_realms
                .get_mut(&destination)
                .unwrap()
                .vm
                .shadow_realm_prototype()
                .unwrap();
        }
        let limit = vm
            .test262_realms
            .get_mut(&destination)
            .unwrap()
            .vm
            .heap
            .allow_only(0);
        assert_eq!(
            vm.test262_export_foreign_value(destination, &Value::Object(wrapper)),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.test262_realms[&destination]
            .shadow_realm_reexports
            .is_empty());
        vm.test262_realms
            .get_mut(&destination)
            .unwrap()
            .vm
            .heap
            .allow_only(16 * 1024 * 1024);
        let exported = vm
            .export_foreign_shadow_realm(source, target, destination)
            .unwrap()
            .unwrap();
        assert_eq!(
            vm.test262_export_foreign_value(destination, &Value::Object(wrapper)),
            Ok(exported)
        );
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
    for child_limit in [false, true] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        execute(&mut vm, "globalThis.child = $262.createRealm().global;");
        let call = execute(&mut vm, "child.Function.prototype.call")
            .object_id()
            .unwrap();
        let apply = execute(&mut vm, "child.Function.prototype.apply");
        let (realm, _, _, _) = vm.test262_foreign_reference(call).unwrap();
        vm.test262_realms
            .get_mut(&realm)
            .unwrap()
            .vm
            .global("TypeError")
            .unwrap();
        let limit = if child_limit {
            vm.test262_realms
                .get_mut(&realm)
                .unwrap()
                .vm
                .heap
                .allow_only(0)
        } else {
            vm.heap.allow_only(0)
        };
        assert_eq!(
            vm.test262_foreign_call(call, apply, vec![Value::Number(7.0)], false),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.test262_realms
            .get_mut(&realm)
            .unwrap()
            .vm
            .heap
            .allow_only(16 * 1024 * 1024);
        assert!(vm.stack.is_empty() && vm.test262_realms[&realm].vm.stack.is_empty());
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn buffer_transport_records_retain_both_heaps_across_collection_and_flush() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute_script(&compile(&parse("$262.createRealm();").unwrap()).unwrap())
        .unwrap();
    let realm = *vm.test262_realms.keys().next().unwrap();
    let buffer = vm
        .with_roots(|heap| heap.alloc_array_buffer(8, None))
        .unwrap();
    let root = vm.heap.root(buffer).unwrap();
    let value = vm
        .test262_transport_value(realm, Value::Object(buffer))
        .unwrap();
    assert_eq!(
        value.object_id().unwrap().heap,
        vm.test262_realms[&realm].vm.object_prototype.heap
    );
    assert!(vm
        .test262_foreign_buffer_mirrors
        .contains_key(&(buffer, realm)));
    vm.heap.unroot(root).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(vm.heap.contains(buffer));
    let target = value.object_id().unwrap();
    vm.heap.array_buffer_write(buffer, 0, &[7]).unwrap();
    vm.test262_sync_foreign_buffer_mirrors(realm);
    let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
    child
        .with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    assert!(child.heap.contains(target));
    assert_eq!(child.heap.array_buffer_copy(target, 0, 1).unwrap(), vec![7]);
    child.heap.array_buffer_write(target, 0, &[42]).unwrap();
    vm.test262_refresh_foreign_buffer_mirrors(realm, target)
        .unwrap();
    assert_eq!(vm.heap.array_buffer_copy(buffer, 0, 1).unwrap(), vec![42]);
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn foreign_completion_and_detachment_preserve_real_resource_boundaries() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    execute(&mut vm, "globalThis.child = $262.createRealm().global;");
    let realm = *vm.test262_realms.keys().next().unwrap();
    let limit = {
        let child = &mut vm.test262_realms.get_mut(&realm).unwrap().vm;
        child
            .with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
        child.heap.allow_only(0)
    };
    assert_eq!(
        vm.test262_run_native_for_realm(
            realm,
            NativeFunction::ArrayBufferByteLength,
            Value::Undefined,
            Vec::new()
        ),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .allow_only(16 * 1024 * 1024);
    for source in [
        "var local = new ArrayBuffer(8); child.eval('globalThis.take = function(buffer) {return buffer.byteLength;};'); child.take(local); child.$262.detachArrayBuffer(local); local.byteLength === 0 ? 42 : 0",
        "var foreign = child.eval('new ArrayBuffer(8)'); var view = new Uint8Array(foreign); child.$262.detachArrayBuffer(foreign); view.length === 0 ? 42 : 0",
        "child.ParentSpecies = function(n) {return new Uint8Array(new ArrayBuffer(n).transferToImmutable());}; child.eval('var array = new Uint8Array([7,42]); array.constructor = {[Symbol.species]:ParentSpecies}; try {array.slice(); 0;} catch (error) {error instanceof TypeError ? 42 : 0;}')",
        "child.ParentSpecies = function(n) {return new Float64Array(n);}; child.eval('var array = new Uint8Array([7,42]); array.constructor = {[Symbol.species]:ParentSpecies}; var result = array.slice(); result[0] === 7 && result[1] === 42 ? 42 : 0;')",
    ] {
        assert_eq!(execute(&mut vm, source), Value::Number(42.0), "{source}");
        assert!(vm.stack.is_empty());
    }
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn foreign_native_fill_preserves_post_callback_mirror_growth_refusal() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let view = execute(&mut vm, "globalThis.child = $262.createRealm().global; child.eval('globalThis.backing = new ArrayBuffer(8, {maxByteLength:8192}); globalThis.view = new Uint8Array(backing); globalThis.coercion = {valueOf() {backing.resize(8192); return 7;}}; view')");
    let view_root = vm.heap.root(view.object_id().unwrap()).unwrap();
    let buffer = vm
        .get_property(&view, &"buffer".into())
        .unwrap()
        .object_id()
        .unwrap();
    let (realm, target, _, _) = vm.test262_foreign_reference(buffer).unwrap();
    let mirror = vm.test262_foreign_buffer_clone(buffer).unwrap().unwrap();
    let mirror_root = vm.heap.root(mirror).unwrap();
    let value = execute(&mut vm, "child.eval('coercion')");
    let value_root = vm.heap.root(value.object_id().unwrap()).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.test262_foreign_typed_array_native_call(
            NativeFunction::TypedArrayMethod(crate::native::TypedArrayMethod::Fill),
            view,
            vec![value],
            false
        ),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.heap.buffer_byte_length(mirror).unwrap(), 8);
    assert_eq!(
        vm.test262_realms[&realm]
            .vm
            .heap
            .buffer_byte_length(target)
            .unwrap(),
        8192
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.test262_refresh_foreign_buffer_mirrors(realm, target)
        .unwrap();
    assert_eq!(vm.heap.buffer_byte_length(mirror).unwrap(), 8192);
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    vm.heap.unroot(value_root).unwrap();
    vm.heap.unroot(mirror_root).unwrap();
    vm.heap.unroot(view_root).unwrap();
}
