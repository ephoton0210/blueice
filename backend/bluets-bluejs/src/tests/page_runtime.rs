// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct BlueTS classic-script page-realm regressions.

use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};
use std::collections::{BTreeMap, BTreeSet};

const CALLBACK_METHOD_SOURCE: &str = "interface Visitor { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; } let observed: number = 0; function onText(value: string): void { if (value === 'A') { observed += 1; } } function onCount(value: number): void { observed += value; } function dispatch(kind: string, listener: any): void { if (kind === 'text') { listener('A'); } else { listener(2); } } const visitor: Visitor = { visit: dispatch }; visitor.visit('text', onText); visitor.visit('count', onCount); observed;";

fn artifact() -> DirectScript {
    compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const answer: number = 40 + 2; answer;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap()
}

fn origin() -> bluejs::BlueJsPageOrigin {
    bluejs::BlueJsPageOrigin::new("https://example.test").unwrap()
}

fn module_graph() -> DirectModuleGraph {
    let entry = "page:///app/main.ts";
    let dependency = "page:///app/dependency.ts";
    compile_direct_module_graph(
        entry,
        &AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    entry,
                    "import { value } from './dependency'; \
                     export const answer: number = value + 1; answer;",
                ),
                AuthorizedModule::new(dependency, "export const value: number = 41;"),
            ],
            [AuthorizedModuleResolution::new(
                entry,
                "./dependency",
                dependency,
            )],
        )
        .unwrap(),
        CompilerOptions {
            resolver_fingerprint: "page-authorized-resolver-v1".to_string(),
            ..CompilerOptions::default()
        },
    )
    .unwrap()
}

#[test]
fn direct_script_uses_the_page_realms_exact_generation_and_static_metadata() {
    let artifact = artifact();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();

    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    assert_eq!(
        runtime.execute_program(7, attachment.handle).unwrap(),
        bluejs::Value::Number(42.0)
    );
    assert_eq!(runtime.realm_stats(7).unwrap().program_count, 1);
    assert_eq!(
        debug
            .get(runtime.program_registry(), attachment.handle)
            .unwrap()
            .handle(),
        attachment.handle
    );
}

#[test]
fn direct_page_while_runs_zero_and_multiple_iterations() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function sum(value: number): number { let total: number = 0; while (value) { total += value; value -= 1; } return total; } sum(0) + sum(3);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(6.0)
    );
}

#[test]
fn direct_page_callback_method_overloads_run_both_tags_through_one_object_function() {
    let source = CALLBACK_METHOD_SOURCE;
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(3.0)
    );
}

#[test]
fn direct_page_callback_method_debug_frame_expires_after_navigation() {
    let source = CALLBACK_METHOD_SOURCE;
    let dispatch_start = source.find("function dispatch").unwrap();
    let dispatch_end = source.find(" const visitor").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| {
            entry.code_unit.ordinal() > 0
                && attachment
                    .safe_point_map
                    .source_span_for_safe_point(entry.code_unit.ordinal(), entry.bytecode_offset)
                    .is_some_and(|mapped| {
                        (mapped.start_byte, mapped.end_byte) == (dispatch_start, dispatch_end)
                    })
        })
        .expect("dispatch must have a mapped child instruction");
    let point = bluejs::BlueJsSafePoint {
        code_unit: child.code_unit,
        bytecode_offset: child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("the first overloaded method call must pause in dispatch");
    };
    assert_eq!(frame.program(), attachment.handle);
    let mapped = attachment
        .safe_point_map
        .source_span_for_safe_point(frame.code_unit_ordinal(), bytecode_offset)
        .expect("live dispatch instruction must retain source provenance");
    assert_eq!(mapped.source, ENTRY);
    assert_eq!(
        (mapped.start_byte, mapped.end_byte),
        (dispatch_start, dispatch_end)
    );
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_ok());
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
}

