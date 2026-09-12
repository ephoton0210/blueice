// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sparse-array storage and execution through the real public interfaces.
use blueice_bluejs::{
    compile, parse, Heap, HeapConfig, HeapError, RuntimeError, Value, Vm, VmConfig,
};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn sparse_arrays_distinguish_holes_and_length_from_ordinary_properties() {
    let mut heap = Heap::default();
    let array = heap.alloc_array(3, None).unwrap();
    assert!(heap.is_array(array).unwrap());
    assert_eq!(
        heap.get_own(array, "length").unwrap(),
        Some(Value::Number(3.0))
    );
    assert_eq!(heap.get_own(array, "0").unwrap(), None);
    heap.set(array, "1", Value::Undefined).unwrap();
    heap.set(array, "name", Value::String("array".into()))
        .unwrap();
    assert_eq!(heap.get_own(array, "1").unwrap(), Some(Value::Undefined));
    assert_eq!(heap.own_keys(array).unwrap(), ["1", "length", "name"]);
    assert_eq!(heap.enumerable_own_keys(array).unwrap(), ["1", "name"]);
    assert!(!heap.delete(array, "length").unwrap());
    assert!(heap.delete(array, "1").unwrap());
    assert_eq!(heap.get(array, "length").unwrap(), Value::Number(3.0));
    assert_eq!(heap.get_own(array, "1").unwrap(), None);
    let ordinary = heap.alloc_object(None).unwrap();
    assert!(!heap.is_array(ordinary).unwrap());
    heap.set(ordinary, "length", Value::String("ordinary".into()))
        .unwrap();
    assert_eq!(heap.enumerable_own_keys(ordinary).unwrap(), ["length"]);
    assert!(heap.delete(ordinary, "length").unwrap());
}

#[test]
fn canonical_index_boundaries_grow_length_without_dense_allocation() {
    let mut heap = Heap::default();
    let array = heap.alloc_array(0, None).unwrap();
    let baseline = heap.stats().managed_bytes;
    heap.set(array, "length", Value::Number(u32::MAX as f64))
        .unwrap();
    assert_eq!(heap.stats().managed_bytes, baseline);
    heap.set(array, "length", Value::Number(0.0)).unwrap();
    for key in ["01", "-0", "+1", "1.0", "4294967295"] {
        heap.set(array, key, Value::Null).unwrap();
        assert_eq!(
            heap.get(array, "length").unwrap(),
            Value::Number(0.0),
            "{key}"
        );
    }
    heap.set(array, "4294967294", Value::Number(7.0)).unwrap();
    assert_eq!(
        heap.get(array, "length").unwrap(),
        Value::Number(u32::MAX as f64)
    );
    assert!(heap.stats().managed_bytes < baseline + 2048);
    heap.set(array, "0", Value::Number(1.0)).unwrap();
    assert_eq!(
        heap.own_keys(array).unwrap(),
        [
            "0",
            "4294967294",
            "length",
            "01",
            "-0",
            "+1",
            "1.0",
            "4294967295"
        ]
    );
    heap.set(array, "length", Value::Number(-0.0)).unwrap();
    let Value::Number(length) = heap.get(array, "length").unwrap() else {
        panic!("numeric length")
    };
    assert_eq!(length.to_bits(), 0.0f64.to_bits());
    assert_eq!(
        heap.own_keys(array).unwrap(),
        ["length", "01", "-0", "+1", "1.0", "4294967295"]
    );
}

#[test]
fn invalid_length_and_failed_index_stores_are_atomic() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 1,
        major_threshold_bytes: 1024,
        max_heap_bytes: 2048,
    })
    .unwrap();
    let array = heap.alloc_array(2, None).unwrap();
    heap.root(array).unwrap();
    heap.set(array, "1", Value::Number(9.0)).unwrap();
    let baseline = heap.stats().managed_bytes;
    for value in [
        Value::Number(-1.0),
        Value::Number(1.5),
        Value::Number(f64::NAN),
        Value::Number(f64::INFINITY),
        Value::Number(4294967296.0),
        Value::String("2".into()),
    ] {
        assert_eq!(
            heap.set(array, "length", value),
            Err(HeapError::InvalidArrayLength)
        );
        assert_eq!(heap.get(array, "length").unwrap(), Value::Number(2.0));
        assert_eq!(heap.get(array, "1").unwrap(), Value::Number(9.0));
        assert_eq!(heap.stats().managed_bytes, baseline);
    }
    assert_eq!(
        heap.set(array, "10", Value::String("x".repeat(2048).into())),
        Err(HeapError::HeapLimitExceeded { limit: 2048 })
    );
    assert_eq!(heap.get(array, "length").unwrap(), Value::Number(2.0));
    assert_eq!(heap.own_keys(array).unwrap(), ["1", "length"]);
    assert_eq!(heap.stats().managed_bytes, baseline);
}

