// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{Heap, HeapConfig, HeapError, JsString, JsSymbol, PropertyDescriptor as D, PropertyName, Value};

#[test]
fn immutable_descriptors_and_nonextensible_objects() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    assert!(heap.is_extensible(object).unwrap());
    assert!(!heap.define_own_property(object, "mixed", D { get: Some(Value::Undefined), value: Some(Value::Null), ..D::default() }).unwrap());
    heap.define_own_property(object, "fixed", D::data(Value::Number(f64::NAN), false, true, false)).unwrap();
    assert!(heap.define_own_property(object, "fixed", D { value: Some(Value::Number(f64::NAN)), ..D::default() }).unwrap());
    for descriptor in [D { enumerable: Some(false), ..D::default() }, D { configurable: Some(true), ..D::default() }, D { get: Some(Value::Undefined), ..D::default() }] {
        assert!(!heap.define_own_property(object, "fixed", descriptor).unwrap());
    }
    heap.define_own_property(object, "accessor", D { get: Some(Value::Undefined), ..D::default() }).unwrap();
    assert!(!heap.define_own_property(object, "accessor", D { get: Some(Value::Null), ..D::default() }).unwrap());
    assert!(!heap.define_own_property(object, "accessor", D { set: Some(Value::Null), ..D::default() }).unwrap());
    assert_eq!(heap.enumerable_own_keys(object).unwrap(), vec![JsString::from("fixed")]);
    assert!(!heap.delete(object, "fixed").unwrap());
    assert_eq!(heap.set(object, "fixed", Value::Null), Err(HeapError::ReadOnlyProperty));
    heap.prevent_extensions(object).unwrap();
    assert!(!heap.is_extensible(object).unwrap());
    assert!(!heap.define_own_property(object, "new", D::default()).unwrap());
    assert_eq!(heap.set(object, "new", Value::Null), Err(HeapError::ReadOnlyProperty));
    assert!(heap.set_prototype(object, None).is_ok());
    let other = heap.alloc_object(None).unwrap();
    assert_eq!(heap.set_prototype(object, Some(other)), Err(HeapError::ReadOnlyProperty));
}

#[test]
fn descriptors_trace_accessors_and_preserve_symbol_order() {
    let mut heap = Heap::new(HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: 32768 }).unwrap();
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let getter = heap.alloc_object(None).unwrap();
    let symbol = JsSymbol::new(Some("key".into()));
    heap.define_own_property(object, symbol.clone(), D { get: Some(Value::Object(getter)), configurable: Some(true), ..D::default() }).unwrap();
    heap.set(object, "x", Value::Null).unwrap();
    heap.collect_major();
    assert!(heap.get_own_property_descriptor(getter, "missing").unwrap().is_none());
    assert_eq!(heap.own_keys(object).unwrap(), vec![JsString::from("x")]);
    assert_eq!(heap.own_property_keys(object).unwrap(), vec![PropertyName::from("x"), symbol.clone().into()]);
    assert!(heap.delete(object, symbol).unwrap());
    heap.collect_major();
    assert!(matches!(heap.get(getter, "x"), Err(HeapError::InvalidObject(_))));
    let utf8 = "ab".to_owned();
    let left = JsString::from(&utf8);
    let copied = JsString::from(&left);
    assert_eq!(left, copied);
    assert!(left == "ab");
}

#[test]
fn array_truncation_stops_at_nonconfigurable_elements() {
    let mut heap = Heap::default();
    let array = heap.alloc_array(0, None).unwrap();
    let invalid_length = heap.define_own_property(array, "length", D { value: Some(Value::Number(-1.0)), ..D::default() }).unwrap_err();
    assert_eq!(invalid_length, HeapError::InvalidArrayLength);
    assert!(matches!(blueice_bluejs::RuntimeError::from(invalid_length), blueice_bluejs::RuntimeError::RangeError(_)));
    heap.define_own_property(array, "2", D::data(Value::Null, true, true, false)).unwrap();
    heap.set(array, "4", Value::Bool(true)).unwrap();
    assert_eq!(heap.set(array, "length", Value::Number(1.0)), Err(HeapError::ReadOnlyProperty));
    assert_eq!(heap.get(array, "length").unwrap(), Value::Number(3.0));
    assert_eq!(heap.get(array, "4").unwrap(), Value::Undefined);
    assert_eq!(heap.get(array, "2").unwrap(), Value::Null);
    assert!(!heap.define_own_property(array, "length", D { value: Some(Value::Number(0.0)), writable: Some(false), ..D::default() }).unwrap());
    assert_eq!(heap.get_own_property_descriptor(array, "length").unwrap().unwrap().writable, Some(false));
    assert_eq!(heap.set(array, "3", Value::Null), Err(HeapError::ReadOnlyProperty));
}
