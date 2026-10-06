// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn promise() -> ObjectId {
    Vm::default().heap.alloc_object(None).unwrap()
}

fn yield_exit() -> InterpreterExit {
    InterpreterExit::Yield {
        value: Value::Undefined,
        pc: 3,
        iterators: Vec::new(),
        handlers: Vec::new(),
    }
}

fn suspend_exit(pc: usize) -> InterpreterExit {
    InterpreterExit::Suspend {
        pc,
        iterators: Vec::new(),
        handlers: Vec::new(),
    }
}

fn await_exit() -> InterpreterExit {
    InterpreterExit::Await {
        promise: promise(),
        pc: 5,
        handlers: Vec::new(),
    }
}

fn straight_line(outcome: Result<InterpreterExit, RuntimeError>) -> String {
    match straight_line_exit(outcome, "no yield here") {
        Ok(StraightLineExit::Return(value)) => format!("return {value:?}"),
        Ok(StraightLineExit::Await { pc, .. }) => format!("await at {pc}"),
        Err(error) => format!("error {error:?}"),
    }
}

fn async_generator(outcome: Result<InterpreterExit, RuntimeError>) -> String {
    match async_generator_exit(outcome) {
        Ok(AsyncGeneratorExit::Return(value)) => format!("return {value:?}"),
        Ok(AsyncGeneratorExit::Yield { pc, .. }) => format!("yield at {pc}"),
        Ok(AsyncGeneratorExit::Await { pc, .. }) => format!("await at {pc}"),
        Err(error) => format!("error {error:?}"),
    }
}