#[test]
fn truncation_removes_gc_edges_and_preserves_inherited_and_named_properties() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 1,
        ..HeapConfig::default()
    })
    .unwrap();
    let prototype = heap.alloc_object(None).unwrap();
    heap.set(prototype, "1", Value::Number(8.0)).unwrap();
    let array = heap.alloc_array(2, Some(prototype)).unwrap();
    let root = heap.root(array).unwrap();
    let child = heap.alloc_array(0, None).unwrap();
    heap.set(array, "1", Value::Object(child)).unwrap();
    heap.set(child, "0", Value::Object(array)).unwrap();
    heap.set(array, "name", Value::String("keep".into()))
        .unwrap();
    heap.collect_minor();
    assert!(heap.contains(child));
    let before = heap.stats().managed_bytes;
    heap.set(array, "length", Value::Number(0.0)).unwrap();
    assert!(heap.stats().managed_bytes < before);
    assert_eq!(heap.get(array, "1").unwrap(), Value::Number(8.0));
    assert_eq!(heap.get_own(array, "1").unwrap(), None);
    assert_eq!(
        heap.get(array, "name").unwrap(),
        Value::String("keep".into())
    );
    heap.collect_major();
    assert!(!heap.contains(child));
    assert!(heap.contains(prototype));
    heap.unroot(root).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().managed_bytes, 0);
}

#[test]
fn literals_holes_index_updates_and_length_coercions_execute() {
    for (source, expected) in [
        ("[].length", Value::Number(0.0)),
        ("[,,].length", Value::Number(2.0)),
        ("[1,].length", Value::Number(1.0)),
        ("[1,,].length", Value::Number(2.0)),
        ("[,,][0]", Value::Undefined),
        ("typeof []", Value::String("object".into())),
        (
            "let a=[2,4,6]; let s=0; for(let i=0;i<a.length;i++){s+=a[i];} s",
            Value::Number(12.0),
        ),
        ("let a=[]; a[4]=7; a.length*10+a[4]", Value::Number(57.0)),
        ("let a=[1,2,3]; a.length=1; a[2]", Value::Undefined),
        (
            "let a=[1]; let result=(a.length='3'); result=== '3' && a.length===3",
            Value::Bool(true),
        ),
        ("let a=[1,2]; a.length=null; a.length", Value::Number(0.0)),
        ("let a=[1,2]; a.length=true; a.length", Value::Number(1.0)),
        ("let a=[]; a.length='0x10'; a.length", Value::Number(16.0)),
        (
            "let a=[]; a.length=-0; 1/a.length",
            Value::Number(f64::INFINITY),
        ),
        (
            "let a=[1,2,3]; let n=a.length--; n*10+a.length",
            Value::Number(32.0),
        ),
        ("let a=[]; a.length+='2'; a.length", Value::Number(2.0)),
        (
            "let i=0; let a=[i++,i++,i++]; a[0]*100+a[1]*10+a[2]",
            Value::Number(12.0),
        ),
        (
            "let i=0; let a=[3]; let old=a[i++]++; old*100+a[0]*10+i",
            Value::Number(341.0),
        ),
    ] {
        assert_eq!(evaluate(source).unwrap(), expected, "{source}");
    }
}