#[test]
fn direct_page_optional_dot_read_short_circuits_null_and_undefined_receivers() {
    for nullish in ["null", "undefined"] {
        let source = format!(
            "function choose(flag: boolean): {{ value: number }} | {nullish} {{ return flag ? {{ value: 41 }} : {nullish}; }} const full: {{ value: number }} | {nullish} = choose(true); const empty: {{ value: number }} | {nullish} = choose(false); const first: number = full?.value ?? 0; const second: number = empty?.value ?? 0; first + second;"
        );
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert!(artifact.debug_info.contracts.is_empty());
        assert_eq!(artifact.sources.len(), 1);
        let mut owner = DirectPageRealmOwner::default();
        owner.open_realm(7, origin()).unwrap();
        let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
        assert_eq!(
            owner.execute_program(7, &attachment).unwrap(),
            bluejs::Value::Number(41.0),
            "nullish arm: {nullish}"
        );
    }
}

#[test]
fn direct_page_optional_dot_read_debugger_provenance_expires_on_navigation() {
    let source = "const receiver: { value: number } | null = null; const read: number | undefined = receiver?.value; read;";
    let read_start = source.find("const read").unwrap();
    let read_end = read_start + source[read_start..].find(';').unwrap() + 1;
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let read = attachment
        .provenance
        .iter()
        .find(|entry| (entry.source.start, entry.source.end) == (read_start, read_end))
        .expect("optional read declaration must retain original source provenance");
    let DirectSafePointBinding::Bound(point) = read.safe_point else {
        panic!("optional read declaration must bind a root safe point");
    };
    assert_eq!(point.code_unit.ordinal(), 0);
    let mapped = attachment
        .safe_point_map
        .source_span_for_safe_point(0, point.bytecode_offset)
        .expect("root safe point must map to the original optional read declaration");
    assert_eq!(mapped.source, ENTRY);
    assert_eq!((mapped.start_byte, mapped.end_byte), (read_start, read_end));
    assert_eq!(
        attachment.breakpoint_at_or_after(ENTRY, read_start),
        DirectSafePointBinding::Bound(point)
    );
    assert_eq!(
        runtime
            .execute_program_until_debugger_pause(7, attachment.handle, point)
            .unwrap(),
        bluejs::BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: point.bytecode_offset
        }
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.resume_debugger_execution(7).is_err());
}

#[test]
fn direct_page_try_catches_exact_value_and_restores_outer_binding_in_finally() {
    let source = "function f(caught: number): number { let observed: number = 0; try { throw 7; } catch (caught) { if (caught === 7) { observed += 1; } } finally { if (caught === 99) { observed += 10; } } return observed; } f(99);";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(11.0)
    );
}

#[test]
fn direct_page_try_finally_preserves_or_replaces_normal_return_and_throw() {
    for (source, expected) in [
        ("function f(): number { try { 1; } finally { 2; } return 3; } f();", 3.0),
        ("function f(): number { try { return 5; } finally { 0; } return 0; } f();", 5.0),
        ("function f(): number { try { return 5; } finally { return 9; } return 0; } f();", 9.0),
        ("function f(): number { try { throw 5; } finally { return 9; } return 0; } f();", 9.0),
        ("function f(): number { try { throw 5; } catch (error) { if (error === 5) { return 6; } } finally { 0; } return 0; } f();", 6.0),
    ] {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let mut owner = DirectPageRealmOwner::default();
        owner.open_realm(7, origin()).unwrap();
        let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
        assert_eq!(
            owner.execute_program(7, &attachment).unwrap(),
            bluejs::Value::Number(expected),
            "{source}"
        );
    }
}

#[test]
fn direct_page_try_finally_throw_overrides_return_and_catch_rethrow_escapes() {
    for (source, expected) in [
        ("function f(): number { try { return 5; } finally { throw 9; } return 0; } f();", 9.0),
        ("function f(): void { try { throw 5; } catch (error) { throw 6; } finally { 0; } } f();", 6.0),
    ] {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let mut owner = DirectPageRealmOwner::default();
        owner.open_realm(7, origin()).unwrap();
        let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
        assert!(matches!(
            owner.execute_program(7, &attachment),
            Err(BridgeError::PageRuntime(
                bluejs::BlueJsPageRuntimeError::Runtime(bluejs::RuntimeError::Thrown(
                    bluejs::Value::Number(value)
                ))
            )) if value == expected
        ), "{source}");
    }
}

