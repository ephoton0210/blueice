// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny compiler snapshots preflight before copying environment names.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 10_000;
    vm
}
fn binding(name: &str) -> Binding {
    Binding {
        name: name.into(),
        mutable: true,
        strict_immutable: false,
        lexical: false,
        catch_parameter: false,
        eval_var: false,
    }
}
fn visible_page() -> Vm {
    let mut vm = page();
    vm.bindings = vec![Some(Value::Number(1.0))];
    vm.binding_metadata = vec![binding("x")].into();
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

#[cfg(test)]
#[test]
fn eval_environment_storage_visible_refusal_precedes_name_copy() {
    let mut vm = visible_page();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| EvalCompileEnvironment::visible(&vm));
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
}

#[cfg(test)]
#[test]
fn eval_environment_storage_name_refusal_precedes_name_copy() {
    let mut vm = visible_page();
    let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| environment.finish_names(&vm, true));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn scoped_page() -> Vm {
    let mut vm = page();
    vm.binding_metadata = [
        "x", "x", "captured", "captured", "self", "x", "local", "caught", "blocked", "outerlex",
    ]
    .into_iter()
    .map(binding)
    .collect::<Vec<_>>()
    .into();
    for slot in [0, 3, 7, 8, 9] {
        std::sync::Arc::make_mut(&mut vm.binding_metadata)[slot].lexical = true;
    }
    std::sync::Arc::make_mut(&mut vm.binding_metadata)[0].mutable = false;
    std::sync::Arc::make_mut(&mut vm.binding_metadata)[0].strict_immutable = true;
    std::sync::Arc::make_mut(&mut vm.binding_metadata)[3].strict_immutable = true;
    std::sync::Arc::make_mut(&mut vm.binding_metadata)[4].mutable = false;
    std::sync::Arc::make_mut(&mut vm.binding_metadata)[7].catch_parameter = true;
    vm.bindings = vec![None; vm.binding_metadata.len()];
    vm.bindings[4] = Some(Value::Number(4.0));
    vm.bindings[5] = Some(Value::Number(5.0));
    vm.active_scopes = vec![10, 20, 30];
    vm.active_scope_slots = vec![vec![0, 9], vec![6, 8], vec![1, 7, 8]];
    vm.variable_scope = 20;
    let cell = vm.heap.alloc_object(None).unwrap();
    vm.cells.insert(2, cell);
    vm.cells.insert(3, cell);
    vm
}
fn dynamic(vm: &mut Vm, name: &str) -> ObjectId {
    let cell = vm.heap.alloc_object(None).unwrap();
    vm.dynamic_eval_bindings.insert(
        name.into(),
        crate::heap::DynamicEvalBinding {
            cell,
            shadowed_cells: Vec::new(),
        },
    );
    cell
}
fn assert_visible(vm: &Vm, environment: &EvalCompileEnvironment) {
    let mut expected = vm.eval_visible_bindings();
    let mut captures = HashMap::new();
    if vm.parameter_eval_env.is_some() {
        for (name, binding, slot) in &mut expected {
            if !vm
                .active_scope_slots
                .iter()
                .any(|slots| slots.contains(slot))
            {
                if let Some(dynamic) = vm.dynamic_eval_bindings.get(name) {
                    binding.mutable = true;
                    binding.strict_immutable = false;
                    binding.lexical = false;
                    binding.catch_parameter = false;
                    binding.eval_var = true;
                    captures.insert(*slot, dynamic.cell);
                }
            }
        }
    }
    assert!(expected == environment.visible);
    assert_eq!(captures, environment.dynamic_captures);
}
fn assert_names(vm: &Vm, environment: &EvalCompileEnvironment, global: bool) {
    let variables = if global {
        environment
            .visible
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect()
    } else {
        vm.eval_variable_environment_names()
    };
    let mut lexical = vm.eval_lexical_conflicts();
    if global {
        lexical.extend(
            vm.global_bindings
                .iter()
                .filter(|(_, binding)| !binding.property)
                .map(|(name, _)| name.clone()),
        );
    }
    assert_eq!(environment.variables, variables);
    assert_eq!(environment.lexical, lexical);
}

