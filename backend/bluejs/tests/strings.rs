// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, Heap, HeapConfig, HeapError, JsString, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Value {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn surrogate_literals_templates_and_concatenation_are_lossless() {
    for source in [
        r"'\ud800' === '\u{D800}'",
        r"'\udfff' === '\u{DFFF}'",
        r"'\ud83d\ude00' === '😀'",
        r"'\ud83d' + '\ude00' === '\u{1F600}'",
        r"`\ud800${'\udfff'}\u{D801}` === '\ud800\udfff\ud801'",
        r"'\udc00\ud800' !== '\ufffd\ufffd'",
        r"'\ud800' !== '\ufffd'",
        r"'\ud800' < '\ud801' && '\udfff' < '\ue000'",
        r"'\ud800' < '\ud800\u0000'",
        r"!!'\ud800' && !''",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn property_keys_preserve_code_unit_identity() {
    for source in [
        r"let o={'\ud800':1, '\ud801':2, '\ufffd':4}; o['\ud800']+o['\ud801']+o['\ufffd'] === 7",
        r"let k='\ud800'; let o={[k]:1}; o[k]+=2; o['\u{D800}']++ === 3 && o[k] === 4",
        r"let p={'\udfff':8}; let o={__proto__:p}; o['\udfff'] === 8",
        r"let o={'😀':1}; o['\ud83d'+'\ude00'] === 1",
        r"let a=[]; a['\ud800']=7; a.length === 0 && a['\ud800'] === 7",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn numeric_coercion_does_not_repair_surrogates() {
    for source in [r"+'\ud800'", r"+'1\udfff'", r"+'\ud800\udc00'", r"+'\ud800 1'"] {
        assert!(matches!(evaluate(source), Value::Number(n) if n.is_nan()), "{source}");
    }
    assert_eq!(evaluate(r"+'\uFEFF42\u2029'"), Value::Number(42.0));
}

#[test]
fn runtime_string_limit_counts_utf16_payload_bytes() {
    let mut vm = Vm::new(VmConfig { max_string_bytes: 4, ..VmConfig::default() }).unwrap();
    for source in ["'ab'", "'冰山'", "'😀'", r"'\ud800\udfff'"] {
        assert!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).is_ok(), "{source}");
    }
    for source in ["'abc'", "'ab'+'c'", r"'\ud800'+'ab'", r"`${'ab'}c`"] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()), Err(RuntimeError::StringLimit { limit: 4 }), "{source}");
    }
}

#[test]
fn host_strings_and_property_keys_are_lossless_and_exactly_accounted() {
    let units = vec![0, 0xd800, 0x61, 0xdfff, 0xd83d, 0xde00];
    let string = JsString::from_code_units(units.clone());
    assert_eq!(string.as_code_units(), units);
    assert_eq!(string.len(), 6);
    assert!(!string.is_empty());
    assert!(string.to_utf8().is_err());
    assert_eq!(JsString::from("冰😀").to_utf8().unwrap(), "冰😀");
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    let baseline = heap.stats().managed_bytes;
    heap.set(object, "", Value::Undefined).unwrap();
    let record_bytes = heap.stats().managed_bytes - baseline;
    heap.delete(object, "").unwrap();
    heap.set(object, &string, Value::String(string.clone())).unwrap();
    // Two key copies plus one value payload, each six 16-bit units.
    assert_eq!(heap.stats().managed_bytes - baseline, record_bytes + 36);
    assert_eq!(heap.get_own(object, &string).unwrap(), Some(Value::String(string.clone())));
    assert_eq!(heap.own_keys(object).unwrap(), vec![string.clone()]);
    assert_eq!(heap.get(object, "\0�a�😀").unwrap(), Value::Undefined);
    heap.set(object, &string, Value::String("😀".into())).unwrap();
    assert_eq!(heap.stats().managed_bytes - baseline, record_bytes + 24 + 4);
    heap.delete(object, &string).unwrap();
    assert_eq!(heap.stats().managed_bytes, baseline);
}

#[test]
fn surrogate_key_edges_survive_promotion_and_are_released_on_deletion() {
    let mut heap = Heap::new(HeapConfig { nursery_capacity: 1, ..HeapConfig::default() }).unwrap();
    let owner = heap.alloc_array(0, None).unwrap();
    let root = heap.root(owner).unwrap();
    heap.collect_minor();
    let first = heap.alloc_object(None).unwrap();
    let key = JsString::from_code_units(vec![0xd800]);
    heap.set(owner, &key, Value::Object(first)).unwrap();
    heap.collect_minor();
    assert!(heap.contains(first));
    let second = heap.alloc_object(None).unwrap();
    heap.set(owner, JsString::from_code_units(vec![0xd801]), Value::Object(second)).unwrap();
    heap.set(owner, "4294967296", Value::Number(8.0)).unwrap();
    heap.set(owner, "9999999999999999999999999999999", Value::Number(9.0)).unwrap();
    heap.collect_major();
    assert!(heap.contains(first) && heap.contains(second));
    assert_eq!(heap.get(owner, "length").unwrap(), Value::Number(0.0));
    assert_eq!(heap.enumerable_own_keys(owner).unwrap()[0], key);
    heap.delete(owner, &key).unwrap();
    heap.collect_major();
    assert!(!heap.contains(first) && heap.contains(second));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn boxed_string_virtual_properties_have_their_required_storage_invariants() {
    let mut heap = Heap::default();
    let object = heap.alloc_string("😀".into(), None).unwrap();
    let baseline = heap.stats().managed_bytes;
    let high = Value::String(JsString::from_code_units(vec![0xd83d]));
    assert_eq!(heap.get(object, "0").unwrap(), high);
    assert_eq!(heap.set(object, "0", Value::Undefined), Err(HeapError::ReadOnlyProperty));
    assert_eq!(heap.set(object, "length", Value::Number(0.0)), Err(HeapError::ReadOnlyProperty));
    assert!(!heap.delete(object, "0").unwrap());
    assert!(!heap.delete(object, "length").unwrap());
    assert_eq!(heap.stats().managed_bytes, baseline);
    heap.set(object, "3", Value::Bool(true)).unwrap();
    heap.set(object, "01", Value::Null).unwrap();
    assert_eq!(heap.own_keys(object).unwrap(), ["0", "1", "3", "length", "01"]);
    assert_eq!(heap.enumerable_own_keys(object).unwrap(), ["0", "1", "3", "01"]);
    assert!(heap.delete(object, "3").unwrap());
    assert_eq!(heap.get(object, "length").unwrap(), Value::Number(2.0));
    assert!(heap.delete(object, "missing").unwrap());
    // Both exotic kinds share non-configurable length, but only arrays
    // permit writing it; use the same host interface for the comparison.
    let array = heap.alloc_array(2, None).unwrap();
    assert!(!heap.delete(array, "length").unwrap());
    heap.set(array, "length", Value::Number(0.0)).unwrap();
    assert_eq!(heap.get(array, "length").unwrap(), Value::Number(0.0));
    assert!(!HeapError::ReadOnlyProperty.to_string().is_empty());
}
