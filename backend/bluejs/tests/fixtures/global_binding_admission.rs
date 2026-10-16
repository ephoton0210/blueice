// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny realm-global records check admission and persistent native ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn eval_code(source: &str) -> Bytecode {
    crate::compiler::compile_eval(
        &crate::parse(source).unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn global_binding_admission_accounts_for_persistent_table_and_name_capacity() {
    let mut vm = bounded_vm();
    let code = compile("var retained;");
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.create_global_binding(global, &code.bindings[0], false, true)
        .unwrap();
    let bytes = host_scheduling::map_slots(&vm.global_bindings)
        + vm.global_bindings
            .keys()
            .map(String::capacity)
            .sum::<usize>();
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes - before,
        bytes
    );
}

#[cfg_attr(test, test)]
fn global_binding_admission_refuses_script_and_eval_before_cells_or_properties_publish() {
    for eval in [false, true] {
        let mut vm = bounded_vm();
        let code = if eval {
            eval_code("var first=1,second=2;")
        } else {
            compile("var first=1,second=2;")
        };
        vm.prepare_root_execution(&code, false).unwrap();
        let global = vm.global("globalThis").unwrap().object_id().unwrap();
        let before = vm.heap.stats();
        let _peer = leave_room(&mut vm, 0);
        let result = if eval {
            vm.prepare_eval_global_var_declarations(&code)
        } else {
            vm.prepare_global_declarations(&code)
        };
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert!(vm.global_bindings.is_empty() && vm.script_global_slots.is_empty());
        assert_eq!(vm.heap.get_own(global, "first").unwrap(), None);
        assert_eq!(vm.heap.stats(), before);
    }
}

