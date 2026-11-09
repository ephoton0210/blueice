// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Control flow page-realm regressions.

use super::*;

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
fn direct_page_typeof_guard_preserves_throw_and_executes_mutable_and_parameters() {
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
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
        assert_eq!(
            owner.execute_program(7, &attachment).unwrap(),
            bluejs::Value::Undefined,
            "{source}"
        );
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
