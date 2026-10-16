// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small suspended graphs and scalar budgets verify native dependency owners.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn compile(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}

struct Fixture {
    vm: Vm,
    registry: ModuleRegistry,
    observed: Rc<RefCell<Vec<usize>>>,
}

impl Fixture {
    fn new(parents: usize, distinct: bool) -> Self {
        let mut vm = Vm::default();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let output = observed.clone();
        let counter = vm.displaced_execution_bytes.clone();
        vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
            output.borrow_mut().push(counter.get());
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let children = if distinct { parents + 1 } else { 1 };
        execute(&mut vm, &format!("var gates=[]; for(var i=0;i<{children};i++) gates.push(Promise.withResolvers()); 0;"));
        let mut registry = HashMap::new();
        for child in 0..children {
            registry.insert(
                format!("test/m{child}.mjs"),
                compile(&format!(
                    "await gates[{child}].promise; export const value=1;"
                )),
            );
        }
        for parent in 0..=parents {
            let child = if distinct { parent } else { 0 };
            registry.insert(
                format!("test/p{parent}.mjs"),
                compile(&format!(
                    "import './m{child}.mjs'; observe(); export const value=1;"
                )),
            );
        }
        if children >= 2 {
            registry.insert(
                "test/batch.mjs".into(),
                compile("import './m0.mjs'; import './m1.mjs'; observe(); export const value=1;"),
            );
        }
        for child in 0..children {
            vm.execute_module_graph(&format!("test/m{child}.mjs"), &registry)
                .unwrap();
        }
        let registry = vm.module_registry.clone();
        let mut fixture = Self {
            vm,
            registry,
            observed,
        };
        for parent in 0..parents {
            assert_eq!(
                fixture.evaluate(&format!("test/p{parent}.mjs")),
                Ok(Value::Undefined)
            );
        }
        fixture
    }

    fn evaluate(&mut self, parent: &str) -> Result<Value, RuntimeError> {
        self.vm.with_module_records(|vm, linked| {
            vm.evaluate_module_record(parent, &self.registry, linked)
        })?
    }

    fn assert_refused(&mut self, parent: &str) {
        let forward = self.vm.module_pending_dependencies.clone();
        let reverse = self.vm.module_async_parents.clone();
        let capacities = (
            self.vm.module_pending_dependencies.capacity(),
            self.vm.module_async_parents.capacity(),
        );
        assert_eq!(self.evaluate(parent), Err(RuntimeError::SchedulingLimit));
        assert_eq!(self.vm.module_pending_dependencies, forward);
        assert_eq!(self.vm.module_async_parents, reverse);
        assert_eq!(
            (
                self.vm.module_pending_dependencies.capacity(),
                self.vm.module_async_parents.capacity()
            ),
            capacities
        );
        let record = self.vm.linked_record(parent).unwrap();
        assert!(!record.suspended && !record.evaluating);
    }
}

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

#[cfg_attr(test, test)]
fn module_dependency_forward_storage_tracks_tables_sets_and_names() {
    for part in 0..4 {
        let mut fixture = Fixture::new(1, false);
        let before = fixture.vm.host_scheduling_usage().unwrap().payload_bytes;
        let map = &mut fixture.vm.module_pending_dependencies;
        let delta = match part {
            0 => {
                let capacity = map.capacity();
                map.reserve(capacity + 1);
                (map.capacity() - capacity) * std::mem::size_of::<(String, HashSet<String>)>()
            }
            1 => {
                let set = map.get_mut("test/p0.mjs").unwrap();
                let capacity = set.capacity();
                set.reserve(capacity + 1);
                (set.capacity() - capacity) * std::mem::size_of::<String>()
            }
            2 => {
                let (mut key, set) = map.remove_entry("test/p0.mjs").unwrap();
                let old = key.capacity();
                key.reserve(32);
                let delta = key.capacity() - old;
                map.insert(key, set);
                delta
            }
            _ => {
                let set = map.get_mut("test/p0.mjs").unwrap();
                let mut name = set.take("test/m0.mjs").unwrap();
                let old = name.capacity();
                name.reserve(32);
                let delta = name.capacity() - old;
                set.insert(name);
                delta
            }
        };
        assert!(delta > 0);
        assert_eq!(
            fixture.vm.host_scheduling_usage().unwrap().payload_bytes - before,
            delta,
            "part={part}"
        );
    }
}