#[test]
fn direct_page_try_cannot_catch_or_override_a_fatal_instruction_limit() {
    let source = "function spin(): void { while (true) {} } function f(): number { try { spin(); } catch (error) { return 1; } finally { return 2; } return 0; } f();";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime_config = bluejs::BlueJsPageRuntimeConfig::default();
    runtime_config.vm.instruction_budget = 128;
    let mut owner =
        DirectPageRealmOwner::new(runtime_config, DirectDebugRetentionLimits::default()).unwrap();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert!(matches!(
        owner.execute_program(7, &attachment),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::Runtime(bluejs::RuntimeError::InstructionLimit)
        ))
    ));
}

#[test]
fn direct_page_immutable_typeof_guard_runs_both_branches_and_early_completion() {
    for source in [
        "function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { return 1; } else { return value; } } f('Ada') + f(41);",
        "function f(input: string | number): number { const value: string | number = input; if (typeof value !== \"string\") { return value; } return 1; } f('Ada') + f(41);",
        "function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { throw value; } return value; } f(42);",
    ] {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let bluejs::BlueJsProgramV1::Script(program) = &artifact.program else {
            panic!("a guarded function must lower to a direct script");
        };
        assert!(matches!(
            program.body.first(),
            Some(bluejs::Stmt::FunctionDecl(bluejs::Function { body, .. }))
                if matches!(body.as_slice(), [bluejs::Stmt::VarDecl(_, _), bluejs::Stmt::If { .. }, ..])
        ));
        let mut owner = DirectPageRealmOwner::default();
        owner.open_realm(7, origin()).unwrap();
        let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
        assert_eq!(
            owner.execute_program(7, &attachment).unwrap(),
            bluejs::Value::Number(42.0),
            "{source}"
        );
    }
}

#[test]
fn direct_page_typeof_guard_preserves_string_throw_and_rejects_unproven_shapes() {
    let source = "function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { throw value; } return value; } f('bad');";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert!(matches!(
        owner.execute_program(7, &attachment),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::Runtime(bluejs::RuntimeError::Thrown(
                bluejs::Value::String(value)
            ))
        )) if value == "bad"
    ));

    for source in [
        "function takesString(value: string): void {} function f(input: string | number): void { let value: string | number = input; if (typeof value === 'string') { takesString(value); } } f('Ada');",
        "function takesString(value: string): void {} function f(value: string | number): void { if (typeof value === 'string') { takesString(value); } } f('Ada');",
    ] {
        assert!(matches!(
            compile_direct_script(
                ENTRY,
                &MapLoader::from([ModuleSource::new(ENTRY, source)]),
                CompilerOptions::default(),
            ),
            Err(BridgeError::BlueTs(_))
        ), "unproven narrowing must prevent direct execution: {source}");
    }
}

#[test]
fn direct_page_while_runs_inside_an_existing_braced_if_branch() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function count(value: number): number { if (value > 0) { while (value > 0) { value -= 1; } } return value; } count(3);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(0.0)
    );
}

#[test]
fn direct_page_while_preserves_return_and_throw_completion() {
    let returned = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function find(value: number): number { while (value > 0) { if (value === 2) { return value; } value -= 1; } return 0; } find(3);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let thrown = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function fail(): void { while (true) { throw 7; } } fail();",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let returned = owner.attach_script(&returned, 7, &origin()).unwrap();
    assert_eq!(
        owner.execute_program(7, &returned).unwrap(),
        bluejs::Value::Number(2.0)
    );
    let thrown = owner.attach_script(&thrown, 7, &origin()).unwrap();
    assert!(matches!(
        owner.execute_program(7, &thrown),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::Runtime(bluejs::RuntimeError::Thrown(
                bluejs::Value::Number(7.0)
            ))
        ))
    ));
}

#[test]
fn direct_page_endless_while_exhausts_its_vm_instruction_budget() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function spin(): void { while (true) {} } spin();",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime_config = bluejs::BlueJsPageRuntimeConfig::default();
    runtime_config.vm.instruction_budget = 128;
    let mut owner =
        DirectPageRealmOwner::new(runtime_config, DirectDebugRetentionLimits::default()).unwrap();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert!(matches!(
        owner.execute_program(7, &attachment),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::Runtime(bluejs::RuntimeError::InstructionLimit)
        ))
    ));
}