#[test]
fn sparse_array_searches_preserve_holes_and_only_visit_present_indices() {
    assert_eq!(
        evaluate(
            "let a=new Array(100000);a[99999]=7;a['01']=9;a.includes(7)&&a.indexOf(7)===99999&&!a.includes(7,100000)&&a.includes(undefined)&&a.indexOf(undefined)===-1&&a.indexOf(9)===-1",
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        evaluate(
            "Array.prototype[50000]=8;let a=new Array(100000);a.includes(8)&&a.indexOf(8)===50000",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn array_from_maps_iterables_lazily_and_closes_on_a_mapper_throw() {
    assert_eq!(
        evaluate(
            "let closed=0;let marker={};let source={};source[Symbol.iterator]=function(){return {next:function(){return {value:1,done:false}},return:function(){closed++;return {done:true}}}};let thrown;try{Array.from(source,function(){throw marker})}catch(error){thrown=error===marker}thrown&&closed===1",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn invalid_runtime_lengths_report_range_errors_and_vm_remains_reusable() {
    let mut vm = Vm::default();
    for source in [
        "let a=[]; a.length=-1",
        "let a=[]; a.length=1.5",
        "let a=[]; a.length=NaN",
        "let a=[]; a.length=Infinity",
        "let a=[]; a.length=4294967296",
        "let a=[]; a.length=undefined",
        "let a=[]; a.length--",
    ] {
        assert!(
            matches!(
                vm.execute(&compile(&parse(source).unwrap()).unwrap()),
                Err(RuntimeError::RangeError(_))
            ),
            "{source}"
        );
        assert_eq!(
            vm.execute(&compile(&parse("[3][0]").unwrap()).unwrap())
                .unwrap(),
            Value::Number(3.0)
        );
    }
    assert!(matches!(
        evaluate("let a=[]; a.length={}"),
        Err(RuntimeError::RangeError(_))
    ));
}

#[test]
fn nested_arrays_survive_vm_safepoints_and_share_an_array_prototype() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1024,
            max_heap_bytes: 8192,
        },
        ..VmConfig::default()
    })
    .unwrap();
    let code = compile(
        &parse(
            "let a=[{x:7},[8],,undefined]; a[4]=a; for(let i=0;i<40;i++){let garbage=[{},[]];} a",
        )
        .unwrap(),
    )
    .unwrap();
    let Value::Object(array) = vm.execute(&code).unwrap() else {
        panic!("expected array")
    };
    assert!(vm.heap().is_array(array).unwrap());
    assert_eq!(vm.heap().get_own(array, "2").unwrap(), None);
    assert_eq!(
        vm.heap().get_own(array, "3").unwrap(),
        Some(Value::Undefined)
    );
    assert_eq!(vm.heap().get(array, "4").unwrap(), Value::Object(array));
    let Value::Object(object) = vm.heap().get(array, "0").unwrap() else {
        panic!("expected object element")
    };
    assert_eq!(vm.heap().get(object, "x").unwrap(), Value::Number(7.0));
    let Value::Object(child) = vm.heap().get(array, "1").unwrap() else {
        panic!("expected child array")
    };
    assert_eq!(vm.heap().get(child, "0").unwrap(), Value::Number(8.0));
    let prototype = vm.heap().prototype(array).unwrap().unwrap();
    assert!(vm.heap().is_array(prototype).unwrap());
    assert_eq!(
        vm.heap().get(prototype, "length").unwrap(),
        Value::Number(0.0)
    );
    assert_eq!(vm.heap().prototype(child).unwrap(), Some(prototype));
    let object_prototype = vm.heap().prototype(prototype).unwrap().unwrap();
    assert!(!vm.heap().is_array(object_prototype).unwrap());
    assert_eq!(vm.heap().prototype(object_prototype).unwrap(), None);
    assert!(vm.heap().stats().minor_collections > 1);
    vm.execute(&compile(&parse("1").unwrap()).unwrap()).unwrap();
    assert!(!vm.heap().contains(array) && !vm.heap().contains(child));
}

#[test]
fn array_entry_points_reject_stale_and_cross_heap_handles() {
    let mut heap = Heap::default();
    let stale = heap.alloc_array(0, None).unwrap();
    heap.collect_major();
    let mut other = Heap::default();
    let foreign = other.alloc_array(0, None).unwrap();
    let array = heap.alloc_array(2, None).unwrap();
    for id in [stale, foreign] {
        assert_eq!(heap.is_array(id), Err(HeapError::InvalidObject(id)));
        assert_eq!(
            heap.enumerable_own_keys(id),
            Err(HeapError::InvalidObject(id))
        );
        assert_eq!(
            heap.alloc_array(0, Some(id)),
            Err(HeapError::InvalidObject(id))
        );
        assert_eq!(
            heap.set(id, "length", Value::Number(1.0)),
            Err(HeapError::InvalidObject(id))
        );
        assert_eq!(
            heap.set(array, "3", Value::Object(id)),
            Err(HeapError::InvalidObject(id))
        );
        assert_eq!(
            heap.set(array, "length", Value::Object(id)),
            Err(HeapError::InvalidObject(id))
        );
    }
    assert_eq!(heap.get(array, "length").unwrap(), Value::Number(2.0));
    assert_eq!(heap.own_keys(array).unwrap(), ["length"]);
}

#[test]
fn minor_gc_does_not_retain_truncated_or_replaced_array_elements() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 8,
        ..HeapConfig::default()
    })
    .unwrap();
    let array = heap.alloc_array(3, None).unwrap();
    heap.root(array).unwrap();
    heap.collect_minor();
    let keep = heap.alloc_object(None).unwrap();
    heap.set(array, "0", Value::Object(keep)).unwrap();
    let truncated = heap.alloc_object(None).unwrap();
    heap.set(array, "1", Value::Object(truncated)).unwrap();
    let replaced = heap.alloc_array(0, None).unwrap();
    heap.set(array, "2", Value::Object(replaced)).unwrap();
    heap.set(array, "2", Value::Null).unwrap();
    heap.set(array, "length", Value::Number(1.0)).unwrap();
    heap.collect_minor();
    assert!(heap.contains(keep));
    assert!(!heap.contains(truncated) && !heap.contains(replaced));
    heap.set(array, "length", Value::Number(3.0)).unwrap();
    assert_eq!(heap.get_own(array, "1").unwrap(), None);
    assert_eq!(heap.get_own(array, "2").unwrap(), None);
    assert_eq!(heap.get(array, "0").unwrap(), Value::Object(keep));
}