#[cfg_attr(test, test)]
fn module_dependency_reverse_storage_tracks_tables_vectors_and_names() {
    for part in 0..4 {
        let mut fixture = Fixture::new(1, false);
        let before = fixture.vm.host_scheduling_usage().unwrap().payload_bytes;
        let map = &mut fixture.vm.module_async_parents;
        let delta = match part {
            0 => {
                let capacity = map.capacity();
                map.reserve(capacity + 1);
                (map.capacity() - capacity) * std::mem::size_of::<(String, Vec<String>)>()
            }
            1 => {
                let values = map.get_mut("test/m0.mjs").unwrap();
                let capacity = values.capacity();
                values.reserve_exact(capacity + 1);
                (values.capacity() - capacity) * std::mem::size_of::<String>()
            }
            2 => {
                let (mut key, values) = map.remove_entry("test/m0.mjs").unwrap();
                let old = key.capacity();
                key.reserve(32);
                let delta = key.capacity() - old;
                map.insert(key, values);
                delta
            }
            _ => {
                let name = &mut map.get_mut("test/m0.mjs").unwrap()[0];
                let old = name.capacity();
                name.reserve(32);
                name.capacity() - old
            }
        };
        assert!(delta > 0);
        assert_eq!(
            fixture.vm.host_scheduling_usage().unwrap().payload_bytes - before,
            delta,
            "part={part}"
        );
    }
}

#[cfg_attr(test, test)]
fn module_dependency_first_edge_refuses_before_either_direction_is_published() {
    let mut fixture = Fixture::new(0, false);
    let _owner = leave_room(&mut fixture.vm, 0);
    fixture.assert_refused("test/p0.mjs");
}

#[cfg_attr(test, test)]
fn module_dependency_table_growth_refuses_before_either_direction_is_published() {
    let mut fixture = Fixture::new(7, true);
    assert_eq!(
        fixture.vm.module_pending_dependencies.len(),
        fixture.vm.module_pending_dependencies.capacity()
    );
    assert_eq!(
        fixture.vm.module_async_parents.len(),
        fixture.vm.module_async_parents.capacity()
    );
    let _owner = leave_room(&mut fixture.vm, 0);
    fixture.assert_refused("test/p7.mjs");
}

#[cfg_attr(test, test)]
fn module_dependency_reverse_vector_growth_refuses_before_publication() {
    let mut fixture = Fixture::new(8, false);
    let values = &fixture.vm.module_async_parents["test/m0.mjs"];
    assert_eq!(values.len(), values.capacity());
    let _owner = leave_room(&mut fixture.vm, 0);
    fixture.assert_refused("test/p8.mjs");
}

fn one_edge_bytes(parent: &str) -> usize {
    2 * parent.len()
        + "test/m0.mjs".len()
        + HashSet::<String>::with_capacity(1).capacity() * std::mem::size_of::<String>()
}

#[cfg_attr(test, test)]
fn module_dependency_batch_refusal_does_not_publish_a_partial_first_edge() {
    let mut fixture = Fixture::new(1, true);
    let _owner = leave_room(&mut fixture.vm, one_edge_bytes("test/batch.mjs"));
    fixture.assert_refused("test/batch.mjs");
}

#[cfg_attr(test, test)]
fn module_dependency_spare_slots_admit_only_new_set_and_name_storage() {
    let mut fixture = Fixture::new(1, false);
    let bytes = one_edge_bytes("test/p1.mjs");
    let before = fixture.vm.host_scheduling_usage().unwrap().payload_bytes;
    let _owner = leave_room(&mut fixture.vm, bytes);
    assert_eq!(fixture.evaluate("test/p1.mjs"), Ok(Value::Undefined));
    let remaining = fixture.vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(remaining, MAX_HOST_SCHEDULING_BYTES);
    assert!(before < remaining);
    assert_eq!(
        fixture.vm.module_async_parents["test/m0.mjs"],
        vec!["test/p0.mjs", "test/p1.mjs"]
    );
}

#[cfg_attr(test, test)]
fn module_dependency_completion_releases_empty_native_tables() {
    let mut fixture = Fixture::new(2, false);
    execute(&mut fixture.vm, "gates[0].resolve(); 0;");
    fixture.vm.run_promise_jobs_bounded(32).unwrap();
    assert!(
        fixture.vm.module_pending_dependencies.is_empty()
            && fixture.vm.module_async_parents.is_empty()
    );
    assert_eq!(fixture.vm.module_pending_dependencies.capacity(), 0);
    assert_eq!(fixture.vm.module_async_parents.capacity(), 0);
}