#[test]
fn direct_page_while_loopback_keeps_its_live_source_map_and_nested_frame() {
    let source = "function count(value: number): number { let total: number = 0; while (value > 0) { total += value; value -= 1; } return total; } count(3);";
    let function_end = source.find(" count(3);").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let first_child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| entry.code_unit.ordinal() == 1)
        .expect("the named function must have mapped child instructions");
    let point = bluejs::BlueJsSafePoint {
        code_unit: first_child.code_unit,
        bytecode_offset: first_child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset: mut offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("the loop's named function must pause in its child frame");
    };
    assert_eq!(frame.program(), attachment.handle);
    assert_eq!(frame.code_unit_ordinal(), 1);

    let mut visits = BTreeMap::<u32, usize>::new();
    let mut returned = false;
    for _ in 0..256 {
        let mapped = attachment
            .safe_point_map
            .source_span_for_safe_point(frame.code_unit_ordinal(), offset)
            .expect("each paused loop instruction must retain checked BlueTS provenance");
        assert_eq!(mapped.source, ENTRY);
        assert_eq!((mapped.start_byte, mapped.end_byte), (0, function_end));
        assert_eq!(
            &source[mapped.start_byte..mapped.end_byte],
            &source[..function_end]
        );
        *visits.entry(offset).or_default() += 1;
        match runtime.step_debugger_nested_instruction(frame).unwrap() {
            bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
                frame: same_frame,
                bytecode_offset,
            } => {
                assert_eq!(same_frame, frame);
                offset = bytecode_offset;
            }
            bluejs::BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            state => panic!("unexpected nested loop state: {state:?}"),
        }
    }
    assert!(
        returned,
        "the finite loop must return within the step bound"
    );
    assert!(
        visits.values().any(|count| *count >= 2),
        "a loop-back instruction must revisit the same mapped safe point"
    );
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(bluejs::BlueJsPageDebuggerExecutionState::Completed)
    );

    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame: second_frame,
        ..
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("a new invocation must pause independently");
    };
    assert_ne!(second_frame, frame);
    assert!(matches!(
        runtime.resume_debugger_nested_execution(second_frame),
        Ok(bluejs::BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(bluejs::BlueJsPageDebuggerExecutionState::Completed)
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime
        .step_debugger_nested_instruction(second_frame)
        .is_err());
}

#[test]
fn direct_page_try_nested_safe_points_expire_with_the_page_generation() {
    let source = "function f(): number { try { throw 1; } catch (caught) { if (caught === 1) { return 2; } } finally { 3; } return 0; } f();";
    let function_end = source.find(" f();").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| entry.code_unit.ordinal() == 1)
        .expect("the try function must have mapped nested instructions");
    let point = bluejs::BlueJsSafePoint {
        code_unit: child.code_unit,
        bytecode_offset: child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset: mut offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("the try function must pause in its nested frame");
    };
    assert_eq!(frame.program(), attachment.handle);
    assert_eq!(frame.code_unit_ordinal(), 1);
    let mut visited = BTreeSet::new();
    let mut returned = false;
    for _ in 0..256 {
        let mapped = attachment
            .safe_point_map
            .source_span_for_safe_point(frame.code_unit_ordinal(), offset)
            .expect("every paused try instruction must retain original BlueTS provenance");
        assert_eq!(mapped.source, ENTRY);
        assert_eq!((mapped.start_byte, mapped.end_byte), (0, function_end));
        visited.insert(offset);
        match runtime.step_debugger_nested_instruction(frame).unwrap() {
            bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
                frame: same_frame,
                bytecode_offset,
            } => {
                assert_eq!(same_frame, frame);
                offset = bytecode_offset;
            }
            bluejs::BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            state => panic!("unexpected nested try state: {state:?}"),
        }
    }
    assert!(
        returned,
        "the finite try call must return within the step bound"
    );
    assert!(
        visited.len() > 3,
        "try/catch/finally must map multiple live instructions"
    );
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(bluejs::BlueJsPageDebuggerExecutionState::Completed)
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
    let successor = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    assert_ne!(successor.handle, attachment.handle);
    successor
        .safe_point_map
        .validate_against(runtime.program_registry(), successor.handle)
        .unwrap();
}