#[cfg_attr(test, test)]
fn namespace_and_deferred_walks_reject_invalid_requests_and_missing_execution_contexts() {
    for (source, name) in [
        ("export {answer} from '../outside.mjs';", "answer"),
        ("export * as ns from '../outside.mjs';", "ns"),
        (
            "import defer * as ns from '../outside.mjs'; export {ns};",
            "ns",
        ),
        ("export * from '../outside.mjs';", "answer"),
        (
            "import source blob from '../outside.wasm'; export {blob as source};",
            "source",
        ),
    ] {
        let code = crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
        let modules = HashMap::from([("main.mjs".into(), code)]);
        let mut resolving = Vec::new();
        assert!(matches!(
            Vm::resolve_export(&modules, "main.mjs", name, &mut resolving),
            Err(RuntimeError::ModuleResolution(_))
        ));
        assert!(
            resolving.is_empty(),
            "failed export resolution retained its cycle marker"
        );
        if source.starts_with("export * from") {
            let mut stars = HashSet::new();
            assert!(matches!(
                Vm::exported_names(&modules, "main.mjs", &mut stars),
                Err(RuntimeError::ModuleResolution(_))
            ));
            assert!(stars.is_empty());
        }
    }
    let absent = HashMap::new();
    assert_eq!(
        Vm::gather_async_dependencies("missing", &absent, &HashMap::new(), &mut HashSet::new()),
        Ok(Vec::new())
    );
    assert_eq!(
        Vm::ready_for_sync_execution("missing", &absent, &HashMap::new(), &mut HashSet::new()),
        Ok(true)
    );
    for broken_entry in [false, true] {
        let main = if broken_entry {
            "import '../outside.mjs';"
        } else {
            "import './other.mjs';"
        };
        let modules = HashMap::from([
            (
                "main.mjs".into(),
                crate::compile_module(&crate::parse_module(main).unwrap()).unwrap(),
            ),
            (
                "other.mjs".into(),
                crate::compile_module(&crate::parse_module("import '../outside.mjs';").unwrap())
                    .unwrap(),
            ),
        ]);
        let mut linked = HashMap::from([
            ("main.mjs".into(), record(false, false)),
            ("other.mjs".into(), record(false, false)),
        ]);
        assert!(matches!(
            Vm::gather_async_dependencies("main.mjs", &modules, &linked, &mut HashSet::new()),
            Err(RuntimeError::ModuleResolution(_))
        ));
        assert!(matches!(
            Vm::ready_for_sync_execution("main.mjs", &modules, &linked, &mut HashSet::new()),
            Err(RuntimeError::ModuleResolution(_))
        ));
        linked.get_mut("main.mjs").unwrap().evaluated = true;
        assert!(matches!(
            Vm::scc_evaluated("main.mjs", &modules, &linked),
            Err(RuntimeError::ModuleResolution(_))
        ));
        linked.get_mut("other.mjs").unwrap().error = Some(Value::Number(7.0));
        linked.get_mut("other.mjs").unwrap().evaluated = true;
        assert!(matches!(
            Vm::cycle_root_error("main.mjs", &modules, &linked),
            Err(RuntimeError::ModuleResolution(_))
        ));
    }
    let mut vm = Vm::default();
    let expected = Err(RuntimeError::TypeError(
        "the module graph of a deferred namespace is not available".into(),
    ));
    assert_eq!(vm.with_module_records(|_, _| ()), expected);
    assert_eq!(vm.evaluate_module_sync("missing"), expected);
    assert_eq!(
        vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn module_turns_separate_a_debugger_pause_from_return_await_and_failure() {
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    match ModuleRun::classify(Ok(InterpreterExit::Suspend {
        pc: 7,
        iterators: Vec::new(),
        handlers: Vec::new(),
    })) {
        ModuleRun::Paused {
            pc,
            iterators,
            handlers,
        } => {
            assert_eq!(pc, 7);
            assert!(iterators.is_empty() && handlers.is_empty());
        }
        _ => panic!("debugger suspension entered terminal module cleanup"),
    }
    match ModuleRun::classify(Ok(InterpreterExit::Return(Value::Number(42.0)))) {
        ModuleRun::Completed(Ok(StraightLineExit::Return(value))) => {
            assert_eq!(value, Value::Number(42.0))
        }
        _ => panic!("module return lost its completion value"),
    }
    match ModuleRun::classify(Ok(InterpreterExit::Await {
        promise,
        pc: 9,
        handlers: Vec::new(),
    })) {
        ModuleRun::Completed(Ok(StraightLineExit::Await {
            promise: actual,
            pc,
            handlers,
        })) => {
            assert_eq!(actual, promise);
            assert_eq!(pc, 9);
            assert!(handlers.is_empty());
        }
        _ => panic!("module await lost its registered promise"),
    }
    for (outcome, expected) in [
        (
            Ok(yield_exit()),
            RuntimeError::TypeError("yield requires a generator function".into()),
        ),
        (
            Err(RuntimeError::InstructionLimit),
            RuntimeError::InstructionLimit,
        ),
    ] {
        match ModuleRun::classify(outcome) {
            ModuleRun::Completed(Err(error)) => assert_eq!(error, expected),
            _ => panic!("an abrupt module turn did not fail"),
        }
    }
}

#[cfg_attr(test, test)]
fn code_that_is_not_a_generator_only_returns_or_awaits() {
    assert_eq!(
        straight_line(Ok(InterpreterExit::Return(Value::Number(1.0)))),
        "return Number(1.0)"
    );
    assert_eq!(straight_line(Ok(await_exit())), "await at 5");
    assert_eq!(
        straight_line(Ok(yield_exit())),
        "error TypeError(\"no yield here\")"
    );
    assert_eq!(
        straight_line(Ok(suspend_exit(0))),
        "error TypeError(\"only a generator suspends at its entry\")"
    );
    assert_eq!(
        straight_line(Err(RuntimeError::InstructionLimit)),
        "error InstructionLimit"
    );
}

#[cfg_attr(test, test)]
fn an_async_generator_resumption_returns_yields_or_awaits() {
    assert_eq!(
        async_generator(Ok(InterpreterExit::Return(Value::Null))),
        "return Null"
    );
    assert_eq!(async_generator(Ok(yield_exit())), "yield at 3");
    assert_eq!(async_generator(Ok(await_exit())), "await at 5");
    assert_eq!(
        async_generator(Ok(suspend_exit(0))),
        "error TypeError(\"only a generator suspends at its entry\")"
    );
    assert_eq!(
        async_generator(Err(RuntimeError::InstructionLimit)),
        "error InstructionLimit"
    );
}

#[cfg_attr(test, test)]
fn a_declaration_prefix_must_suspend_exactly_at_the_evaluation_entry() {
    assert_eq!(expect_entry_suspend(suspend_exit(7), 7), Ok(()));
    let unsupported = Err(RuntimeError::Unsupported(
        "module declaration prefix did not suspend at its evaluation entry",
    ));
    assert_eq!(expect_entry_suspend(suspend_exit(6), 7), unsupported);
    assert_eq!(
        expect_entry_suspend(InterpreterExit::Return(Value::Undefined), 7),
        unsupported
    );
}

fn rejected(action: CompletionAction) -> String {
    match rejected_await_step(action, 7, "no tail calls here") {
        Ok(RejectedAwait::Resume(pc)) => format!("resume at {pc}"),
        Ok(RejectedAwait::Return(value)) => format!("return {value:?}"),
        Err(error) => format!("error {error:?}"),
    }
}

#[cfg_attr(test, test)]
fn a_rejected_await_resumes_in_its_handler_returns_or_fails() {
    assert_eq!(rejected(CompletionAction::Continue), "resume at 7");
    assert_eq!(rejected(CompletionAction::Jump(9)), "resume at 9");
    assert_eq!(
        rejected(CompletionAction::Return(Value::Number(1.0))),
        "return Number(1.0)"
    );
    for action in [
        CompletionAction::TailRecur(Vec::new()),
        CompletionAction::TailCall(Vec::new()),
    ] {
        assert_eq!(rejected(action), "error TypeError(\"no tail calls here\")");
    }
    assert_eq!(
        rejected(CompletionAction::Throw(RuntimeError::Thrown(
            Value::Number(2.0)
        ))),
        "error Thrown(Number(2.0))"
    );
}

/// A module the host built by hand: the requests and exports it names,
/// and the offset of its evaluation entry (none for a broken one).
fn hand_built(
    exports: Vec<ModuleExport>,
    requests: &[(&str, bool)],
    entry: Option<u32>,
) -> Bytecode {
    let mut code = Bytecode::empty();
    code.module = true;
    code.strict = true;
    code.module_evaluate_entry = entry;
    code.module_exports = exports;
    code.module_requests = requests
        .iter()
        .map(|(request, deferred)| crate::bytecode::ModuleRequest {
            module_request: (*request).to_string(),
            module_type: ModuleType::JavaScript,
            deferred: *deferred,
        })
        .collect();
    code
}

fn compiled_module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}

const ESCAPES: &str = "relative module request ../../x escapes its host root";

#[cfg_attr(test, test)]
fn imports_are_named_in_errors_the_way_they_are_spelled() {
    assert_eq!(
        imported_name_text(&ModuleImportName::Named("x".into())),
        "x"
    );
    assert_eq!(imported_name_text(&ModuleImportName::Namespace), "*");
    assert_eq!(
        imported_name_text(&ModuleImportName::DeferredNamespace),
        "*"
    );
    assert_eq!(imported_name_text(&ModuleImportName::Source), "source");
}

#[cfg_attr(test, test)]
fn a_frame_carries_on_as_its_thrown_value_was_resolved() {
    let mut vm = Vm::default();
    let mut resume = |action: Result<CompletionAction, RuntimeError>| {
        straight_line(vm.interpret_after_throw(
            &Bytecode::empty(),
            &mut Vec::new(),
            4,
            Vec::new(),
            action,
            "no tails",
        ))
    };
    assert_eq!(
        resume(Ok(CompletionAction::Return(Value::Number(3.0)))),
        "return Number(3.0)"
    );
    assert_eq!(
        resume(Ok(CompletionAction::TailCall(Vec::new()))),
        "error TypeError(\"no tails\")"
    );
    assert_eq!(
        resume(Err(RuntimeError::InstructionLimit)),
        "error InstructionLimit"
    );
}

#[cfg_attr(test, test)]
fn a_module_only_counts_as_reachable_when_every_request_on_the_way_resolves() {
    let modules = HashMap::from([
        (
            "t/a.js".to_string(),
            hand_built(
                Vec::new(),
                &[("./done.js", true), ("./b.js", false), ("./c.js", false)],
                Some(0),
            ),
        ),
        (
            "t/b.js".to_string(),
            hand_built(Vec::new(), &[("../../x", false)], Some(0)),
        ),
        (
            "t/c.js".to_string(),
            hand_built(Vec::new(), &[("./a.js", false)], Some(0)),
        ),
        (
            "t/leaf.js".to_string(),
            hand_built(Vec::new(), &[], Some(0)),
        ),
        (
            "t/d.js".to_string(),
            hand_built(Vec::new(), &[("./leaf.js", false)], Some(0)),
        ),
    ]);
    let reaches =
        |from: &str, goal: &str| Vm::module_reaches(from, goal, &modules, &mut HashSet::new());
    assert_eq!(
        reaches("t/nowhere.js", "t/goal.js"),
        Err(RuntimeError::ModuleResolution(
            "module t/nowhere.js was not linked".into()
        ))
    );
    let escapes = Err(RuntimeError::ModuleResolution(ESCAPES.into()));
    assert_eq!(reaches("t/b.js", "t/goal.js"), escapes);
    assert_eq!(reaches("t/a.js", "t/goal.js"), escapes);
    // A module reaches itself, and what its requests reach; a deferred
    // request is no edge, and a module that is asked about twice while
    // one walk is under way is only walked once.
    assert_eq!(reaches("t/a.js", "t/a.js"), Ok(true));
    assert_eq!(reaches("t/d.js", "t/leaf.js"), Ok(true));
    assert_eq!(reaches("t/d.js", "t/a.js"), Ok(false));
    let mut visited = HashSet::from(["t/leaf.js".to_string()]);
    assert_eq!(
        Vm::module_reaches("t/leaf.js", "t/a.js", &modules, &mut visited),
        Ok(false)
    );
}

fn record(evaluating: bool, suspended: bool) -> LinkedModule {
    LinkedModule {
        cells: HashMap::new(),
        namespace: None,
        deferred_namespace: None,
        evaluated: false,
        evaluating,
        suspended,
        completion: None,
        error: None,
    }
}

#[cfg_attr(test, test)]
fn the_cycle_root_of_a_dependency_is_the_waiting_parent_it_reaches() {
    let modules = HashMap::from([
        (
            "t/dep.js".to_string(),
            hand_built(Vec::new(), &[("./parent.js", false)], Some(0)),
        ),
        (
            "t/parent.js".to_string(),
            hand_built(Vec::new(), &[], Some(0)),
        ),
        (
            "t/broken.js".to_string(),
            hand_built(Vec::new(), &[("../../x", false)], Some(0)),
        ),
    ]);
    let mut vm = Vm::default();
    vm.module_async_parents.insert(
        "t/dep.js".to_string(),
        vec![
            "t/gone.js".to_string(),
            "t/idle.js".to_string(),
            "t/parent.js".to_string(),
        ],
    );
    vm.module_async_parents
        .insert("t/broken.js".to_string(), vec!["t/parent.js".to_string()]);
    let linked = HashMap::from([
        ("t/idle.js".to_string(), record(false, false)),
        ("t/parent.js".to_string(), record(true, false)),
    ]);
    // A parent that is unknown or idle is passed over; the one that waits
    // and is reached from the dependency is its cycle root.
    assert_eq!(
        vm.async_dependency_root("t/dep.js", &modules, &linked),
        Ok("t/parent.js".to_string())
    );
    // One that waits without being reachable leaves the dependency itself.
    assert_eq!(
        vm.async_dependency_root("t/parent.js", &modules, &linked),
        Ok("t/parent.js".to_string())
    );
    assert_eq!(
        vm.async_dependency_root("t/broken.js", &modules, &linked),
        Err(RuntimeError::ModuleResolution(ESCAPES.into()))
    );
}

#[cfg_attr(test, test)]
fn a_rejected_await_is_caught_where_the_frame_left_off() {
    let modules = HashMap::from([(
        "t/main.js".to_string(),
        compiled_module(
            "let seen; try { await Promise.reject(7) } catch (e) { seen = e } seen === 7",
        ),
    )]);
    assert_eq!(
        Vm::default().execute_module_graph("t/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn namespaces_reject_missing_records_sources_and_export_cells() {
    let code =
        crate::compile_module(&crate::parse_module("export const value = 42").unwrap()).unwrap();
    let modules = HashMap::from([("t/main.mjs".to_string(), code)]);
    let mut vm = Vm::default();
    let mut linked = HashMap::new();
    let mut roots = Vec::new();
    assert!(matches!(
        vm.module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots),
        Err(RuntimeError::ModuleResolution(_))
    ));
    linked.insert("t/main.mjs".to_string(), record(false, false));
    assert!(matches!(
        vm.module_namespace(
            "t/main.mjs",
            false,
            &HashMap::new(),
            &mut linked,
            &mut roots
        ),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert!(matches!(
        vm.module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert!(vm.module_namespace_cache.is_empty());
    assert!(roots.is_empty());
    assert!(linked["t/main.mjs"].namespace.is_none());
    let namespace = vm.heap.alloc_module_namespace(Vec::new(), false).unwrap();
    let root = vm.heap.root(namespace).unwrap();
    linked.get_mut("t/main.mjs").unwrap().namespace = Some(namespace);
    assert_eq!(
        vm.module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots),
        Ok(namespace)
    );
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn failed_namespace_allocations_roll_back_nested_cycles_and_allow_retry() {
    let modules = HashMap::from([
        (
            "t/main.mjs".to_string(),
            crate::compile_module(
                &crate::parse_module("export * as child from './child.mjs';").unwrap(),
            )
            .unwrap(),
        ),
        (
            "t/child.mjs".to_string(),
            crate::compile_module(
                &crate::parse_module("export * as parent from './main.mjs';").unwrap(),
            )
            .unwrap(),
        ),
    ]);
    for deferred in [false, true] {
        let mut failures = 0;
        let mut completed = false;
        for extra in (0..16_384).step_by(8) {
            let mut vm = Vm::default();
            let mut linked = HashMap::from([
                ("t/main.mjs".to_string(), record(false, false)),
                ("t/child.mjs".to_string(), record(false, false)),
            ]);
            let mut roots = Vec::new();
            let limit = vm.heap.allow_only(extra);
            match vm.module_namespace("t/main.mjs", deferred, &modules, &mut linked, &mut roots) {
                Ok(_) => completed = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: observed })) => {
                    assert_eq!(observed, limit);
                    failures += 1;
                    assert!(vm.module_namespace_cache.is_empty());
                    assert!(vm.module_deferred_namespace_cache.is_empty());
                    assert!(vm.deferred_namespaces.is_empty());
                    assert!(roots.is_empty());
                    assert!(linked
                        .values()
                        .all(|record| record.namespace.is_none()
                            && record.deferred_namespace.is_none()));
                }
                Err(error) => panic!("namespace headroom {extra}: {error:?}"),
            }
            vm.heap.allow_only(16 * 1024 * 1024);
            let namespace = vm
                .module_namespace("t/main.mjs", deferred, &modules, &mut linked, &mut roots)
                .unwrap();
            // Inspect live namespace cells without triggering deferred evaluation.
            let descriptor = vm
                .heap
                .get_own_property_descriptor(namespace, "child")
                .unwrap()
                .unwrap();
            let child = descriptor.value.unwrap().object_id().unwrap();
            let descriptor = vm
                .heap
                .get_own_property_descriptor(child, "parent")
                .unwrap()
                .unwrap();
            let parent = descriptor.value.unwrap().object_id().unwrap();
            if deferred {
                assert_eq!(parent, vm.module_namespace_cache["t/main.mjs"]);
            } else {
                assert_eq!(parent, namespace);
            }
            for root in roots {
                vm.heap.unroot(root).unwrap();
            }
            if completed {
                break;
            }
        }
        assert!(completed && failures > 0);
    }
}

#[cfg_attr(test, test)]
fn namespace_root_exhaustion_rolls_back_new_placeholders_and_preserves_existing_cache_entries() {
    let modules = HashMap::from([
        (
            "t/main.mjs".to_string(),
            compiled_module("export * as child from './child.mjs';"),
        ),
        (
            "t/child.mjs".to_string(),
            compiled_module("export * as parent from './main.mjs';"),
        ),
        ("t/retained.mjs".to_string(), compiled_module("export {};")),
    ]);
    for deferred in [false, true] {
        // Each namespace has temporary and cache roots, and each namespace
        // export owns a rooted binding cell. A deferred entry also creates
        // its ordinary namespace when this cycle refers back to it.
        let required = if deferred { 9 } else { 6 };
        for remaining in 0..=required {
            let mut vm = Vm::default();
            let mut linked = modules
                .keys()
                .map(|name| (name.clone(), record(false, false)))
                .collect();
            let mut roots = Vec::new();
            let retained = vm
                .module_namespace("t/retained.mjs", false, &modules, &mut linked, &mut roots)
                .unwrap();
            let checkpoint = roots.len();
            vm.heap.allow_root_registrations(remaining);
            let result =
                vm.module_namespace("t/main.mjs", deferred, &modules, &mut linked, &mut roots);
            if remaining < required {
                assert_eq!(
                    result,
                    Err(RuntimeError::Heap(crate::heap::HeapError::IdExhausted))
                );
                assert_eq!(roots.len(), checkpoint);
                assert!(
                    linked["t/main.mjs"].namespace.is_none()
                        && linked["t/main.mjs"].deferred_namespace.is_none()
                );
                assert!(
                    linked["t/child.mjs"].namespace.is_none()
                        && linked["t/child.mjs"].deferred_namespace.is_none()
                );
                assert_eq!(vm.module_namespace_cache.len(), 1);
                assert!(
                    vm.module_deferred_namespace_cache.is_empty()
                        && vm.deferred_namespaces.is_empty()
                );
            } else {
                assert!(result.unwrap().heap == retained.heap);
            }
            assert_eq!(
                vm.module_namespace("t/retained.mjs", false, &modules, &mut linked, &mut roots),
                Ok(retained)
            );
            assert_eq!(
                vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
            for root in roots {
                vm.heap.unroot(root).unwrap();
            }
        }
    }
    let mut vm = Vm::default();
    vm.set_module_source_loader_context(vec!["retained.wasm".into(), "new.wasm".into()]);
    let retained = vm
        .module_source_object("retained.wasm", &HashMap::new())
        .unwrap();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.module_source_object("new.wasm", &HashMap::new()),
        Err(RuntimeError::Heap(crate::heap::HeapError::IdExhausted))
    );
    assert!(!vm.module_source_cache.contains_key("new.wasm"));
    assert!(!vm.module_source_roots.contains_key("new.wasm"));
    assert_eq!(
        vm.module_source_object("retained.wasm", &HashMap::new()),
        Ok(retained)
    );
}

#[cfg_attr(test, test)]
fn source_export_namespaces_roll_back_when_the_host_source_is_missing_or_cannot_be_rooted() {
    let modules = HashMap::from([(
        "t/main.mjs".to_string(),
        compiled_module("import source blob from './blob.wasm'; export {blob};"),
    )]);
    for exhausted in [false, true] {
        let mut vm = Vm::default();
        let mut linked = HashMap::from([("t/main.mjs".to_string(), record(false, false))]);
        let mut roots = Vec::new();
        if exhausted {
            vm.set_module_source_loader_context(vec!["t/blob.wasm".into()]);
            vm.heap.allow_root_registrations(2);
        }
        let error = vm
            .module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots)
            .unwrap_err();
        if exhausted {
            assert_eq!(
                error,
                RuntimeError::Heap(crate::heap::HeapError::IdExhausted)
            );
        } else {
            assert!(
                matches!(error,RuntimeError::TypeError(message) if message.contains("host did not provide a source-phase representation"))
            );
        }
        assert!(roots.is_empty() && vm.module_namespace_cache.is_empty());
        assert!(linked["t/main.mjs"].namespace.is_none());
        assert!(vm.module_source_cache.is_empty());
    }
}

#[cfg_attr(test, test)]
fn source_export_namespaces_preserve_source_identity_at_each_allocation_and_root_boundary() {
    let modules = HashMap::from([(
        "t/main.mjs".to_string(),
        compiled_module("import source blob from './blob.wasm'; export {blob};"),
    )]);
    for remaining in 0..=4 {
        let mut vm = Vm::default();
        vm.set_module_source_loader_context(vec!["t/blob.wasm".into()]);
        let mut linked = HashMap::from([("t/main.mjs".to_string(), record(false, false))]);
        let mut roots = Vec::new();
        vm.heap.allow_root_registrations(remaining);
        let result = vm.module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots);
        if remaining < 4 {
            assert_eq!(result, Err(RuntimeError::Heap(HeapError::IdExhausted)));
            assert!(roots.is_empty() && vm.module_namespace_cache.is_empty());
            assert!(linked["t/main.mjs"].namespace.is_none());
        } else {
            let namespace = result.unwrap();
            let source = vm.heap.get_own(namespace, "blob").unwrap().unwrap();
            assert_eq!(source, Value::Object(vm.module_source_cache["t/blob.wasm"]));
            for root in roots {
                vm.heap.unroot(root).unwrap();
            }
        }
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        if let Some(source) = vm.module_source_cache.get("t/blob.wasm") {
            assert!(vm.heap.contains(*source));
        }
        assert_eq!(
            vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
    let mut next_extra = 0;
    let mut failures = 0;
    let mut completed = false;
    while next_extra <= 16 * 1024 {
        let extra = next_extra;
        let mut vm = Vm::default();
        vm.set_module_source_loader_context(vec!["t/blob.wasm".into()]);
        let mut linked = HashMap::from([("t/main.mjs".to_string(), record(false, false))]);
        let mut roots = Vec::new();
        let limit = vm.heap.allow_only(extra);
        match vm.module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots) {
            Ok(_) => completed = true,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                assert_eq!(actual, limit);
                failures += 1;
                assert!(roots.is_empty() && vm.module_namespace_cache.is_empty());
                assert!(linked["t/main.mjs"].namespace.is_none());
                next_extra = vm.heap.next_allocation_headroom(extra);
            }
            Err(error) => panic!("source namespace headroom {extra}: {error:?}"),
        }
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let namespace = vm
            .module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots)
            .unwrap();
        let source = vm.heap.get_own(namespace, "blob").unwrap().unwrap();
        assert_eq!(source, Value::Object(vm.module_source_cache["t/blob.wasm"]));
        assert_eq!(
            vm.module_source_object("t/blob.wasm", &modules).unwrap(),
            source.object_id().unwrap()
        );
        for root in roots {
            vm.heap.unroot(root).unwrap();
        }
        if completed {
            break;
        }
    }
    assert!(completed && failures > 0);
}