#[cfg_attr(test, test)]
fn module_dependency_rejection_removes_obsolete_links_from_a_live_sibling() {
    let mut fixture = Fixture::new(1, true);
    fixture.evaluate("test/batch.mjs").unwrap();
    execute(&mut fixture.vm, "gates[0].reject(7); 0;");
    fixture.vm.run_promise_jobs_bounded(32).unwrap();
    assert!(fixture.vm.linked_record("test/m1.mjs").unwrap().suspended);
    assert!(fixture
        .vm
        .linked_record("test/batch.mjs")
        .unwrap()
        .error
        .is_some());
    assert!(fixture.vm.module_pending_dependencies.is_empty());
    assert!(fixture.vm.module_async_parents.is_empty());
    assert_eq!(fixture.vm.module_pending_dependencies.capacity(), 0);
    assert_eq!(fixture.vm.module_async_parents.capacity(), 0);
    assert_eq!(
        fixture
            .vm
            .host_scheduling_usage()
            .unwrap()
            .pending_reactions,
        1
    );
}

#[cfg_attr(test, test)]
fn module_dependency_walk_storage_is_charged_during_parent_callbacks() {
    let mut fixture = Fixture::new(2, false);
    execute(&mut fixture.vm, "gates[0].resolve(); 0;");
    fixture.vm.run_promise_jobs_bounded(32).unwrap();
    let observed = fixture.observed.borrow();
    assert_eq!(observed.len(), 2);
    let minimum = 5 * std::mem::size_of::<String>()
        + "test/m0.mjs".len()
        + "test/p0.mjs".len()
        + "test/p1.mjs".len();
    assert!(
        observed[0] >= minimum,
        "observed={observed:?}, minimum={minimum}"
    );
    assert_eq!(fixture.vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_deferred_frontier_storage_is_charged_before_dependency_code_runs() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = observed.clone();
    let counter = vm.displaced_execution_bytes.clone();
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        output.borrow_mut().push(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    execute(
        &mut vm,
        "var gate=Promise.withResolvers(); function callback() {} 0;",
    );
    let registry = HashMap::from([
        (
            "test/entry.mjs".into(),
            compile("import './a.mjs'; import './b.mjs'; export const value=1;"),
        ),
        (
            "test/a.mjs".into(),
            compile("observe(); await gate.promise;"),
        ),
        (
            "test/b.mjs".into(),
            compile("observe(); await gate.promise;"),
        ),
    ]);
    vm.set_module_loader_context("test/main.js", registry);
    execute(&mut vm, "var target=import.defer('./entry.mjs'); 0;");
    let job_bytes = vm.promise_jobs[0].scheduling_payload_bytes();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback });
    vm.run_next_promise_job().unwrap();
    let minimum =
        job_bytes + 2 * std::mem::size_of::<String>() + "test/a.mjs".len() + "test/b.mjs".len();
    assert!(
        observed.borrow()[0] >= minimum,
        "observed={:?}, minimum={minimum}",
        observed.borrow()
    );
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_dependency_duplicate_batches_do_not_recharge_or_reorder_edges() {
    let mut fixture = Fixture::new(0, false);
    let mut bytes =
        one_edge_bytes("test/p0.mjs") + "test/m0.mjs".len() + 4 * std::mem::size_of::<String>();
    bytes += HashMap::<String, HashSet<String>>::with_capacity(1).capacity()
        * std::mem::size_of::<(String, HashSet<String>)>();
    bytes += HashMap::<String, Vec<String>>::with_capacity(1).capacity()
        * std::mem::size_of::<(String, Vec<String>)>();
    let _owner = leave_room(&mut fixture.vm, bytes);
    for dependencies in [
        vec!["test/m0.mjs".into(), "test/m0.mjs".into()],
        vec!["test/m0.mjs".into()],
    ] {
        fixture
            .vm
            .register_async_module_dependencies("test/p0.mjs", dependencies)
            .unwrap();
        assert_eq!(
            fixture.vm.module_pending_dependencies["test/p0.mjs"].len(),
            1
        );
        assert_eq!(
            fixture.vm.module_async_parents["test/m0.mjs"],
            vec!["test/p0.mjs"]
        );
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES
        );
    }
}

fn ordering_graph(failing_parent: bool) -> (Vm, HashMap<String, Bytecode>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    execute(&mut vm, "var gate=Promise.withResolvers(), order=[]; 0;");
    let first = if failing_parent {
        "throw 7;"
    } else {
        "order.push(0);"
    };
    let registry = HashMap::from([
        (
            "test/root.mjs".into(),
            compile("import './p0.mjs'; import './p1.mjs'; order.push(2);"),
        ),
        (
            "test/p0.mjs".into(),
            compile(&format!("import './m0.mjs'; {first}")),
        ),
        (
            "test/p1.mjs".into(),
            compile("import './m0.mjs'; order.push(1);"),
        ),
        ("test/m0.mjs".into(), compile("await gate.promise;")),
    ]);
    vm.execute_module_graph("test/root.mjs", &registry).unwrap();
    (vm, registry)
}