#[cfg_attr(test, test)]
fn eval_environment_storage_visible_preserves_all_binding_priorities() {
    let vm = scoped_page();
    let environment = EvalCompileEnvironment::visible(&vm).unwrap();
    assert_visible(&vm, &environment);
    let x = environment
        .visible
        .iter()
        .find(|(name, _, _)| name == "x")
        .unwrap();
    assert_eq!(x.2, 1);
    let named = environment
        .visible
        .iter()
        .find(|(name, _, _)| name == "self")
        .unwrap();
    assert!(!named.1.mutable && !named.1.strict_immutable);
    assert!(environment
        .visible
        .windows(2)
        .all(|pair| pair[0].0 < pair[1].0));
}

#[cfg_attr(test, test)]
fn eval_environment_storage_parameter_correction_preserves_active_shadowing() {
    let mut vm = scoped_page();
    vm.parameter_eval_env = Some(vm.heap.alloc_object(None).unwrap());
    let captured = dynamic(&mut vm, "captured");
    dynamic(&mut vm, "x");
    let environment = EvalCompileEnvironment::visible(&vm).unwrap();
    assert_visible(&vm, &environment);
    let candidate = environment
        .visible
        .iter()
        .find(|(name, _, _)| name == "captured")
        .unwrap();
    assert!(candidate.1.eval_var && candidate.1.mutable && !candidate.1.lexical);
    assert_eq!(
        environment.dynamic_captures.get(&candidate.2),
        Some(&captured)
    );
    assert!(!environment.dynamic_captures.contains_key(&1));
}

#[cfg_attr(test, test)]
fn eval_environment_storage_local_names_keep_variable_and_lexical_boundaries() {
    let mut vm = scoped_page();
    dynamic(&mut vm, "runtime");
    dynamic(&mut vm, "local");
    let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
    environment.finish_names(&vm, false).unwrap();
    assert_names(&vm, &environment, false);
    assert_eq!(environment.variables, ["local", "runtime"]);
    assert_eq!(environment.lexical, ["blocked"]);
}

#[cfg_attr(test, test)]
fn eval_environment_storage_global_names_preserve_append_order_and_duplicates() {
    let mut vm = scoped_page();
    for (name, property) in [("blocked", false), ("realm", false), ("property", true)] {
        let cell = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(cell).unwrap();
        vm.global_bindings.insert(
            name.into(),
            GlobalBinding {
                cell,
                mutable: true,
                property,
                _root: root,
            },
        );
    }
    let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
    environment.finish_names(&vm, true).unwrap();
    assert_names(&vm, &environment, true);
    assert_eq!(
        environment
            .lexical
            .iter()
            .filter(|name| *name == "blocked")
            .count(),
        2
    );
    assert!(!environment.lexical.iter().any(|name| name == "property"));
}

#[cfg_attr(test, test)]
fn eval_environment_storage_body_lexicals_distinguish_functions_and_arrows() {
    for (source, ordinary) in [("(function(){})", true), ("(()=>{})", false)] {
        let mut vm = page();
        let code = crate::compile(&crate::parse(source).unwrap()).unwrap();
        vm.callee = vm.execute_script(&code).unwrap();
        vm.active_scopes.clear();
        vm.active_scope_slots.clear();
        vm.variable_scope = 99;
        vm.variable_scope_lexicals = vec!["body".into(), "body".into()];
        let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
        environment.finish_names(&vm, false).unwrap();
        assert_names(&vm, &environment, false);
        assert_eq!(environment.lexical.len(), usize::from(ordinary));
    }
}