#[cfg_attr(test, test)]
fn global_binding_admission_recreation_refuses_before_restoring_the_deleted_global() {
    let mut vm = bounded_vm();
    let code = eval_code("var retained=1;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_eval_global_var_declarations(&code).unwrap();
    vm.enter_scope(&code, 0).unwrap();
    let slot = code
        .bindings
        .iter()
        .position(|binding| binding.name == "retained")
        .unwrap();
    let cell = vm.cells[&slot];
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    assert!(vm.delete_eval_var(slot).unwrap());
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.assign_binding_slot(&code, slot, Value::Number(42.0), true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.get_own(cell, "value").unwrap(), None);
    assert_eq!(vm.heap.get_own(global, "retained").unwrap(), None);
    assert_eq!(vm.heap.stats(), before);
}

fn declaration_bytes(vm: &Vm, names: &[&str]) -> usize {
    let globals = SchedulingMapCapacity::for_additional(
        &vm.global_bindings,
        names.len(),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let aliases = SchedulingMapCapacity::for_additional(
        &vm.script_global_slots,
        names.len(),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    names.len() * std::mem::size_of::<GlobalRecord>()
        + names.iter().map(|name| 2 * name.len()).sum::<usize>()
        + globals.staging_bytes()
        + aliases.staging_bytes()
}

#[cfg_attr(test, test)]
fn global_binding_admission_exact_budget_and_one_byte_refusal() {
    for eval in [false, true] {
        for spare in [false, true] {
            for accepts in [false, true] {
                let mut vm = bounded_vm();
                let code = if eval {
                    eval_code("var first=1,second=2;")
                } else {
                    compile("let first=1;var second=2;")
                };
                vm.prepare_root_execution(&code, false).unwrap();
                vm.global("globalThis").unwrap();
                if spare {
                    vm.global_bindings.reserve(7);
                    vm.script_global_slots.reserve(7);
                }
                let bytes = declaration_bytes(&vm, &["first", "second"]);
                let before = vm.heap.stats();
                let capacities = (
                    vm.global_bindings.capacity(),
                    vm.script_global_slots.capacity(),
                );
                let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
                let result = if eval {
                    vm.prepare_eval_global_var_declarations(&code)
                } else {
                    vm.prepare_global_declarations(&code)
                };
                if accepts {
                    result.unwrap();
                    assert_eq!(vm.global_bindings.len(), 2);
                    assert_eq!(vm.script_global_slots.len(), 2);
                    assert_eq!(
                        vm.global_binding_value("first").unwrap(),
                        if eval { Some(Value::Undefined) } else { None }
                    );
                } else {
                    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
                    assert!(vm.global_bindings.is_empty() && vm.script_global_slots.is_empty());
                    assert_eq!(vm.heap.stats(), before);
                    assert_eq!(
                        (
                            vm.global_bindings.capacity(),
                            vm.script_global_slots.capacity()
                        ),
                        capacities
                    );
                }
                assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
                assert_eq!(vm.displaced_execution_bytes.get(), 0);
            }
        }
    }
}

#[cfg_attr(test, test)]
fn global_binding_admission_existing_aliases_and_accessors_need_no_copy() {
    let mut vm = bounded_vm();
    let code = compile("var retained;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    let pointer = vm.script_global_slots[&0].as_ptr();
    let cell = vm.global_bindings["retained"].cell;
    let _peer = leave_room(&mut vm, 0);
    vm.prepare_global_declarations(&code).unwrap();
    assert_eq!(vm.script_global_slots[&0].as_ptr(), pointer);
    assert_eq!(vm.global_bindings["retained"].cell, cell);

    let mut vm = bounded_vm();
    vm.execute_script(&compile(
        "Object.defineProperty(globalThis,'watched',{get(){throw 7;},configurable:true});",
    ))
    .unwrap();
    let code = compile("var watched;");
    vm.prepare_root_execution(&code, false).unwrap();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let descriptor = vm
        .heap
        .get_own_property_descriptor(global, "watched")
        .unwrap()
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    vm.prepare_global_declarations(&code).unwrap();
    assert!(vm.global_bindings.is_empty() && vm.script_global_slots.is_empty());
    let after = vm
        .heap
        .get_own_property_descriptor(global, "watched")
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            after.get,
            after.set,
            after.value,
            after.writable,
            after.enumerable,
            after.configurable
        ),
        (
            descriptor.get,
            descriptor.set,
            descriptor.value,
            descriptor.writable,
            descriptor.enumerable,
            descriptor.configurable
        )
    );
}

#[cfg_attr(test, test)]
fn global_binding_admission_initial_value_copy_is_admitted_before_any_cell() {
    let mut vm = bounded_vm();
    let code = compile("var first,text;");
    vm.prepare_root_execution(&code, false).unwrap();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap
        .set(global, "text", Value::String("tiny".into()))
        .unwrap();
    let bytes = declaration_bytes(&vm, &["first", "text"]);
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(
        vm.prepare_global_declarations(&code),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.global_bindings.is_empty() && vm.script_global_slots.is_empty());
    assert_eq!(vm.heap.get_own(global, "first").unwrap(), None);
    assert_eq!(
        vm.heap.get_own(global, "text").unwrap(),
        Some(Value::String("tiny".into()))
    );
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn global_binding_admission_persistent_capacity_survives_frames_and_releases_with_names() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("let retained=42;")).unwrap();
    let bytes = vm.global_binding_storage_bytes();
    assert!(bytes > 0);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, bytes);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let (name, binding) = vm.global_bindings.remove_entry("retained").unwrap();
    let old_capacity = name.capacity();
    let mut padded = String::with_capacity(64);
    padded.push_str(&name);
    let added = padded.capacity() - old_capacity;
    vm.global_bindings.insert(padded, binding);
    vm.global_bindings.reserve(7);
    assert!(vm.host_scheduling_usage().unwrap().payload_bytes >= before + added);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.global_binding_value("retained").unwrap(),
        Some(Value::Number(42.0))
    );
    vm.reset_lexical_global_bindings();
    assert_eq!(vm.global_binding_storage_bytes(), 0);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn global_binding_admission_suspended_lexicals_keep_names_and_restore_without_growth() {
    for aborts in [false, true] {
        let mut vm = bounded_vm();
        vm.execute_script(&compile("let retained=7;")).unwrap();
        let cell = vm.global_bindings["retained"].cell;
        let pointer = vm
            .global_bindings
            .get_key_value("retained")
            .unwrap()
            .0
            .as_ptr();
        let saved = vm.suspend_lexical_global_bindings().unwrap();
        assert_eq!(vm.reserved_global_binding_slots, 1);
        assert_eq!(
            saved.bindings.get_key_value("retained").unwrap().0.as_ptr(),
            pointer
        );
        assert_eq!(
            vm.displaced_execution_control_bytes.get(),
            storage_bytes(&saved.bindings)
        );
        let owner = aborts.then(|| leave_room(&mut vm, 0));
        let result = vm.execute_script(&compile("var a=1,b=2,c=3,d=4;42;"));
        if aborts {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        } else {
            result.unwrap();
        }
        let capacity = vm.global_bindings.capacity();
        saved.restore(&mut vm);
        assert_eq!(vm.global_bindings.capacity(), capacity);
        assert_eq!(vm.global_bindings["retained"].cell, cell);
        assert_eq!(
            vm.global_bindings
                .get_key_value("retained")
                .unwrap()
                .0
                .as_ptr(),
            pointer
        );
        assert_eq!(vm.reserved_global_binding_slots, 0);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        if aborts {
            assert_eq!(
                vm.check_scheduling_failure(),
                Err(RuntimeError::SchedulingLimit)
            );
        }
        drop(owner);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            vm.global_binding_value("retained").unwrap(),
            Some(Value::Number(7.0))
        );
    }
}