#[test]
fn direct_page_typeof_guard_keeps_live_nested_provenance_and_rejects_stale_frames() {
    let source = "function choose(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { return 1; } return value; } choose('Ada');";
    let function_end = source.find(" choose('Ada');").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| entry.code_unit.ordinal() == 1)
        .expect("guarded named function must have a mapped child frame");
    let point = bluejs::BlueJsSafePoint {
        code_unit: child.code_unit,
        bytecode_offset: child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("guarded function must pause in its nested frame");
    };
    let mapped = attachment
        .safe_point_map
        .source_span_for_safe_point(frame.code_unit_ordinal(), bytecode_offset)
        .expect("the live guarded instruction must retain original provenance");
    assert_eq!(mapped.source, ENTRY);
    assert_eq!((mapped.start_byte, mapped.end_byte), (0, function_end));
    assert!(matches!(
        runtime.step_debugger_nested_instruction(frame),
        Ok(bluejs::BlueJsPageDebuggerNestedExecutionState::Paused { .. })
    ));
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
}

#[test]
fn direct_script_keeps_ambient_host_typings_static_without_making_them_a_program_source() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const answer: number = hostAnswer; answer;",
        )]),
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "blueice:///profiles/test/lib.blueice.d.ts",
                "declare const hostAnswer: number;",
            )],
            ..CompilerOptions::default()
        },
    )
    .unwrap();
    assert_eq!(artifact.sources.len(), 2);
    assert_eq!(artifact.debug_info.sources.len(), 2);

    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert_eq!(owner.debug_record_count(), 1);
    assert_eq!(
        attachment.safe_point_map.entries[0].source,
        ENTRY.to_string()
    );
}

#[test]
fn direct_script_lowers_checked_dom_method_lookup_and_text_assignment() {
    let ambient = ModuleSource::new(
        "blueice:///profiles/test-dom/lib.blueice.d.ts",
        "interface Element { textContent: string; }\n\
         interface Document { getElementById(id: string): Element | null; }\n\
         declare const document: Document;",
    );
    let compile = |source| {
        compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                runtime_policy: blueice_bluets::RuntimePolicy::Checked,
                require_declared_global_calls: true,
                ambient_declaration_modules: vec![ambient.clone()],
                ..CompilerOptions::default()
            },
        )
    };
    compile(
        "const node = document.getElementById('target')!;\n\
         node.textContent = 'rendered';",
    )
    .unwrap();
    assert!(compile("document.getElementById(42);").is_err());
    assert!(
        compile("const node = document.getElementById('target')!; node.textContent = 42;").is_err()
    );
}

#[test]
fn realm_owner_prunes_classic_script_metadata_as_part_of_navigation() {
    let artifact = artifact();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();

    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    assert_eq!(owner.debug_record_count(), 1);
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(42.0)
    );

    owner.navigate(7, origin()).unwrap();
    assert_eq!(owner.debug_record_count(), 0);
    assert!(matches!(
        owner.debug_metadata(attachment.handle),
        Err(DirectDebugAttachmentError::BlueJsProgram(
            bluejs::BlueJsProgramDebugError::UnknownProgram
        ))
    ));
    assert!(matches!(
        owner.execute_program(7, &attachment),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. }
        ))
    ));
}

