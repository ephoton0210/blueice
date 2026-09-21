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

/// Run with a tiny nursery so a collection happens on nearly every allocation.
fn evaluate_with_nursery(source: &str, nursery_capacity: usize) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = nursery_capacity;
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
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
fn mutating_and_locale_array_methods_preserve_generic_property_semantics() {
    assert_eq!(
        evaluate("let values=[1,2,3];let popped=values.pop();popped===3&&values.length===2",)
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate("let values=[1,2,3];let shifted=values.shift();shifted===1&&values.length===2")
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate("let values=[1,2,3];values.unshift(7,8)===5&&values[0]===7&&values[4]===3")
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate("let values=[1,2,3];values.reverse()===values&&values.join(',')==='3,2,1'")
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate("let sparse=[,2];sparse.reverse();sparse[0]===2&&!(1 in sparse)").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate("[1,null,{toLocaleString:function(){return 'custom'}}].toLocaleString()").unwrap(),
        Value::String("1,,custom".into())
    );
    assert_eq!(
        evaluate(
            "(3.5).toFixed(1)==='3.5'&&(12).toExponential(1)==='1.2e+1'&&(12).toPrecision(3)==='12.0'&&(12).toLocaleString()==='12'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn array_fill_is_generic_and_observes_relative_bounds() {
    assert_eq!(
        evaluate(
            "let values=[0,1,2,3];let result=values.fill('x',-3,-1);let all=values.fill('z',-Infinity,Infinity);let generic={length:4};Array.prototype.fill.call(generic,7,1,-1);let descriptor=Object.getOwnPropertyDescriptor(Array.prototype,'fill');result===values&&all===values&&values.join(',')==='z,z,z,z'&&generic[0]===undefined&&generic[1]===7&&generic[2]===7&&generic[3]===undefined&&descriptor.writable&&descriptor.configurable&&!descriptor.enumerable&&Array.prototype.fill.length===1",
        )
        .unwrap(),
        Value::Bool(true)
    );
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
fn callback_and_reverse_search_array_methods_preserve_holes_order_and_length() {
    assert_eq!(
        evaluate(
            "typeof [].map==='function'&&typeof [].every==='function'&&typeof [].some==='function'&&typeof [].reduceRight==='function'&&typeof [].lastIndexOf==='function'&&Object.prototype.toString.call(JSON)==='[object JSON]'",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let source=[,2,,4];let calls=[];let mapped=source.map(function(value,index,array){calls.push(index);return value*2});let every=source.every(function(value){return value%2===0});let some=source.some(function(value){return value===4});let reduced=source.reduceRight(function(left,value){return left+value},'');let found=[1,2,1,2].lastIndexOf(1,-2);let ordered=false;let arrayLike={};Object.defineProperty(arrayLike,'length',{get:function(){ordered=true;return 0}});try{Array.prototype.some.call(arrayLike,null)}catch(error){}mapped.length===4&&mapped[0]===undefined&&mapped[1]===4&&mapped[2]===undefined&&mapped[3]===8&&calls.join(',')==='1,3'&&every&&some&&reduced==='42'&&found===2&&[1,2,1].lastIndexOf(1,undefined)===0&&ordered",
        )
        .unwrap(),
        Value::Bool(true)
    );
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
fn map_filter_and_array_of_use_create_data_property_and_species_construction() {
    assert_eq!(
        evaluate(
            "let calls=[];function C(length){calls.push(length);return {}}let source=[1,2,3];source.constructor={};source.constructor[Symbol.species]=C;let mapped=source.map(function(value){return value*2});let filtered=source.filter(function(value){return value>1});let of=Array.of.call(C,'a','b');calls.join(',')==='3,0,2'&&mapped[0]===2&&mapped[2]===6&&filtered[0]===2&&filtered[1]===3&&of[0]==='a'&&of[1]==='b'&&of.length===2&&Array[Symbol.species]===Array",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn at_and_iterator_methods_use_relative_indices_and_share_values_identity() {
    for source in [
        "let array=[10,,30];array.at(-1)===30&&array.at(-4)===undefined&&array.at(NaN)===10&&array.at(1)===undefined",
        "Array.prototype[Symbol.iterator]===Array.prototype.values&&Array.prototype.at.length===1",
        "let entries=[10,,30].entries();entries.next().value[0]===0&&entries.next().value[1]===undefined",
        "[10,,30].values().next().value===10&&[10,,30].keys().next().value===0",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn find_methods_visit_holes_and_observe_forward_or_reverse_order() {
    assert_eq!(
        evaluate(
            "let seen=[];let array=[,2,3,2];let found=array.find(function(value,index){seen.push([value,index].join(':'));return value===2});let first=array.findIndex(function(value){return value===2});let last=array.findLast(function(value){return value===2});let lastIndex=array.findLastIndex(function(value){return value===2});found===2&&first===1&&last===2&&lastIndex===3&&seen[0]===':0'&&seen[1]==='2:1'&&[].find(function(){return true})===undefined&&[].findIndex(function(){return true})===-1",
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
    assert_eq!(evaluate("[].push(1)"), Ok(Value::Number(1.0)));
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

#[test]
fn array_slice_with_an_end_before_the_start_returns_an_empty_array() {
    let source = "let a=[1,2,3,4];let r=a.slice(3,1);let s=a.slice(9007199254740992,0);let t=a.slice(-1,-3);let u=a.slice(Infinity,-Infinity);r.length===0&&s.length===0&&t.length===0&&u.length===0&&a.slice(1,3).join()===\"2,3\"";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

fn assert_all_true(cases: &[&str]) {
    for source in cases {
        assert_eq!(
            evaluate(source).unwrap_or_else(|error| panic!("{source}: {error}")),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn array_copy_within_moves_a_range_with_generic_property_semantics() {
    assert_all_true(&[
        // Overlapping ranges copy in the direction that preserves the source;
        // negative, undefined, NaN and infinite indexes clamp like slice.
        "[1,2,3,4,5].copyWithin(0,3).join()==='4,5,3,4,5'&&[1,2,3,4,5].copyWithin(1,0,3).join()==='1,1,2,3,5'&&[1,2,3,4,5].copyWithin(-2,-4,-1).join()==='1,2,3,2,3'&&[1,2,3,4,5].copyWithin(0,1,2).join()==='2,2,3,4,5'&&[1,2,3].copyWithin(1).join()==='1,1,2'&&[1,2,3].copyWithin(5,0).join()==='1,2,3'&&[1,2,3].copyWithin(0,5).join()==='1,2,3'&&[1,2,3].copyWithin(0,1,undefined).join()==='2,3,3'&&[1,2,3,4].copyWithin(NaN,'2').join()==='3,4,3,4'&&[1,2,3].copyWithin(-Infinity,Infinity).join()==='1,2,3'",
        // A hole in the source deletes the destination instead of copying.
        "let a=[1,,3];let r=a.copyWithin(0,1);r===a&&!(0 in a)&&a[1]===3&&a[2]===3&&a.length===3",
        // Any array-like receiver works, and a missing source key deletes.
        "let o={length:5,0:'a',1:'b',3:'d'};let r=Array.prototype.copyWithin.call(o,1,0,3);r===o&&o[0]==='a'&&o[1]==='a'&&o[2]==='b'&&!(3 in o)&&o[4]===undefined&&Array.prototype.copyWithin.call(true,0,1) instanceof Boolean&&Array.prototype.copyWithin.length===2&&Array.prototype.copyWithin.name==='copyWithin'",
        // target, start and end are coerced in that order.
        "let log=[];let a=[0,1,2,3];a.copyWithin({valueOf(){log.push('target');return 0}},{valueOf(){log.push('start');return 2}},{valueOf(){log.push('end');return 4}});log.join()==='target,start,end'&&a.join()==='2,3,2,3'",
        // The observable step order through a Proxy: HasProperty, Get and
        // Set per element, and DeletePropertyOrThrow for a hole.
        "let log=[];let target=[1,2,3];let p=new Proxy(target,{has(t,k){log.push('has:'+String(k));return k in t},get(t,k,r){log.push('get:'+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){log.push('set:'+String(k));return Reflect.set(t,k,v,r)},deleteProperty(t,k){log.push('delete:'+String(k));return delete t[k]}});Array.prototype.copyWithin.call(p,0,1);log.join()==='get:length,has:1,get:1,set:0,has:2,get:2,set:1'&&target.join()==='2,3,3'",
        "let log=[];let target=[1,,3];let p=new Proxy(target,{deleteProperty(t,k){log.push('delete:'+String(k));return delete t[k]}});Array.prototype.copyWithin.call(p,0,1);log.join()==='delete:0'&&!(0 in target)&&target[1]===3",
        // Failing Set/coercion/ToObject surface as TypeErrors.
        "let caught=[];try{Object.freeze([1,2,3]).copyWithin(0,1)}catch(e){caught.push(e instanceof TypeError)}try{Array.prototype.copyWithin.call(null,0,1)}catch(e){caught.push(e instanceof TypeError)}try{[1].copyWithin(Symbol(),0)}catch(e){caught.push(e instanceof TypeError)}let frozen=Object.freeze({length:2,0:1,1:2});try{Array.prototype.copyWithin.call(frozen,0,1)}catch(e){caught.push(e instanceof TypeError)}caught.join()==='true,true,true,true'",
    ]);
}

#[test]
fn array_prototype_generic_methods_track_resizable_typed_array_lengths() {
    assert_all_true(&[
        // A fixed-length view that shrank out of bounds has length 0, so the
        // generic algorithm is a no-op instead of throwing.
        "let rab=new ArrayBuffer(4,{maxByteLength:8});let fixed=new Uint8Array(rab,0,4);let tracking=new Uint8Array(rab);tracking.set([0,1,2,3]);Array.prototype.copyWithin.call(fixed,0,2);let first=tracking.join()==='2,3,2,3';rab.resize(3);let untouched=Array.prototype.copyWithin.call(fixed,0,1)===fixed&&fixed.length===0;Array.prototype.copyWithin.call(tracking,0,1);let second=tracking.join()==='3,2,2';rab.resize(1);Array.prototype.copyWithin.call(tracking,0,0,1);rab.resize(0);Array.prototype.copyWithin.call(tracking,0,0,1);rab.resize(6);tracking.set([0,1,2,3,4,5]);Array.prototype.copyWithin.call(tracking,0,2);first&&untouched&&second&&tracking.join()==='2,3,4,5,4,5'",
        // Array.prototype.toLocaleString reads `length` generically (0 when
        // out of bounds); only %TypedArray%.prototype.toLocaleString throws.
        "let rab=new ArrayBuffer(4,{maxByteLength:8});let fixed=new Uint8Array(rab,0,4);let tracking=new Uint8Array(rab);tracking.set([0,2,4,6]);let generic=Array.prototype.toLocaleString;let full=generic.call(fixed)===['0','2','4','6'].join(',');rab.resize(3);let oob=generic.call(fixed)==='';let shrunk=generic.call(tracking)==='0,2,4';let own;try{fixed.toLocaleString();own='no throw'}catch(e){own=e instanceof TypeError}full&&oob&&shrunk&&own===true",
    ]);
}

#[test]
fn array_flat_flattens_by_depth_and_skips_holes() {
    assert_all_true(&[
        "JSON.stringify([1,[2,[3,[4]]]].flat())==='[1,2,[3,[4]]]'&&JSON.stringify([1,[2,[3,[4]]]].flat(2))==='[1,2,3,[4]]'&&JSON.stringify([1,[2,[3,[4]]]].flat(Infinity))==='[1,2,3,4]'&&JSON.stringify([1,[2]].flat(0))==='[1,[2]]'&&JSON.stringify([1,[2]].flat(-5))==='[1,[2]]'&&JSON.stringify([1,[2]].flat(undefined))==='[1,2]'&&JSON.stringify([1,[2]].flat('x'))==='[1,[2]]'&&JSON.stringify([1,[2]].flat(null))==='[1,[2]]'&&JSON.stringify([1,[2]].flat(1.9))==='[1,2]'&&Array.prototype.flat.length===0&&Array.prototype.flat.name==='flat'",
        // Holes are skipped at every level; array-like values are not
        // flattened, but an array-like receiver is read generically.
        "let a=[1,,[2,,3]];let r=a.flat();JSON.stringify(r)==='[1,2,3]'&&r.length===3&&JSON.stringify(Array.prototype.flat.call({length:3,0:1,2:[4]}))==='[1,4]'&&JSON.stringify([{length:1,0:'x'}].flat())==='[{\"0\":\"x\",\"length\":1}]'",
        // IsArray sees through a Proxy, and the result comes from
        // ArraySpeciesCreate(receiver, 0).
        "let p=new Proxy([1,2],{});let flattened=[p,[3]].flat();let sp=[];let source=[1,[2]];source.constructor={};source.constructor[Symbol.species]=function(n){sp.push(n);return [];};let viaSpecies=source.flat();flattened.join()==='1,2,3'&&sp.join()==='0'&&viaSpecies.join()==='1,2'&&Array.isArray(viaSpecies)",
        // `depth` is coerced (and may throw) before the species lookup.
        "let log=[];let a=[1,[2]];Object.defineProperty(a,'constructor',{get(){log.push('constructor');return undefined}});let r=a.flat({valueOf(){log.push('depth');return 1}});let boom={};let caught;try{[1].flat({valueOf(){throw boom}})}catch(e){caught=e}log.join()==='depth,constructor'&&r.join()==='1,2'&&caught===boom",
        // Unbounded nesting (here a cycle) is a catchable RangeError rather
        // than exhausting the host stack or running forever.
        "let a=[1];a.push(a);let threw=false;try{a.flat(Infinity)}catch(e){threw=e instanceof RangeError}threw",
    ]);
}

#[test]
fn array_flat_map_maps_then_flattens_exactly_one_level() {
    assert_all_true(&[
        "let log=[];let source=[1,2,3];let result=source.flatMap(function(value,index,array){log.push([this.tag,value,index,array===source].join());return value%2?[value,value*10]:value;},{tag:'T'});result.join()==='1,10,2,3,30'&&log.join(';')==='T,1,0,true;T,2,1,true;T,3,2,true'&&JSON.stringify([1].flatMap(function(v){return [[v]]}))==='[[1]]'&&Array.prototype.flatMap.length===1&&Array.prototype.flatMap.name==='flatMap'&&JSON.stringify([1,,3].flatMap(function(v){return v}))==='[1,3]'",
        // The receiver's length is read before the callback is validated.
        "let caught=[];try{[].flatMap()}catch(e){caught.push(e instanceof TypeError)}try{[].flatMap({})}catch(e){caught.push(e instanceof TypeError)}try{Array.prototype.flatMap.call(null,function(){})}catch(e){caught.push(e instanceof TypeError)}let order=[];try{Array.prototype.flatMap.call({get length(){order.push('length');return 1}},1)}catch(e){order.push(e instanceof TypeError)}caught.join()==='true,true,true'&&order.join()==='length,true'",
        // A non-callable mapper is rejected before the species lookup.
        "let log=[];let a=[1];Object.defineProperty(a,'constructor',{get(){log.push('constructor');return undefined}});try{a.flatMap(1)}catch(e){log.push(e instanceof TypeError)}log.join()==='true'",
        // A throwing mapper propagates unchanged and leaves the VM usable.
        "let boom=new Error('boom');let caught;try{[1,2].flatMap(function(v){if(v===2)throw boom;return [v]})}catch(e){caught=e}caught===boom&&[3].flatMap(function(v){return [v,v]}).join()==='3,3'",
        // A TypedArray or array-like receiver yields a plain Array without
        // ever consulting `constructor`; nested array-likes stay unflattened.
        "let same=function(e){return e};let ta=new Int32Array([1,0,42]);Object.defineProperty(ta,'constructor',{get(){throw 'no constructor lookup'}});let fromTyped=[].flatMap.call(ta,same);let obj=new Int32Array(2);let nested=[{length:1,0:'a'},obj].flatMap(same);fromTyped.join()==='1,0,42'&&Object.getPrototypeOf(fromTyped)===Array.prototype&&!(fromTyped instanceof Int32Array)&&nested.length===2&&nested[1]===obj&&Array.prototype.flatMap.call({length:2,0:1,1:[2]},same).join()==='1,2'",
    ]);
}

#[test]
fn copy_within_flat_and_flat_map_root_their_intermediate_objects_under_gc_stress() {
    // A one-object nursery makes every allocation a collection point, so an
    // element, mapped value or frame source that was only held in a Rust
    // local would be reclaimed mid-algorithm.
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    let source = "let nested=[{a:1},[{b:2},[{c:3}]]];let flat=nested.flat(Infinity);let mapped=nested.flatMap(function(x){return [{wrapped:x},{other:[x]}]});let moved=[{x:0},{x:1},{x:2},{x:3},{x:4}];moved.copyWithin(1,0,4);flat.length===3&&flat[2].c===3&&mapped.length===4&&mapped[0].wrapped===nested[0]&&mapped[3].other[0]===nested[1]&&moved[4].x===3&&moved[1]===moved[0]&&moved[0].x===0";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
    assert!(vm.heap().stats().minor_collections > 10);
}

#[test]
fn array_push_on_a_non_array_receiver_follows_the_generic_algorithm() {
    assert_all_true(&[
        // A missing or non-numeric `length` goes through ToLength; the
        // result and the written-back `length` are the new length.
        "let o={};let r=Array.prototype.push.call(o,'a','b');r===2&&o[0]==='a'&&o[1]==='b'&&o.length===2",
        "let o={length:'2'};let r=Array.prototype.push.call(o,'x');r===3&&o[2]==='x'&&o.length===3",
        "let o={length:-5};let r=Array.prototype.push.call(o,'x');r===1&&o[0]==='x'&&o.length===1",
        // With no arguments `length` is still written back, as ToLength(length).
        "let o={length:'1.9'};let r=Array.prototype.push.call(o);r===1&&o.length===1",
        // A getter-only `length` makes the final strict Set throw, after the
        // element itself was stored.
        "let o={get length(){return 1}};let caught;try{Array.prototype.push.call(o,'x')}catch(e){caught=e instanceof TypeError}caught===true&&o[1]==='x'",
        // Growing past 2**53-1 throws before anything is written.
        "let o={length:9007199254740991};let caught;try{Array.prototype.push.call(o,'x')}catch(e){caught=e instanceof TypeError}caught===true&&!('9007199254740991' in o)&&o.length===9007199254740991",
        // Genuine arrays keep working (including holes and appended values).
        "let a=[1,,3];let r=a.push(4,5);r===5&&a.length===5&&!(1 in a)&&a[3]===4&&a[4]===5",
    ]);
}

#[test]
fn array_push_on_a_typed_array_throws_a_type_error_instead_of_crashing() {
    assert_all_true(&[
        // `length` is a getter-only accessor on %TypedArray%.prototype, so the
        // final Set(O, "length", len, true) fails with a TypeError.
        "let ta=new Uint8Array(2);let caught;try{Array.prototype.push.call(ta,1)}catch(e){caught=e instanceof TypeError}caught===true&&ta.length===2&&ta[0]===0&&ta[1]===0",
        "let ta=new Uint8Array(2);let caught;try{Array.prototype.push.call(ta)}catch(e){caught=e instanceof TypeError}caught===true",
    ]);
}

#[test]
fn array_push_on_a_frozen_or_length_locked_array_throws_a_type_error() {
    assert_all_true(&[
        // The strict Set of `length` (or of the new index) fails, so push
        // throws instead of silently dropping the write.
        "let a=[1];Object.defineProperty(a,'length',{writable:false});let caught;try{a.push(2)}catch(e){caught=e instanceof TypeError}caught===true&&a.length===1&&!(1 in a)",
        "let a=Object.freeze([1]);let caught;try{a.push(2)}catch(e){caught=e instanceof TypeError}caught===true&&a.length===1&&!(1 in a)",
        // Even with no arguments the final Set of `length` must fail.
        "let a=Object.freeze([1]);let caught;try{a.push()}catch(e){caught=e instanceof TypeError}caught===true",
        "let a=[];Object.defineProperty(a,'length',{writable:false});let caught;try{a.push()}catch(e){caught=e instanceof TypeError}caught===true",
        // A sealed (non-extensible) array cannot grow either.
        "let a=Object.preventExtensions([1]);let caught;try{a.push(2)}catch(e){caught=e instanceof TypeError}caught===true&&a.length===1",
    ]);
}

#[test]
fn array_from_keeps_mapped_values_alive_across_collections() {
    // Every mapper result is only reachable from Array.from's own state while
    // later mapper calls allocate; a collection in between must not free it.
    let source = "let a=Array.from({length:300},(_,i)=>({i}));\
                  a.length===300&&a[0].i===0&&a[299].i===299";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
    assert_eq!(evaluate_with_nursery(source, 1).unwrap(), Value::Bool(true));
}

#[test]
fn push_on_a_real_array_honours_inherited_index_setters() {
    // The first element store reaches an inherited setter; whatever it does
    // to the array must be observed by the strict Set of "length".
    for freeze in [
        "Object.freeze(array)",
        "Object.defineProperty(array,'length',{writable:false})",
    ] {
        let source = format!(
            "var array=[];var calls=0;\
             Object.defineProperty(Array.prototype,'0',{{set(_v){{{freeze};calls++;}}}});\
             var threw=false;try{{array.push(1)}}catch(e){{threw=e instanceof TypeError}}\
             threw&&!array.hasOwnProperty(0)&&array.length===0&&calls===1"
        );
        assert_eq!(evaluate(&source).unwrap(), Value::Bool(true), "{freeze}");
    }
}

#[test]
fn push_on_a_real_array_runs_an_inherited_setter_instead_of_defining() {
    let source = "var seen=[];\
        Object.defineProperty(Array.prototype,'1',{set(v){seen.push(v)},configurable:true});\
        var a=[7];var n=a.push(8,9);\
        n===3&&a.length===3&&!a.hasOwnProperty(1)&&a[2]===9&&seen.length===1&&seen[0]===8";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn push_on_a_real_array_rejects_an_inherited_read_only_element() {
    let source = "Object.defineProperty(Object.prototype,'0',{value:1,writable:false});\
        var a=[];var threw=false;try{a.push(2)}catch(e){threw=e instanceof TypeError}\
        threw&&!a.hasOwnProperty(0)";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn push_on_a_real_array_with_a_proxy_prototype_uses_its_set_trap() {
    let source = "var log=[];\
        var a=[];Object.setPrototypeOf(a,new Proxy(Array.prototype,{\
          set(t,k,v,r){log.push(k);return Reflect.set(t,k,v,r)}}));\
        a.push(5);a[0]===5&&a.length===1&&log.join()==='0'";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn push_keeps_working_and_stays_correct_for_plain_arrays() {
    let source = "var a=[1];var n=a.push(2,3);n===3&&a.join()==='1,2,3'&&a.push()===3";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn array_from_keeps_a_thrown_mapper_error_alive_while_the_iterator_closes() {
    // The mapper's Error is reachable only from Array.from's pending
    // completion while the iterator's return() runs user code.
    let source = "var closed=0;var items={};\
        items[Symbol.iterator]=function(){return{\
          return:function(){closed+=1},\
          next:function(){return{done:false}}}};\
        var caught;try{Array.from(items,function(){throw new Error('boom')})}catch(e){caught=e}\
        closed===1&&caught instanceof Error&&caught.message==='boom'";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
    assert_eq!(evaluate_with_nursery(source, 1).unwrap(), Value::Bool(true));
}

#[test]
fn sort_keeps_collected_values_alive_when_the_comparator_empties_the_array() {
    // After the first comparison the receiver no longer holds any element;
    // the values being sorted are reachable only from sort's own state.
    let source = "var a=[];for(var i=0;i<20;i++)a.push({i});\
        var n=0;a.sort(function(x,y){if(n++===0){a.length=0}var junk=[{},{},{},{}];return x.i-y.i});\
        a.length===20&&a.every((v,k)=>v.i===k)";
    assert_eq!(evaluate_with_nursery(source, 1).unwrap(), Value::Bool(true));
}