#[cfg_attr(test, test)]
fn global_binding_admission_suspension_refusal_preserves_the_realm_and_peer_account() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("let retained=7;")).unwrap();
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let cell = vm.global_bindings["retained"].cell;
    let capacity = vm.global_bindings.capacity();
    let owner = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.suspend_lexical_global_bindings(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.global_bindings["retained"].cell, cell);
    assert_eq!(vm.global_bindings.capacity(), capacity);
    assert_eq!(vm.reserved_global_binding_slots, 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn global_binding_admission_restore_releases_a_replaced_property_binding_root() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("let retained=7;")).unwrap();
    let cell = vm.global_bindings["retained"].cell;
    let saved = vm.suspend_lexical_global_bindings().unwrap();
    vm.execute_script(&compile("var retained=99;")).unwrap();
    let replaced = vm.global_bindings["retained"].cell;
    assert_ne!(replaced, cell);
    saved.restore(&mut vm);
    assert_eq!(vm.global_bindings["retained"].cell, cell);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.heap.get_own(replaced, "value"),
        Err(HeapError::InvalidObject(replaced))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn global_binding_admission_recreation_heap_and_root_refusals_leave_no_property_or_root() {
    let mut probe = Heap::default();
    let cell = probe.alloc_object(None).unwrap();
    let before = probe.stats().managed_bytes;
    probe.set(cell, "value", Value::Number(42.0)).unwrap();
    let property_bytes = probe.stats().managed_bytes - before;
    for root_refusal in [false, true] {
        let mut vm = bounded_vm();
        let code = eval_code("var retained=1;");
        vm.prepare_root_execution(&code, false).unwrap();
        vm.prepare_eval_global_var_declarations(&code).unwrap();
        vm.enter_scope(&code, 0).unwrap();
        let slot = code
            .bindings
            .iter()
            .position(|binding| binding.name == "retained")
            .unwrap();
        let cell = vm.cells[&slot];
        let global = vm.global("globalThis").unwrap().object_id().unwrap();
        assert!(vm.delete_eval_var(slot).unwrap());
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        if root_refusal {
            vm.heap.allow_root_registrations(0);
        } else {
            vm.heap.allow_only(property_bytes);
        }
        let result = vm.assign_binding_slot(&code, slot, Value::Number(42.0), true);
        if root_refusal {
            assert_eq!(result, Err(RuntimeError::Heap(HeapError::IdExhausted)));
        } else {
            assert!(matches!(
                result,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
            ));
        }
        assert!(vm.global_bindings.is_empty());
        assert_eq!(vm.heap.get_own(global, "retained").unwrap(), None);
        assert_eq!(vm.heap.get_own(cell, "value").unwrap(), None);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
        vm.cells.remove(&slot);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            vm.heap.get_own(cell, "value"),
            Err(HeapError::InvalidObject(cell))
        );
    }
}

