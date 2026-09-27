// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn sparse_array_scans_propagate_stale_object_and_instruction_errors() {
    let mut vm = Vm::default();
    let objects: Vec<_> = (0..3)
        .map(|_| vm.heap.alloc_object(None).unwrap())
        .collect();
    let mut scans: Vec<_> = objects
        .iter()
        .map(|&object| vm.index_scan(object, 65_536).unwrap())
        .collect();
    vm.heap.collect_major();
    for &object in &objects {
        assert!(!vm.heap.contains(object));
    }
    let marker = vm.heap.alloc_object(None).unwrap();
    vm.heap.set(marker, "0", Value::Number(1.0)).unwrap();

    fn stale<T>(result: Result<T, RuntimeError>, object: ObjectId) {
        assert!(
            matches!(result, Err(RuntimeError::Heap(HeapError::InvalidObject(id))) if id == object),
            "the scan must report its collected receiver"
        );
    }
    stale(vm.scan_next(&mut scans[0], objects[0], 0), objects[0]);
    stale(
        vm.array_next_present(&mut scans[1], objects[1], 0),
        objects[1],
    );
    stale(
        vm.array_previous_present(&mut scans[2], objects[2], 1),
        objects[2],
    );
    stale(vm.index_scan(objects[0], 65_536), objects[0]);

    let object = vm.heap.alloc_object(None).unwrap();
    let mut forward = vm.index_scan(object, 1).unwrap();
    let mut backward = vm.index_scan(object, 1).unwrap();
    vm.remaining_instructions = 0;
    assert!(matches!(
        vm.array_next_present(&mut forward, object, 0),
        Err(RuntimeError::InstructionLimit)
    ));
    assert!(matches!(
        vm.array_previous_present(&mut backward, object, 1),
        Err(RuntimeError::InstructionLimit)
    ));
}

#[test]
fn sparse_array_length_rejects_a_symbol() {
    let code = crate::compile(
        &crate::parse("Array.prototype.some.call({ length: Symbol() }, () => false)").unwrap(),
    )
    .unwrap();
    assert!(matches!(
        Vm::default().execute(&code),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn array_concat_keeps_non_array_values_as_elements() {
    let mut vm = Vm::default();
    vm.remaining_instructions = vm.config.instruction_budget;
    let receiver = vm.array_from(vec![Value::Number(1.0)]).unwrap();
    let result = vm.array_concat(&receiver, &[Value::Number(2.0)]).unwrap();
    let result = result.object_id().unwrap();
    assert_eq!(vm.heap.get(result, "0"), Ok(Value::Number(1.0)));
    assert_eq!(vm.heap.get(result, "1"), Ok(Value::Number(2.0)));
}

#[test]
fn math_extrema_replace_the_running_result_for_later_arguments() {
    let mut vm = Vm::default();
    assert_eq!(
        vm.math_method(MathMethod::Max, &[Value::Number(1.0), Value::Number(2.0)]),
        Ok(Value::Number(2.0))
    );
    assert_eq!(
        vm.math_method(MathMethod::Min, &[Value::Number(2.0), Value::Number(1.0)]),
        Ok(Value::Number(1.0))
    );
}

#[test]
fn generator_prototype_releases_its_temporary_root_after_an_allocation_failure() {
    let prerequisites_ready = |max_heap_bytes| {
        let config = VmConfig {
            heap: HeapConfig {
                major_threshold_bytes: max_heap_bytes,
                max_heap_bytes,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        };
        let Ok(mut vm) = Vm::new(config) else {
            return false;
        };
        let _ = vm.generator_prototype();
        vm.string_intrinsics.is_some() && vm.iterator_base.is_some()
    };
    let mut lower = 4_096;
    let mut upper = 4 * 1024 * 1024;
    assert!(
        prerequisites_ready(upper),
        "the bounded search must initialize the prerequisite prototypes"
    );
    while lower + 1 < upper {
        let middle = lower + (upper - lower) / 2;
        if prerequisites_ready(middle) {
            upper = middle;
        } else {
            lower = middle;
        }
    }
    let mut found = false;
    for max_heap_bytes in upper..=upper + 4_096 {
        let config = VmConfig {
            heap: HeapConfig {
                major_threshold_bytes: max_heap_bytes,
                max_heap_bytes,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        };
        let Ok(mut vm) = Vm::new(config) else {
            continue;
        };
        let result = vm.generator_prototype();
        let candidate = vm.string_intrinsics.is_some()
            && vm.iterator_base.is_some()
            && matches!(
                result,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
            );
        if candidate {
            found = true;
        }
        if found {
            break;
        }
    }
    assert!(
        found,
        "a bounded heap must exercise generator prototype cleanup"
    );
}