#[cfg_attr(test, test)]
fn export_resolution_reports_missing_default_and_missing_host_dependencies() {
    for (source, star) in [
        ("export {answer} from './missing.mjs';", false),
        ("export * from './missing.mjs';", true),
    ] {
        let modules = HashMap::from([("t/main.mjs".to_string(), compiled_module(source))]);
        let mut resolving = Vec::new();
        assert!(matches!(
            Vm::resolve_export(&modules, "t/main.mjs", "answer", &mut resolving),
            Err(RuntimeError::ModuleResolution(_))
        ));
        assert!(resolving.is_empty());
        if star {
            assert!(matches!(
                Vm::exported_names(&modules, "t/main.mjs", &mut HashSet::new()),
                Err(RuntimeError::ModuleResolution(_))
            ));
            assert!(matches!(
                Vm::resolve_export(&modules, "t/main.mjs", "default", &mut Vec::new()),
                Ok(ExportResolution::Missing)
            ));
        } else {
            let mut vm = Vm::default();
            let mut linked = HashMap::from([("t/main.mjs".to_string(), record(false, false))]);
            let mut roots = Vec::new();
            assert!(matches!(
                vm.module_namespace("t/main.mjs", false, &modules, &mut linked, &mut roots),
                Err(RuntimeError::ModuleResolution(_))
            ));
            assert!(roots.is_empty() && vm.module_namespace_cache.is_empty());
            assert!(linked["t/main.mjs"].namespace.is_none());
        }
    }
}