#[test]
fn sparse_mutations_match_an_independent_optional_slot_model() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 2,
        major_threshold_bytes: 2048,
        max_heap_bytes: 8192,
    })
    .unwrap();
    let array = heap.alloc_array(0, None).unwrap();
    heap.root(array).unwrap();
    heap.set(array, "tag", Value::String("kept".into()))
        .unwrap();
    let baseline = heap.stats().managed_bytes;
    let mut slots = Vec::<Option<Value>>::new();
    let mut state = 0x937d_671fu32;
    for step in 0..256 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let index = ((state >> 8) % 24) as usize;
        match state % 4 {
            0 => {
                heap.set(array, "length", Value::Number(index as f64))
                    .unwrap();
                slots.resize(index, None);
            }
            1 => {
                assert!(heap.delete(array, index.to_string()).unwrap());
                if let Some(slot) = slots.get_mut(index) {
                    *slot = None;
                }
            }
            _ => {
                let value = if step % 3 == 0 {
                    Value::Undefined
                } else {
                    Value::String(format!("value:{step}").into())
                };
                heap.set(array, index.to_string(), value.clone()).unwrap();
                if slots.len() <= index {
                    slots.resize(index + 1, None);
                }
                slots[index] = Some(value);
            }
        }
        heap.alloc_array(u32::MAX, None).unwrap(); // Allocating garbage stresses root/barrier integration.
        if step % 7 == 0 {
            heap.collect_minor();
        }
        if step % 31 == 0 {
            heap.collect_major();
        }
        assert_eq!(
            heap.get(array, "length").unwrap(),
            Value::Number(slots.len() as f64),
            "step {step}"
        );
        let mut keys: Vec<blueice_bluejs::JsString> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, value)| value.as_ref().map(|_| i.to_string().into()))
            .collect();
        keys.push("length".into());
        keys.push("tag".into());
        assert_eq!(heap.own_keys(array).unwrap(), keys, "step {step}");
        for index in 0..24 {
            assert_eq!(
                heap.get_own(array, index.to_string()).unwrap(),
                slots.get(index).cloned().flatten(),
                "step {step}, index {index}"
            );
        }
    }
    heap.set(array, "length", Value::Number(0.0)).unwrap();
    heap.collect_major();
    assert_eq!(heap.stats().managed_bytes, baseline);
    assert_eq!(
        heap.get(array, "tag").unwrap(),
        Value::String("kept".into())
    );
}

#[test]
fn new_array_encoding_and_runtime_error_boundaries_are_observable() {
    use blueice_bluejs::Opcode;
    let code = compile(&parse("[,,3,]").unwrap()).unwrap();
    let instruction = code
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::NewArray)
        .unwrap();
    assert_eq!(instruction.opcode.width(), 5);
    assert_eq!(instruction.operand, Some(3));
    assert_eq!(
        &code.bytes()[instruction.offset + 1..instruction.offset + 5],
        &3u32.to_le_bytes()
    );
    assert_eq!(
        code.instructions()
            .filter(|instruction| instruction.opcode == Opcode::DefineData)
            .count(),
        1
    );
    assert_eq!(evaluate("let [a]=[1];a"), Ok(Value::Number(1.0)));
    assert!(matches!(
        evaluate("[].push(1)"),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(evaluate("new Array(2).length"), Ok(Value::Number(2.0)));
    let error = evaluate("let a=[];a.length=-1").unwrap_err();
    assert!(error.to_string().starts_with("RangeError:"));
    assert!(std::error::Error::source(&error).is_none());
    assert!(HeapError::InvalidArrayLength
        .to_string()
        .contains("array length"));

    let mut probe = Heap::default();
    probe.alloc_object(None).unwrap();
    let one_object = probe.stats().managed_bytes;
    assert!(matches!(
        Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: 1,
                major_threshold_bytes: one_object,
                max_heap_bytes: one_object
            },
            ..VmConfig::default()
        }),
        Err(HeapError::HeapLimitExceeded { .. })
    ));
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1024,
            max_heap_bytes: 2048,
        },
        ..VmConfig::default()
    })
    .unwrap();
    let baseline = vm.heap().stats().managed_bytes;
    let code = compile(&parse("let a=[];while(true){a[a.length]={};}").unwrap()).unwrap();
    assert!(matches!(
        vm.execute(&code),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(vm.heap().stats().managed_bytes, baseline);
    assert_eq!(
        vm.execute(&compile(&parse("[7][0]").unwrap()).unwrap())
            .unwrap(),
        Value::Number(7.0)
    );
}
