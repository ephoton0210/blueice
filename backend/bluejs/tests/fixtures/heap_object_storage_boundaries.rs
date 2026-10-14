// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Object storage ingress, virtual properties and malformed internal links.

use super::*;

#[cfg_attr(test, test)]
fn symbol_descriptors_reject_foreign_and_reclaimed_receivers() {
    let mut owner = Heap::new(HeapConfig::default()).unwrap();
    let foreign = owner.alloc_object(None).unwrap();
    let root = owner.root(foreign).unwrap();
    let mut heap = Heap::new(HeapConfig::default()).unwrap();
    let reclaimed = heap.alloc_object(None).unwrap();
    heap.collect_major();
    for receiver in [foreign, reclaimed] {
        let symbol = JsSymbol::new(Some("property".into()));
        assert_eq!(
            heap.define_own_property(
                receiver,
                symbol.clone(),
                PropertyDescriptor::data(Value::Number(7.0), true, true, true),
            ),
            Err(HeapError::InvalidObject(receiver))
        );
        assert!(matches!(
            heap.get_own_property_descriptor(receiver, symbol),
            Err(HeapError::InvalidObject(id)) if id == receiver
        ));
    }
    owner.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn boxed_string_virtual_properties_survive_deletion_and_skip_length_enumeration() {
    let mut heap = Heap::new(HeapConfig::default()).unwrap();
    let object = heap.alloc_object(None).unwrap();
    let root = heap.root(object).unwrap();
    heap.set(object, "length", Value::Number(7.0)).unwrap();
    heap.set(object, "named", Value::Bool(true)).unwrap();
    for (name, enumerable) in [("hidden", false), ("visible", true)] {
        assert_eq!(
            heap.define_own_property(
                object,
                name,
                PropertyDescriptor::data(Value::Number(7.0), true, enumerable, true),
            ),
            Ok(true)
        );
    }
    assert_eq!(
        heap.define_own_property(
            object,
            "accessor",
            PropertyDescriptor {
                get: Some(Value::Undefined),
                set: Some(Value::Undefined),
                enumerable: Some(false),
                configurable: Some(true),
                ..PropertyDescriptor::default()
            },
        ),
        Ok(true)
    );
    let descriptor = heap
        .get_own_property_descriptor(object, "accessor")
        .unwrap()
        .unwrap();
    assert!(descriptor.accessor());
    assert_eq!(descriptor.get, Some(Value::Undefined));
    assert_eq!(descriptor.value, None);
    assert_eq!(
        heap.enumerable_own_keys(object),
        Ok(vec!["length".into(), "named".into(), "visible".into()])
    );
    let array = heap.alloc_array(7, None).unwrap();
    let array_root = heap.root(array).unwrap();
    assert_eq!(heap.delete(array, "length"), Ok(false));
    assert_eq!(heap.get(array, "length"), Ok(Value::Number(7.0)));
    assert_eq!(heap.enumerable_own_keys(array), Ok(vec![]));
    heap.unroot(array_root).unwrap();
    for text in ["", "ab"] {
        let string = heap.alloc_string(text.into(), None).unwrap();
        let string_root = heap.root(string).unwrap();
        heap.set(string, "named", Value::Bool(true)).unwrap();
        assert_eq!(heap.delete(string, "length"), Ok(false));
        if !text.is_empty() {
            assert_eq!(heap.delete(string, "0"), Ok(false));
            assert_eq!(heap.get(string, "0"), Ok(Value::String("a".into())));
            assert_eq!(
                heap.enumerable_own_keys(string),
                Ok(vec!["0".into(), "1".into(), "named".into()])
            );
        } else {
            assert_eq!(heap.delete(string, "0"), Ok(true));
            assert_eq!(heap.enumerable_own_keys(string), Ok(vec!["named".into()]));
        }
        heap.unroot(string_root).unwrap();
    }
    heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn malformed_property_links_report_errors_without_reading_other_heap_storage() {
    let mut heap = Heap::default();
    let mut other = Heap::default();
    let foreign = other.alloc_object(None).unwrap();
    let ordinary = heap.alloc_object(None).unwrap();
    let cell = heap.alloc_object(None).unwrap();
    heap.set(cell, "value", Value::Number(1.0)).unwrap();

    let namespace = heap
        .alloc_module_namespace(vec![("x".into(), cell)], false)
        .unwrap();
    let ObjectKind::ModuleNamespace { exports } =
        &mut heap.objects.get_mut(&namespace).unwrap().kind
    else {
        panic!("expected module namespace");
    };
    exports[0].1 = foreign;
    assert_eq!(
        heap.get_own(namespace, "x"),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(
        heap.get(namespace, "x"),
        Err(HeapError::InvalidObject(foreign))
    );
    assert!(matches!(
        heap.get_own_property_descriptor(namespace, "x"),
        Err(HeapError::InvalidObject(id)) if id == foreign
    ));
    assert_eq!(
        heap.define_own_property(
            namespace,
            "x",
            PropertyDescriptor::data(Value::Number(1.0), true, true, false),
        ),
        Err(HeapError::InvalidObject(foreign))
    );

    let arguments = heap
        .alloc_arguments(
            HashMap::from([
                (PropertyName::from("0"), cell),
                (PropertyName::from("1"), cell),
            ]),
            ordinary,
        )
        .unwrap();
    heap.set(arguments, "0", Value::Number(2.0)).unwrap();
    heap.define_own_property(
        arguments,
        "1",
        PropertyDescriptor::data(Value::Number(3.0), true, true, true),
    )
    .unwrap();
    let ObjectKind::Arguments { parameter_map } =
        &mut heap.objects.get_mut(&arguments).unwrap().kind
    else {
        panic!("expected arguments object");
    };
    parameter_map.insert("0".into(), foreign);
    parameter_map.insert("1".into(), foreign);
    assert_eq!(
        heap.get(arguments, "0"),
        Err(HeapError::InvalidObject(foreign))
    );
    for key in ["0", "1"] {
        assert!(matches!(
            heap.get_own_property_descriptor(arguments, key),
            Err(HeapError::InvalidObject(id)) if id == foreign
        ));
    }
    assert_eq!(
        heap.define_own_property(
            arguments,
            "0",
            PropertyDescriptor::data(Value::Number(4.0), true, true, true),
        ),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(heap.unmap_arguments_property(ordinary, &"0".into()), Ok(()));

    let buffer = heap.alloc_array_buffer(1, None).unwrap();
    let view = heap
        .alloc_typed_array(buffer, 0, 1, false, TypedArrayKind::Uint8, None)
        .unwrap();
    let ObjectKind::TypedArray { buffer, .. } = &mut heap.objects.get_mut(&view).unwrap().kind
    else {
        panic!("expected typed array");
    };
    *buffer = foreign;
    assert_eq!(
        heap.get_own(view, "0"),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(heap.get(view, "0"), Err(HeapError::InvalidObject(foreign)));
    assert!(matches!(
        heap.get_own_property_descriptor(view, "0"),
        Err(HeapError::InvalidObject(id)) if id == foreign
    ));
    assert_eq!(
        heap.delete(view, "0"),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(
        heap.own_property_keys(view),
        Err(HeapError::InvalidObject(foreign))
    );
    assert_eq!(
        heap.enumerable_own_keys(view),
        Err(HeapError::InvalidObject(foreign))
    );
}

impl Heap {
    #[doc(hidden)]
    pub fn verify_object_storage_boundary_contracts() {
        malformed_property_links_report_errors_without_reading_other_heap_storage();
        symbol_descriptors_reject_foreign_and_reclaimed_receivers();
        boxed_string_virtual_properties_survive_deletion_and_skip_length_enumeration();
    }
}