#[cfg_attr(test, test)]
fn cold_module_source_failures_publish_no_prototype_and_allocation_failures_allow_retry() {
    let mut failures = 0;
    let mut completed = false;
    let mut next_extra = 0;
    while next_extra <= 16 * 1024 * 1024 {
        let extra = next_extra;
        let mut vm = Vm::default();
        vm.set_module_source_loader_context(vec!["first.wasm".into()]);
        let limit = vm.heap.allow_only(extra);
        match vm.module_source_object("first.wasm", &HashMap::new()) {
            Ok(_) => completed = true,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: observed })) => {
                assert_eq!(observed, limit);
                failures += 1;
                assert!(vm.host_module_source_prototype.is_none());
                assert!(vm.module_source_cache.is_empty() && vm.module_source_roots.is_empty());
                assert!(vm.stack.is_empty());
            }
            Err(error) => panic!("source headroom {extra}: {error:?}"),
        }
        if !completed {
            next_extra = vm.heap.next_allocation_headroom(extra);
        }
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let source = vm
            .module_source_object("first.wasm", &HashMap::new())
            .unwrap();
        let prototype = vm.host_module_source_prototype.unwrap();
        assert_eq!(vm.heap.prototype(source).unwrap(), Some(prototype));
        assert_eq!(
            vm.module_source_object("first.wasm", &HashMap::new()),
            Ok(source)
        );
        if completed {
            break;
        }
    }
    assert!(completed && failures > 0);
    let mut vm = Vm::default();
    vm.set_module_source_loader_context(vec!["first.wasm".into()]);
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.module_source_object("first.wasm", &HashMap::new()),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.host_module_source_prototype.is_none());
    assert!(vm.module_source_cache.is_empty() && vm.module_source_roots.is_empty());
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn await_registration_refusals_do_not_publish_orphan_frames_or_consume_serials() {
    for asynchronous in [false, true] {
        for exhaust_serial in [false, true] {
            let mut vm = Vm::default();
            if asynchronous {
                vm.execute_script(
                    &crate::compile(
                        &crate::parse("(async () => {await new Promise(() => {}); return 42;})()")
                            .unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();
            } else {
                let entry = "pending/main.mjs";
                vm.execute_module_graph(
                    entry,
                    &HashMap::from([(
                        entry.to_string(),
                        compiled_module("await new Promise(() => {}); export const answer = 42;"),
                    )]),
                )
                .unwrap();
            }
            let serial = if asynchronous {
                *vm.async_continuations.keys().next().unwrap()
            } else {
                *vm.module_continuations.keys().next().unwrap()
            };
            let promise = *vm
                .promises
                .iter()
                .find(|(_, record)| {
                    record.reactions.iter().any(|reaction| match reaction {
                        PromiseReaction::ModuleAwait { continuation } => {
                            !asynchronous && *continuation == serial
                        }
                        PromiseReaction::AsyncAwait { continuation } => {
                            asynchronous && *continuation == serial
                        }
                        _ => false,
                    })
                })
                .unwrap()
                .0;
            // Transfer an actual compiled suspension out of its owner. Remove
            // that registration's reaction before testing a refused handoff.
            vm.promises
                .get_mut(&promise)
                .unwrap()
                .reactions
                .retain(|reaction| match reaction {
                    PromiseReaction::ModuleAwait { continuation } => {
                        asynchronous || *continuation != serial
                    }
                    PromiseReaction::AsyncAwait { continuation } => {
                        !asynchronous || *continuation != serial
                    }
                    _ => true,
                });
            let ordinary = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
            let ordinary_root = vm.heap.root(ordinary).unwrap();
            let input = if exhaust_serial { promise } else { ordinary };
            let next = if asynchronous {
                if exhaust_serial {
                    vm.next_async_continuation = u64::MAX;
                }
                vm.next_async_continuation
            } else {
                if exhaust_serial {
                    vm.next_module_continuation = u64::MAX;
                }
                vm.next_module_continuation
            };
            let outcome = if asynchronous {
                let frame = vm.async_continuations.remove(&serial).unwrap();
                vm.suspend_async_await(frame, input)
            } else {
                let frame = vm.module_continuations.remove(&serial).unwrap();
                vm.suspend_module_await(frame, input)
            };
            assert_eq!(
                outcome,
                Err(if exhaust_serial {
                    RuntimeError::InstructionLimit
                } else {
                    RuntimeError::TypeError("invalid await Promise".into())
                })
            );
            assert!(vm.module_continuations.is_empty() && vm.async_continuations.is_empty());
            assert_eq!(
                if asynchronous {
                    vm.next_async_continuation
                } else {
                    vm.next_module_continuation
                },
                next
            );
            assert!(vm.promise_jobs.is_empty());
            assert_eq!(
                vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
            vm.heap.unroot(ordinary_root).unwrap();
        }
    }
}

impl crate::Vm {
    /// Runs retained unit contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_module_linking_contracts() {
        debugger_resume_preserves_an_unsettled_top_level_await();
        paused_and_awaited_modules_preserve_error_materialization_refusals();
        deferred_namespace_imports_preserve_lazy_evaluation_and_allocation_refusals();
        deferred_namespace_reflection_preserves_the_real_evaluation_error();
        source_import_resolution_preserves_real_getter_allocation_errors();
        changed_host_registry_errors_preserve_deferred_records_and_observer_roots();
        continuation_exhaustion_at_actual_second_await_restores_ambient_execution();
        dependency_namespace_settlement_refusals_preserve_the_retained_module_graph();
        module_scope_cleanup_retains_graph_owned_export_cells();
        source_export_namespaces_preserve_source_identity_at_each_allocation_and_root_boundary();
        export_resolution_reports_missing_default_and_missing_host_dependencies();
        source_export_namespaces_roll_back_when_the_host_source_is_missing_or_cannot_be_rooted();
        cold_module_source_failures_publish_no_prototype_and_allocation_failures_allow_retry();
        await_registration_refusals_do_not_publish_orphan_frames_or_consume_serials();
        namespace_root_exhaustion_rolls_back_new_placeholders_and_preserves_existing_cache_entries(
        );
        namespace_and_deferred_walks_reject_invalid_requests_and_missing_execution_contexts();
        module_turns_separate_a_debugger_pause_from_return_await_and_failure();
        code_that_is_not_a_generator_only_returns_or_awaits();
        an_async_generator_resumption_returns_yields_or_awaits();
        a_declaration_prefix_must_suspend_exactly_at_the_evaluation_entry();
        a_rejected_await_resumes_in_its_handler_returns_or_fails();
        imports_are_named_in_errors_the_way_they_are_spelled();
        a_frame_carries_on_as_its_thrown_value_was_resolved();
        a_module_only_counts_as_reachable_when_every_request_on_the_way_resolves();
        the_cycle_root_of_a_dependency_is_the_waiting_parent_it_reaches();
        a_rejected_await_is_caught_where_the_frame_left_off();
        namespaces_reject_missing_records_sources_and_export_cells();
        failed_namespace_allocations_roll_back_nested_cycles_and_allow_retry();
    }
}

#[cfg_attr(test, test)]
fn paused_and_awaited_modules_preserve_error_materialization_refusals() {
    let script = |source| crate::compile(&crate::parse(source).unwrap()).unwrap();
    let reuse = script("21 + 21");
    for awaiting in [false, true] {
        let source = if awaiting {
            "await gate.promise; null.value;"
        } else {
            "null.value;"
        };
        let code = crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
        let offset = code
            .instructions()
            .find(|instruction| instruction.offset >= code.module_evaluate_entry.unwrap() as usize)
            .unwrap()
            .offset as u32;
        let graph = HashMap::from([("refusal/entry.mjs".into(), code)]);
        let mut vm = Vm::default();
        if awaiting {
            vm.execute_script(&script("globalThis.gate = Promise.withResolvers();"))
                .unwrap();
            vm.execute_module_graph("refusal/entry.mjs", &graph)
                .unwrap();
            vm.execute_script(&script("gate.resolve();")).unwrap();
        } else {
            assert!(matches!(
                vm.execute_module_graph_until_debugger_pause("refusal/entry.mjs", &graph, offset),
                Ok(VmDebuggerExecutionState::Paused { .. })
            ));
        }
        let limit = vm.heap.allow_only(0);
        let result = if awaiting {
            vm.run_promise_jobs().map(|_| ())
        } else {
            vm.resume_debugger_module_execution().map(|_| ())
        };
        assert_eq!(
            result,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.module_graph.is_some() && vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn deferred_namespace_reflection_preserves_the_real_evaluation_error() {
    type Operation = fn(&mut Vm, ObjectId) -> Result<(), RuntimeError>;
    let operations: &[Operation] = &[
        |vm, object| {
            vm.object_get_own_property(object, &"answer".into())
                .map(|_| ())
        },
        |vm, object| {
            vm.object_define_own_property(
                object,
                "answer".into(),
                PropertyDescriptor::data(Value::Number(42.0), true, true, true),
            )
            .map(|_| ())
        },
        |vm, object| vm.object_delete(object, &"answer".into()).map(|_| ()),
        |vm, object| vm.object_own_property_keys(object).map(|_| ()),
        |vm, object| vm.has_property(object, &"answer".into()).map(|_| ()),
    ];
    let graph = HashMap::from([
        (
            "deferred/entry.mjs".into(),
            crate::compile_module(
                &crate::parse_module("import defer * as ns from './dep.mjs'; globalThis.ns = ns;")
                    .unwrap(),
            )
            .unwrap(),
        ),
        (
            "deferred/dep.mjs".into(),
            crate::compile_module(
                &crate::parse_module("throw 7; export const answer = 42;").unwrap(),
            )
            .unwrap(),
        ),
    ]);
    let reuse = crate::compile(&crate::parse("21 + 21").unwrap()).unwrap();
    for operation in operations {
        let mut vm = Vm::default();
        vm.execute_module_graph("deferred/entry.mjs", &graph)
            .unwrap();
        let global = vm.global("globalThis").unwrap();
        let namespace = vm
            .get_property(&global, &"ns".into())
            .unwrap()
            .object_id()
            .unwrap();
        assert_eq!(
            operation(&mut vm, namespace),
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        );
        assert!(vm.module_graph.is_some() && vm.stack.is_empty());
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn source_import_resolution_preserves_real_getter_allocation_errors() {
    let script = |source| crate::compile(&crate::parse(source).unwrap()).unwrap();
    let mut vm = Vm::default();
    vm.set_module_source_loader_context(vec!["blob.wasm".into()]);
    vm.execute_script(&script("import.source('./blob.wasm').then(source => {Object.defineProperty(Object.getPrototypeOf(source), 'then', {get() {null.value;}, configurable:true});});")).unwrap();
    vm.run_promise_jobs().unwrap();
    let promise = vm.new_promise().unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.dynamic_import_source(promise, "./blob.wasm", ModuleType::JavaScript),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn module_scope_cleanup_retains_graph_owned_export_cells() {
    let code =
        crate::compile_module(&crate::parse_module("export let answer = 42;").unwrap()).unwrap();
    let modules = HashMap::from([("scope.mjs".to_string(), code.clone())]);
    let mut vm = Vm::default();
    vm.execute_module_graph("scope.mjs", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    let cells = vm.module_graph.as_ref().unwrap().linked["scope.mjs"]
        .cells
        .clone();
    assert!(!cells.is_empty());
    vm.enter_module_record(&code, cells.clone());
    vm.reset_scope(&code, 0);
    assert_eq!(vm.cells, cells);
    assert!(vm.bindings.iter().all(Option::is_none));
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(42.0))
    );
    vm.finish_root_execution(Ok(Value::Number(42.0))).unwrap();
    assert_eq!(
        vm.get_property(&namespace, &"answer".into()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn changed_host_registry_errors_preserve_deferred_records_and_observer_roots() {
    let module = |source| crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
    let original = HashMap::from([
        (
            "entry.mjs".to_string(),
            module("import defer * as ns from './dep.mjs'; export {ns};"),
        ),
        (
            "dep.mjs".to_string(),
            module("import './entry.mjs'; export const answer = 42;"),
        ),
    ]);
    let mut vm = Vm::default();
    vm.execute_module_graph("entry.mjs", &original).unwrap();
    let mut changed = original.clone();
    changed.insert("entry.mjs".into(), module("import '../outside.mjs';"));
    vm.set_module_loader_context("entry.mjs", changed.clone());
    let linked = &vm.module_graph.as_ref().unwrap().linked;
    for result in [
        Vm::gather_async_dependencies("entry.mjs", &changed, linked, &mut HashSet::new())
            .map(|_| ()),
        Vm::ready_for_sync_execution("entry.mjs", &changed, linked, &mut HashSet::new())
            .map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(RuntimeError::ModuleResolution(_))),
            "{result:?}"
        );
    }
    let object = vm
        .execute_script(&crate::compile(&crate::parse("({answer:42})").unwrap()).unwrap())
        .unwrap();
    let observer_root = vm.result_root;
    assert!(matches!(
        vm.evaluate_module_sync("dep.mjs"),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(vm.result_root, observer_root);
    assert!(vm.module_graph.is_some() && vm.stack.is_empty());
    vm.set_module_loader_context("entry.mjs", original);
    vm.evaluate_module_sync("dep.mjs").unwrap();
    assert_eq!(vm.result_root, observer_root);
    assert_eq!(
        vm.heap.get(object.object_id().unwrap(), "answer").unwrap(),
        Value::Number(42.0)
    );
    assert_eq!(
        vm.execute_script(&crate::compile(&crate::parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn continuation_exhaustion_at_actual_second_await_restores_ambient_execution() {
    let script = |source| crate::compile(&crate::parse(source).unwrap()).unwrap();
    let reuse = script("21 + 21");
    for source in [
        "(async function() {await Promise.resolve(); return 42;})()",
        "(async function*() {await Promise.resolve(); return 42;})().next()",
    ] {
        let mut vm = Vm::default();
        vm.execute_script(&script("Promise; (async function*() {})();"))
            .unwrap();
        vm.next_async_continuation = u64::MAX;
        assert_eq!(
            vm.execute_script(&script(source)),
            Err(RuntimeError::InstructionLimit)
        );
        assert!(vm.stack.is_empty() && vm.async_continuations.is_empty());
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
    for generator in [false, true] {
        let source = if generator {
            "globalThis.gate = Promise.withResolvers(); globalThis.generator = (async function*() {await gate.promise; await Promise.resolve(); return 42;})(); globalThis.answer = generator.next();"
        } else {
            "globalThis.gate = Promise.withResolvers(); globalThis.answer = (async () => {await gate.promise; await Promise.resolve(); return 42;})();"
        };
        let mut vm = Vm::default();
        vm.execute_script(&script(source)).unwrap();
        assert!(!vm.async_continuations.is_empty());
        vm.next_async_continuation = u64::MAX;
        vm.execute_script(&script("gate.resolve();")).unwrap();
        assert_eq!(vm.run_promise_jobs(), Err(RuntimeError::InstructionLimit));
        assert!(vm.stack.is_empty() && vm.async_frame_roots.is_empty());
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
    let graph = HashMap::from([(
        "entry.mjs".to_string(),
        crate::compile_module(
            &crate::parse_module("await Promise.resolve(); export const answer = 42;").unwrap(),
        )
        .unwrap(),
    )]);
    let mut vm = Vm {
        next_module_continuation: u64::MAX,
        ..Vm::default()
    };
    assert_eq!(
        vm.execute_module_graph("entry.mjs", &graph),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(vm.module_continuations.is_empty() && vm.stack.is_empty());
    assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));

    let graph = HashMap::from([(
        "second.mjs".to_string(),
        compiled_module("await gate.promise; await Promise.resolve(); export const answer = 42;"),
    )]);
    let mut vm = Vm::default();
    vm.execute_script(&script("globalThis.gate = Promise.withResolvers();"))
        .unwrap();
    vm.execute_module_graph("second.mjs", &graph).unwrap();
    vm.next_module_continuation = u64::MAX;
    vm.execute_script(&script("gate.resolve();")).unwrap();
    assert_eq!(vm.run_promise_jobs(), Err(RuntimeError::InstructionLimit));
    assert!(vm.module_graph.is_some() && vm.stack.is_empty());
    assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));

    let graph = HashMap::from([(
        "entry.mjs".to_string(),
        crate::compile_module(&crate::parse_module("await gate.promise; throw sentinel;").unwrap())
            .unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.execute_script(&script(
        "globalThis.gate = Promise.withResolvers(); globalThis.sentinel = {};",
    ))
    .unwrap();
    // The initial unresolved await installs a genuine continuation without
    // draining it; root refusal is introduced only after that registration.
    vm.execute_module_graph("entry.mjs", &graph).unwrap();
    assert!(!vm.module_continuations.is_empty());
    vm.execute_script(&script("gate.resolve();")).unwrap();
    // Retaining the module's thrown object consumes the first registration;
    // refuse the subsequent graph-owned error registration.
    vm.heap.allow_root_registrations(1);
    assert_eq!(
        vm.run_promise_jobs(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(vm.module_graph.is_some());
    assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn dependency_namespace_settlement_refusals_preserve_the_retained_module_graph() {
    fn script(source: &str) -> Bytecode {
        crate::compile(&crate::parse(source).unwrap()).unwrap()
    }
    for parent in [false, true] {
        let requested = if parent { "parent" } else { "dep" };
        let root = if parent {
            "import './parent.mjs'; await hold.promise;"
        } else {
            "import './dep.mjs'; await hold.promise;"
        };
        let dependency = "await gate.promise; export const answer = 42;";
        let mut modules = HashMap::from([
            (
                "settlement/root.mjs".into(),
                crate::compile_module(&crate::parse_module(root).unwrap()).unwrap(),
            ),
            (
                "settlement/dep.mjs".into(),
                crate::compile_module(&crate::parse_module(dependency).unwrap()).unwrap(),
            ),
        ]);
        if parent {
            modules.insert(
                "settlement/parent.mjs".into(),
                crate::compile_module(
                    &crate::parse_module("export {answer} from './dep.mjs';").unwrap(),
                )
                .unwrap(),
            );
        }
        let import_source = format!("import('./{requested}.mjs')");
        let import_code = script(import_source.as_str());
        let mut completed = false;
        let mut next_extra = 0;
        let mut failures = 0;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            vm.execute_script(&script("ReferenceError; globalThis.gate = Promise.withResolvers(); globalThis.hold = Promise.withResolvers();")).unwrap();
            vm.set_module_loader_context("settlement/root.mjs", modules.clone());
            vm.execute_module_graph("settlement/root.mjs", &modules)
                .unwrap();
            let promise = vm
                .execute_script(&import_code)
                .unwrap()
                .object_id()
                .unwrap();
            let root = vm.heap.root(promise).unwrap();
            vm.run_promise_jobs().unwrap();
            assert!(matches!(
                vm.promises[&promise].status,
                PromiseStatus::Pending
            ));
            vm.execute_script(&script("gate.resolve();")).unwrap();
            let limit = vm.heap.allow_only(extra);
            match vm.run_promise_jobs() {
                Ok(()) => {
                    let PromiseStatus::Fulfilled(namespace) = &vm.promises[&promise].status else {
                        panic!("the completed dependency must fulfill its namespace import");
                    };
                    let namespace = namespace.clone();
                    vm.heap.allow_only(16 * 1024 * 1024);
                    assert_eq!(
                        vm.get_property(&namespace, &"answer".into()).unwrap(),
                        Value::Number(42.0)
                    );
                    completed = true;
                }
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit);
                    failures += 1;
                    next_extra = vm.heap.next_allocation_headroom(extra);
                }
                Err(error) => panic!("parent={parent}, headroom {extra}: {error:?}"),
            }
            assert!(
                vm.module_graph.is_some(),
                "a refused namespace settlement lost the module graph"
            );
            assert!(vm.stack.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(
                vm.execute_script(&script("21 + 21")),
                Ok(Value::Number(42.0))
            );
            vm.heap.unroot(root).unwrap();
            if completed {
                break;
            }
        }
        assert!(completed && failures > 0);
    }
}

#[cfg_attr(test, test)]
fn deferred_namespace_imports_preserve_lazy_evaluation_and_allocation_refusals() {
    let script = |source: &str| crate::compile(&crate::parse(source).unwrap()).unwrap();
    for delayed in [false, true] {
        let mut modules = HashMap::from([(
            "refused/root.mjs".to_string(),
            compiled_module(if delayed {
                "import './dep.mjs'; throw 7; export const answer = 42; export const then = undefined;"
            } else {
                "throw 7; export const answer = 42; export const then = undefined;"
            }),
        )]);
        if delayed {
            modules.insert(
                "refused/dep.mjs".into(),
                compiled_module("await gate.promise; export const answer = 42;"),
            );
        }
        let operation = script("import.defer('./root.mjs')");
        let mut next_extra = 0;
        let mut completed = false;
        let mut refused = 0;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            vm.execute_script(&script(
                "TypeError; globalThis.gate = Promise.withResolvers();",
            ))
            .unwrap();
            vm.set_module_loader_context("refused/caller.mjs", modules.clone());
            let promise = vm.execute_script(&operation).unwrap().object_id().unwrap();
            let root = vm.heap.root(promise).unwrap();
            if delayed {
                vm.run_promise_jobs().unwrap();
                assert!(matches!(
                    vm.promises[&promise].status,
                    PromiseStatus::Pending
                ));
                vm.execute_script(&script("gate.resolve();")).unwrap();
            }
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            let limit = vm.heap.allow_only(extra);
            match vm.run_promise_jobs() {
                Ok(()) => {
                    let PromiseStatus::Fulfilled(namespace) = &vm.promises[&promise].status else {
                        panic!("deferred import evaluated a body before namespace access");
                    };
                    let namespace = namespace.clone();
                    vm.heap.allow_only(16 * 1024 * 1024);
                    assert_eq!(
                        vm.get_property(&namespace, &"then".into()),
                        Ok(Value::Undefined)
                    );
                    assert!(vm
                        .linked_record("refused/root.mjs")
                        .unwrap()
                        .error
                        .is_none());
                    assert_eq!(
                        vm.get_property(&namespace, &"answer".into()),
                        Err(RuntimeError::Thrown(Value::Number(7.0)))
                    );
                    completed = true;
                }
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit);
                    refused += 1;
                    next_extra = vm.heap.next_allocation_headroom(extra);
                }
                Err(error) => {
                    panic!("deferred resolution delayed={delayed}, headroom={extra}: {error:?}")
                }
            }
            assert!(vm.stack.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(
                vm.execute_script(&script("21 + 21")),
                Ok(Value::Number(42.0))
            );
            vm.heap.unroot(root).unwrap();
            if completed {
                break;
            }
        }
        assert!(completed && refused > 0);
    }
}

#[cfg_attr(test, test)]
fn debugger_resume_preserves_an_unsettled_top_level_await() {
    let script = |source: &str| crate::compile(&crate::parse(source).unwrap()).unwrap();
    let mut vm = Vm::default();
    vm.execute_script(&script("globalThis.gate = Promise.withResolvers();"))
        .unwrap();
    let module = compiled_module("await gate.promise; export const answer = 42;");
    let offset = module
        .instructions()
        .find(|instruction| instruction.offset >= module.module_evaluate_entry.unwrap() as usize)
        .unwrap()
        .offset as u32;
    let graph = HashMap::from([("pending/entry.mjs".into(), module)]);
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause("pending/entry.mjs", &graph, offset),
        Ok(VmDebuggerExecutionState::Paused { .. })
    ));
    assert_eq!(
        vm.resume_debugger_module_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    let record = vm.linked_record("pending/entry.mjs").unwrap();
    assert!(record.error.is_none() && record.completion.is_none());
    assert!(!vm.module_continuations.is_empty());
    assert!(vm.stack.is_empty());
    vm.execute_script(&script("gate.resolve();")).unwrap();
    vm.run_promise_jobs().unwrap();
    let slot = graph["pending/entry.mjs"]
        .bindings
        .iter()
        .position(|binding| binding.name == "answer")
        .unwrap();
    let cell = vm.linked_record("pending/entry.mjs").unwrap().cells[&slot];
    assert_eq!(
        vm.heap.get_own(cell, "value").unwrap(),
        Some(Value::Number(42.0))
    );
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );
}