#[cfg_attr(test, test)]
fn module_dependency_callbacks_keep_sibling_and_breadth_first_order() {
    let (mut vm, _) = ordering_graph(false);
    execute(&mut vm, "gate.resolve(); 0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(
        execute(&mut vm, "order.join(',');"),
        Value::String("0,1,2".into())
    );
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    assert_eq!(vm.module_pending_dependencies.capacity(), 0);
    assert_eq!(vm.module_async_parents.capacity(), 0);
}

#[cfg_attr(test, test)]
fn module_dependency_parent_failure_releases_walk_owners_and_runs_ready_siblings() {
    let (mut vm, _) = ordering_graph(true);
    execute(&mut vm, "gate.resolve(); 0;");
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(
        execute(&mut vm, "order.join(',');"),
        Value::String("1".into())
    );
    assert!(vm.linked_record("test/root.mjs").unwrap().error.is_some());
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    assert_eq!(vm.module_pending_dependencies.capacity(), 0);
    assert_eq!(vm.module_async_parents.capacity(), 0);
}

#[cfg_attr(test, test)]
fn module_dependency_resource_abort_refunds_walk_leases() {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("abortDocument", 0, move |_args: &[HostValue]| {
        scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    execute(&mut vm, "var gate=Promise.withResolvers(); 0;");
    let registry = HashMap::from([
        (
            "test/root.mjs".into(),
            compile("import './m0.mjs'; abortDocument();"),
        ),
        ("test/m0.mjs".into(), compile("await gate.promise;")),
    ]);
    vm.execute_module_graph("test/root.mjs", &registry).unwrap();
    execute(&mut vm, "gate.resolve(); 0;");
    assert_eq!(
        vm.run_next_promise_job(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_registry.retained_bytes()
            + vm.module_record_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_dependency_collector_storage_follows_later_dependency_callbacks() {
    for abort in [false, true] {
        let mut vm = Vm::default();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let output = observed.clone();
        let counter = vm.displaced_execution_bytes.clone();
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
            output.borrow_mut().push(counter.get());
            if abort {
                scope.fail();
            }
            Ok(HostValue::Undefined)
        })
        .unwrap();
        execute(
            &mut vm,
            "var gates=[Promise.withResolvers(),Promise.withResolvers()]; 0;",
        );
        let registry = HashMap::from([
            (
                "test/root.mjs".into(),
                compile("import './hot.mjs'; import './cold.mjs';"),
            ),
            ("test/hot.mjs".into(), compile("await gates[0].promise;")),
            (
                "test/cold.mjs".into(),
                compile("observe(); await gates[1].promise;"),
            ),
        ]);
        vm.execute_module_graph("test/hot.mjs", &registry).unwrap();
        let result = vm.execute_module_graph("test/root.mjs", &registry);
        if abort {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        } else {
            result.unwrap();
        }
        let minimum =
            2 * std::mem::size_of::<String>() + "test/hot.mjs".len() + "test/cold.mjs".len();
        assert!(
            observed.borrow()[0] >= minimum,
            "observed={:?}, minimum={minimum}",
            observed.borrow()
        );
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
}

impl crate::Vm {
    /// Runs module dependency ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_module_dependency_ownership_contracts() {
        module_dependency_forward_storage_tracks_tables_sets_and_names();
        module_dependency_reverse_storage_tracks_tables_vectors_and_names();
        module_dependency_first_edge_refuses_before_either_direction_is_published();
        module_dependency_table_growth_refuses_before_either_direction_is_published();
        module_dependency_reverse_vector_growth_refuses_before_publication();
        module_dependency_batch_refusal_does_not_publish_a_partial_first_edge();
        module_dependency_spare_slots_admit_only_new_set_and_name_storage();
        module_dependency_completion_releases_empty_native_tables();
        module_dependency_rejection_removes_obsolete_links_from_a_live_sibling();
        module_dependency_walk_storage_is_charged_during_parent_callbacks();
        module_deferred_frontier_storage_is_charged_before_dependency_code_runs();
        module_dependency_duplicate_batches_do_not_recharge_or_reorder_edges();
        module_dependency_callbacks_keep_sibling_and_breadth_first_order();
        module_dependency_parent_failure_releases_walk_owners_and_runs_ready_siblings();
        module_dependency_resource_abort_refunds_walk_leases();
        module_dependency_collector_storage_follows_later_dependency_callbacks();
    }
}