#[cfg_attr(test, test)]
fn global_binding_admission_abrupt_scripts_keep_required_global_tdz_state() {
    let mut vm = bounded_vm();
    assert!(matches!(
        vm.execute_script(&compile("let first=1;let never=missing;")),
        Err(RuntimeError::ReferenceError(_))
    ));
    assert_eq!(
        vm.global_binding_value("first").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(vm.global_binding_value("never").unwrap(), None);
    assert!(vm.global_binding_storage_bytes() > 0);
    assert!(matches!(
        vm.execute_script(&compile("never;")),
        Err(RuntimeError::ReferenceError(_))
    ));
    assert!(matches!(
        vm.execute_script(&compile("let never;")),
        Err(RuntimeError::SyntaxError(_))
    ));
}

#[cfg_attr(test, test)]
fn global_binding_admission_shadowrealm_reentry_preserves_outer_lexicals_under_gc() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let source = "var realm=new ShadowRealm();
            var run=realm.evaluate('let retained=7;(cb)=>{let answer=cb();return answer+retained;}');
            run(()=>realm.evaluate('let retained=99;var a=1,b=2,c=3,d=4;35;'));";
        assert_eq!(vm.execute_script(&compile(source)), Ok(Value::Number(42.0)));
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            vm.execute_script(&compile("21+21;")),
            Ok(Value::Number(42.0))
        );
    }
}

#[cfg_attr(test, test)]
fn global_binding_admission_eval_restoration_reconciles_alias_storage_to_peers() {
    for nested_script in [false, true] {
        let mut vm = bounded_vm();
        vm.execute_script(&compile("var parent=1;")).unwrap();
        let peer = Vm {
            host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
            ..Vm::default()
        };
        vm.host_scheduling_usage().unwrap();
        if nested_script {
            vm.execute_nested_script(&compile("var child=2;")).unwrap();
        } else {
            vm.execute_eval(&eval_code("var child=2;"), Vec::new(), true)
                .unwrap();
        }
        assert_eq!(
            peer.host_scheduling_snapshot().payload_bytes,
            vm.own_scheduling_usage().payload_bytes
        );
        assert_eq!(
            vm.global_binding_value("child").unwrap(),
            Some(Value::Number(2.0))
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

impl crate::Vm {
    /// Runs finite global-record ownership contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_global_binding_admission_contracts() {
        global_binding_admission_accounts_for_persistent_table_and_name_capacity();
        global_binding_admission_refuses_script_and_eval_before_cells_or_properties_publish();
        global_binding_admission_recreation_refuses_before_restoring_the_deleted_global();
        global_binding_admission_exact_budget_and_one_byte_refusal();
        global_binding_admission_existing_aliases_and_accessors_need_no_copy();
        global_binding_admission_initial_value_copy_is_admitted_before_any_cell();
        global_binding_admission_persistent_capacity_survives_frames_and_releases_with_names();
        global_binding_admission_suspended_lexicals_keep_names_and_restore_without_growth();
        global_binding_admission_suspension_refusal_preserves_the_realm_and_peer_account();
        global_binding_admission_restore_releases_a_replaced_property_binding_root();
        global_binding_admission_recreation_heap_and_root_refusals_leave_no_property_or_root();
        global_binding_admission_abrupt_scripts_keep_required_global_tdz_state();
        global_binding_admission_shadowrealm_reentry_preserves_outer_lexicals_under_gc();
        global_binding_admission_eval_restoration_reconciles_alias_storage_to_peers();
    }
}