#[test]
fn realm_owner_keeps_only_live_root_symbol_slots_after_navigation() {
    let artifact = artifact();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let first = owner.attach_script(&artifact, 7, &origin()).unwrap();
    let first_slot = owner.debug_root_symbol_slots(first.handle).unwrap()[0];
    assert_eq!(first_slot.program, first.handle);

    owner.navigate(7, origin()).unwrap();
    assert!(matches!(
        owner.debug_root_symbol_slots(first.handle),
        Err(DirectDebugAttachmentError::BlueJsProgram(
            bluejs::BlueJsProgramDebugError::UnknownProgram
        ))
    ));
    let second = owner.attach_script(&artifact, 7, &origin()).unwrap();
    let second_slot = owner.debug_root_symbol_slots(second.handle).unwrap()[0];
    assert_ne!(first_slot.code_unit, second_slot.code_unit);
    assert!(matches!(
        owner.debug_root_symbol_slots(first.handle),
        Err(DirectDebugAttachmentError::BlueJsProgram(
            bluejs::BlueJsProgramDebugError::UnknownProgram
        ))
    ));
}

#[test]
fn provenance_mismatch_discards_the_just_installed_page_program() {
    let mut artifact = artifact();
    artifact.bytecode = bluejs::BlueJsProgramV1::Script(bluejs::Program {
        body: vec![bluejs::Stmt::Expr(bluejs::Expr::Number(7.0))],
    })
    .compile()
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();

    assert!(matches!(
        artifact.attach_in_page_realm(&mut runtime, 7, &origin()),
        Err(BridgeError::ProvenanceAttachment(message))
            if message.contains("bytecode does not match")
    ));
    let stats = runtime.realm_stats(7).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

#[test]
fn direct_module_graph_executes_only_its_attached_page_realm_generations() {
    let graph = module_graph();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();

    let attachment = graph
        .attach_in_page_realm(&mut runtime, 7, &origin())
        .unwrap();
    assert_eq!(attachment.modules.len(), 2);
    assert_eq!(
        attachment.execute_in_page_realm(&mut runtime, 7).unwrap(),
        bluejs::Value::Number(42.0)
    );
    assert_eq!(runtime.realm_stats(7).unwrap().program_count, 2);

    runtime.navigate(7, origin()).unwrap();
    assert!(matches!(
        attachment.execute_in_page_realm(&mut runtime, 7),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. }
        ))
    ));
}

#[test]
fn direct_module_graph_cannot_replace_a_realms_live_canonical_modules() {
    let graph = module_graph();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let attachment = graph
        .attach_in_page_realm(&mut runtime, 7, &origin())
        .unwrap();
    attachment.execute_in_page_realm(&mut runtime, 7).unwrap();

    assert!(matches!(
        graph.attach_in_page_realm(&mut runtime, 7, &origin()),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::DuplicateModuleIdentity(module)
        )) if module == "page:///app/dependency.ts"
    ));
    assert_eq!(runtime.realm_stats(7).unwrap().program_count, 2);

    runtime.navigate(7, origin()).unwrap();
    assert!(graph
        .attach_in_page_realm(&mut runtime, 7, &origin())
        .is_ok());
}

#[test]
fn direct_module_graph_retains_exact_static_metadata_for_every_page_generation() {
    let graph = module_graph();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();

    let attachment = graph
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    assert_eq!(debug.len(), attachment.modules.len());
    for (module_id, module_attachment) in &attachment.modules {
        let retained = debug
            .get(runtime.program_registry(), module_attachment.handle)
            .unwrap();
        assert_eq!(retained.handle(), module_attachment.handle);
        assert_eq!(retained.static_info().sources.len(), 1);
        assert_eq!(retained.static_info().sources[0].module, *module_id);
        assert_eq!(retained.safe_point_map(), &module_attachment.safe_point_map);
    }
    let expected_bytes: usize = attachment
        .modules
        .values()
        .map(|module| {
            debug
                .get(runtime.program_registry(), module.handle)
                .unwrap()
                .retained_payload_bytes()
        })
        .sum();
    assert!(expected_bytes > 0);
    assert_eq!(
        debug
            .retained_payload_bytes_for_live_handles(
                runtime.program_registry(),
                runtime.program_handles(7).unwrap(),
            )
            .unwrap(),
        expected_bytes
    );

    runtime.navigate(7, origin()).unwrap();
    assert_eq!(debug.prune_invalid(runtime.program_registry()), 2);
    assert!(debug.is_empty());
    assert_eq!(
        debug
            .retained_payload_bytes_for_live_handles(
                runtime.program_registry(),
                runtime.program_handles(7).unwrap(),
            )
            .unwrap(),
        0
    );
}