#[cfg_attr(test, test)]
fn eval_environment_storage_snapshot_outlives_vm_and_refunds_once() {
    let vm = scoped_page();
    let view = observer(&vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
    environment.finish_names(&vm, false).unwrap();
    let bytes = environment.retained_bytes();
    assert_eq!(environment._charge.as_ref().unwrap().bytes(), bytes);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(environment);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn eval_environment_storage_visible_peak_has_exact_and_one_short_admission() {
    let vm = scoped_page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (sample, peak) = scope.measure_payload_peak(|| EvalCompileEnvironment::visible(&vm));
    let sample = sample.unwrap();
    assert!(peak > sample.retained_bytes());
    drop(sample);
    for accepts in [false, true] {
        let mut vm = scoped_page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = EvalCompileEnvironment::visible(&vm);
        if accepts {
            drop(result.unwrap());
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(vm.binding_metadata[0].name, "x");
    }
}

#[cfg_attr(test, test)]
fn eval_environment_storage_name_peak_has_exact_and_one_short_admission() {
    let vm = scoped_page();
    let mut sample = EvalCompileEnvironment::visible(&vm).unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let (result, peak) = scope.measure_payload_peak(|| sample.finish_names(&vm, false));
    result.unwrap();
    assert!(peak > names_bytes(&sample.variables) + names_bytes(&sample.lexical));
    drop(sample);
    for accepts in [false, true] {
        let mut vm = scoped_page();
        let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, peak - usize::from(!accepts));
        let result = environment.finish_names(&vm, false);
        if accepts {
            result.unwrap();
            assert_names(&vm, &environment, false);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert!(environment.variables.is_empty() && environment.lexical.is_empty());
        }
        drop(peer);
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            before + names_bytes(&environment.variables) + names_bytes(&environment.lexical)
        );
    }
}

#[cfg_attr(test, test)]
fn eval_environment_storage_replacement_keeps_old_names_until_admission() {
    let mut vm = scoped_page();
    let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
    environment.finish_names(&vm, false).unwrap();
    let old_variables = environment.variables.clone();
    let old_lexical = environment.lexical.clone();
    let peer = leave_room(&mut vm, 0);
    assert_eq!(
        environment.finish_names(&vm, true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(environment.variables, old_variables);
    assert_eq!(environment.lexical, old_lexical);
    assert_eq!(
        environment._charge.as_ref().unwrap().bytes(),
        environment.retained_bytes()
    );
    drop(peer);
}

#[cfg_attr(test, test)]
fn eval_environment_storage_replacement_refunds_old_name_backing() {
    let mut vm = scoped_page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
    environment.finish_names(&vm, false).unwrap();
    environment.finish_names(&vm, true).unwrap();
    assert_names(&vm, &environment, true);
    assert_eq!(
        environment._charge.as_ref().unwrap().bytes(),
        environment.retained_bytes()
    );
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before + environment.retained_bytes()
    );
    drop(environment);
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn eval_environment_storage_foreign_and_detached_scopes_refuse() {
    for detached in [false, true] {
        let vm = visible_page();
        let mut environment = EvalCompileEnvironment::visible(&vm).unwrap();
        let before = environment.retained_bytes();
        let other = if detached { Vm::default() } else { page() };
        assert_eq!(
            environment.finish_names(&other, true),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(environment.retained_bytes(), before);
        assert!(environment.variables.is_empty());
    }
}

#[cfg_attr(test, test)]
fn eval_environment_storage_unbounded_snapshot_attaches_and_terminal_reuse_refuses() {
    let unbounded = Vm {
        bindings: vec![Some(Value::Number(1.0))],
        binding_metadata: vec![binding("x")].into(),
        ..Vm::default()
    };
    let mut environment = EvalCompileEnvironment::visible(&unbounded).unwrap();
    assert!(environment._charge.is_none());
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    environment.finish_names(&vm, true).unwrap();
    assert_eq!(
        environment._charge.as_ref().unwrap().bytes(),
        environment.retained_bytes()
    );
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        environment.finish_names(&vm, true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        EvalCompileEnvironment::visible(&vm).err(),
        Some(RuntimeError::SchedulingLimit)
    );
    drop(environment);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_eval_environment_storage_contracts() {
        eval_environment_storage_visible_preserves_all_binding_priorities();
        eval_environment_storage_parameter_correction_preserves_active_shadowing();
        eval_environment_storage_local_names_keep_variable_and_lexical_boundaries();
        eval_environment_storage_global_names_preserve_append_order_and_duplicates();
        eval_environment_storage_body_lexicals_distinguish_functions_and_arrows();
        eval_environment_storage_snapshot_outlives_vm_and_refunds_once();
        eval_environment_storage_visible_peak_has_exact_and_one_short_admission();
        eval_environment_storage_name_peak_has_exact_and_one_short_admission();
        eval_environment_storage_replacement_keeps_old_names_until_admission();
        eval_environment_storage_replacement_refunds_old_name_backing();
        eval_environment_storage_foreign_and_detached_scopes_refuse();
        eval_environment_storage_unbounded_snapshot_attaches_and_terminal_reuse_refuses();
    }
}