#[test]
fn linked_root_symbol_slots_stay_in_their_own_module_generation() {
    let graph = module_graph();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let attachment = graph
        .attach_in_page_realm(&mut runtime, 7, &origin())
        .unwrap();
    assert_eq!(attachment.modules.len(), 2);
    let mut code_units = Vec::new();
    for (module_id, module_attachment) in &attachment.modules {
        let [slot] = module_attachment
            .live_root_symbol_slots(runtime.program_registry())
            .unwrap()
        else {
            panic!("each linked module has one verified root symbol slot");
        };
        assert_eq!(slot.program, module_attachment.handle);
        assert_eq!(
            slot.code_unit.generation(),
            module_attachment.handle.generation()
        );
        assert_eq!(slot.code_unit.ordinal(), 0);
        let symbol = graph.modules[module_id]
            .debug_info
            .symbols
            .iter()
            .find(|symbol| symbol.kind == blueice_bluets::SymbolKind::Variable)
            .unwrap();
        assert_eq!(slot.symbol_id, symbol.id);
        assert_eq!(symbol.span.module, *module_id);
        code_units.push(slot.code_unit);
    }
    assert_ne!(code_units[0], code_units[1]);

    let modules = attachment.modules.values().collect::<Vec<_>>();
    let mut swapped = (*modules[0]).clone();
    swapped.handle = modules[1].handle;
    swapped.safe_point_map.program_generation = modules[1].handle.generation().as_u64();
    swapped.safe_point_map.entries.clear();
    assert!(matches!(
        swapped.live_root_symbol_slots(runtime.program_registry()),
        Err(BridgeError::ProvenanceAttachment(_))
    ));

    runtime.navigate(7, origin()).unwrap();
    for module_attachment in attachment.modules.values() {
        assert!(matches!(
            module_attachment.live_root_symbol_slots(runtime.program_registry()),
            Err(BridgeError::BlueJsDebug(
                bluejs::BlueJsProgramDebugError::UnknownProgram
            ))
        ));
    }
}

#[test]
fn realm_owner_prunes_every_graph_record_as_part_of_close() {
    let graph = module_graph();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();

    let attachment = owner.attach_module_graph(&graph, 7, &origin()).unwrap();
    assert_eq!(owner.debug_record_count(), 2);
    assert_eq!(
        owner.execute_module_graph(7, &attachment).unwrap(),
        bluejs::Value::Number(42.0)
    );

    assert!(owner.close_realm(7));
    assert_eq!(owner.debug_record_count(), 0);
    assert!(matches!(
        owner.execute_module_graph(7, &attachment),
        Err(BridgeError::PageRuntime(
            bluejs::BlueJsPageRuntimeError::UnknownRealm(7)
        ))
    ));
}

#[test]
fn direct_module_graph_provenance_failure_discards_every_admitted_module() {
    let mut graph = module_graph();
    graph
        .modules
        .get_mut("page:///app/main.ts")
        .unwrap()
        .bytecode = bluejs::BlueJsProgramV1::Module(bluejs::Module {
        body: vec![bluejs::Stmt::Expr(bluejs::Expr::Number(7.0))],
        imports: Vec::new(),
        exports: Vec::new(),
        requests: Vec::new(),
    })
    .compile()
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();

    assert!(matches!(
        graph.attach_in_page_realm(&mut runtime, 7, &origin()),
        Err(BridgeError::ProvenanceAttachment(message))
            if message.contains("bytecode does not match")
    ));
    let stats = runtime.realm_stats(7).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

#[test]
fn direct_module_graph_debug_failure_forgets_records_and_discards_every_program() {
    let graph = module_graph();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::new(DirectDebugRetentionLimits {
        max_programs: 1,
        ..DirectDebugRetentionLimits::default()
    });

    assert!(matches!(
        graph.attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug),
        Err(BridgeError::DebugAttachment(
            DirectDebugAttachmentError::RetentionLimit {
                resource: "programs",
                limit: 1,
            }
        ))
    ));
    assert!(debug.is_empty());
    let stats = runtime.realm_stats(7).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}
