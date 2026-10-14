// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::tests::debug_starts_with;
use super::*;
use crate::{parse, parse_module, BlueJsProgramV1, HostValue, JsString};
use std::error::Error;

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://example.test").unwrap()
}

fn source(name: &str) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new(name, format!("sha256:{name}")).unwrap()
}

fn script(text: &str) -> BlueJsProgramV1 {
    BlueJsProgramV1::Script(parse(text).unwrap())
}

fn module(text: &str) -> BlueJsProgramV1 {
    BlueJsProgramV1::Module(parse_module(text).unwrap())
}

fn install(
    runtime: &mut BlueJsPageRuntime,
    tab: u64,
    name: &str,
    program: &BlueJsProgramV1,
) -> BlueJsProgramHandle {
    runtime
        .install_program(tab, &origin(), source(name), program)
        .unwrap()
}

#[cfg_attr(test, test)]
fn an_origin_is_a_non_empty_text_without_nul() {
    assert_eq!(
        BlueJsPageOrigin::new(""),
        Err(BlueJsPageRuntimeError::InvalidOrigin)
    );
    assert_eq!(
        BlueJsPageOrigin::new("a\0b"),
        Err(BlueJsPageRuntimeError::InvalidOrigin)
    );
    assert_eq!(origin().as_str(), "https://example.test");
}

#[cfg_attr(test, test)]
fn a_runtime_needs_every_limit_to_be_positive() {
    let valid = BlueJsPageRuntimeConfig::default();
    for config in [
        BlueJsPageRuntimeConfig {
            max_realms: 0,
            ..valid
        },
        BlueJsPageRuntimeConfig {
            max_programs_per_realm: 0,
            ..valid
        },
        BlueJsPageRuntimeConfig {
            max_bytecode_bytes_per_realm: 0,
            ..valid
        },
    ] {
        assert!(debug_starts_with(
            &BlueJsPageRuntime::new(config).err(),
            "Some(InvalidConfiguration)"
        ));
    }
}

#[cfg_attr(test, test)]
fn realms_are_opened_once_within_the_limit_and_with_a_valid_vm_configuration() {
    let mut runtime = BlueJsPageRuntime::new(BlueJsPageRuntimeConfig {
        max_realms: 1,
        ..BlueJsPageRuntimeConfig::default()
    })
    .unwrap();
    runtime.open_realm(1, origin()).unwrap();
    assert_eq!(
        runtime.open_realm(1, origin()),
        Err(BlueJsPageRuntimeError::RealmAlreadyExists(1))
    );
    assert_eq!(
        runtime.open_realm(2, origin()),
        Err(BlueJsPageRuntimeError::RealmLimit { limit: 1 })
    );
    assert_eq!(
        runtime.navigate(2, origin()),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert!(!runtime.close_realm(2));
    assert!(runtime.close_realm(1));

    let mut invalid = BlueJsPageRuntimeConfig::default();
    invalid.vm.heap.nursery_capacity = 0;
    let mut runtime = BlueJsPageRuntime::new(invalid).unwrap();
    assert!(debug_starts_with(
        &runtime.open_realm(1, origin()),
        "Err(VmInitialization("
    ));
}

#[cfg_attr(test, test)]
fn a_host_function_and_a_failing_configuration_are_installed_per_realm() {
    // One closure type serves every call, so a single instantiation of the
    // generic method observes the unknown-realm, success and failure paths.
    fn configure(
        runtime: &mut BlueJsPageRuntime,
        tab_id: u64,
        install_object: bool,
    ) -> Result<(), BlueJsPageRuntimeError> {
        runtime.configure_realm_bindings(tab_id, |bindings| {
            if install_object {
                bindings.install_global_object("namespace")?;
                return Ok(());
            }
            bindings.install_global_function("answer", 1, |args: &[HostValue]| {
                Ok(HostValue::Number(args.len() as f64 + 41.0))
            })
        })
    }

    let mut runtime = BlueJsPageRuntime::default();
    assert_eq!(
        configure(&mut runtime, 1, false),
        Err(BlueJsPageRuntimeError::UnknownRealm(1))
    );
    runtime.open_realm(1, origin()).unwrap();
    assert_eq!(configure(&mut runtime, 1, false), Ok(()));
    assert_eq!(configure(&mut runtime, 1, true), Ok(()));
    let handle = install(&mut runtime, 1, "page:///a.js", &script("answer(1)"));
    assert_eq!(runtime.execute_program(1, handle), Ok(Value::Number(42.0)));
    assert_eq!(
        configure(&mut runtime, 1, true),
        Err(BlueJsPageRuntimeError::HostBinding(
            RuntimeError::TypeError("host global name is invalid or already defined".into())
        ))
    );
}

#[cfg_attr(test, test)]
fn installing_needs_a_realm_a_free_slot_and_a_program_that_compiles() {
    let mut runtime = BlueJsPageRuntime::new(BlueJsPageRuntimeConfig {
        max_programs_per_realm: 1,
        ..BlueJsPageRuntimeConfig::default()
    })
    .unwrap();
    assert_eq!(
        runtime.install_program(9, &origin(), source("page:///a.js"), &script("1")),
        Err(BlueJsPageRuntimeError::UnknownRealm(9))
    );
    runtime.open_realm(1, origin()).unwrap();
    // `new.target` is not valid at the top level of a script.
    let broken = BlueJsProgramV1::Script(parse("new.target;").unwrap());
    assert!(debug_starts_with(
        &runtime.install_program(1, &origin(), source("page:///broken.js"), &broken),
        "Err(ProgramRegistry("
    ));
    install(&mut runtime, 1, "page:///a.js", &script("1"));
    assert_eq!(
        runtime.install_program(1, &origin(), source("page:///b.js"), &script("2")),
        Err(BlueJsPageRuntimeError::ProgramLimit {
            tab_id: 1,
            limit: 1
        })
    );
}

#[cfg_attr(test, test)]
fn a_module_identity_is_owned_once_and_released_by_discarding() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let first = install(
        &mut runtime,
        1,
        "page:///m.js",
        &module("export var a = 1;"),
    );
    assert_eq!(
        runtime.install_program(
            1,
            &origin(),
            source("page:///m.js"),
            &module("export var b = 2;")
        ),
        Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
            source("page:///m.js").canonical_module_id().to_string()
        ))
    );
    runtime.discard_program(1, first).unwrap();
    install(
        &mut runtime,
        1,
        "page:///m.js",
        &module("export var b = 2;"),
    );
    assert_eq!(
        runtime.discard_program(2, first),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert_eq!(
        runtime.discard_program(1, first),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 1,
            handle: first
        })
    );
}

#[cfg_attr(test, test)]
fn executing_needs_an_owned_program_in_a_live_realm() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let handle = install(&mut runtime, 1, "page:///a.js", &script("6 * 7"));
    assert_eq!(
        runtime.execute_program(2, handle),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert_eq!(runtime.execute_program(1, handle), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn the_debugger_seam_runs_classic_root_code_only() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let classic = install(
        &mut runtime,
        1,
        "page:///classic.js",
        &script(
            "globalThis.before = 1; function child() { return 2; } globalThis.after = child();",
        ),
    );
    let other = install(&mut runtime, 1, "page:///other.js", &script("3"));
    let esm = install(
        &mut runtime,
        1,
        "page:///esm.js",
        &module("export var a = 1;"),
    );
    let safe_points = runtime.safe_points(1, classic, 256).unwrap();
    let root = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit.ordinal() == 0 && safe_point.bytecode_offset != 0)
        .unwrap();
    let child = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit.ordinal() != 0)
        .unwrap();
    let module_point = runtime.safe_points(1, esm, 64).unwrap()[0];

    assert_eq!(
        runtime.execute_program_until_debugger_pause(2, classic, root),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    let elsewhere = install(&mut runtime, 1, "page:///elsewhere.js", &script("4"));
    runtime.discard_program(1, elsewhere).unwrap();
    assert_eq!(
        runtime.execute_program_until_debugger_pause(1, elsewhere, root),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 1,
            handle: elsewhere
        })
    );
    assert!(debug_starts_with(
        &runtime.execute_program_until_debugger_pause(1, other, root),
        "Err(ProgramRegistry("
    ));
    assert_eq!(
        runtime.execute_program_until_debugger_pause(1, esm, module_point),
        Err(BlueJsPageRuntimeError::DebuggerRootScriptOnly)
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause(1, classic, child),
        Err(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly)
    );
    assert_eq!(runtime.validate_safe_point(1, classic, root), Ok(()));
    assert!(debug_starts_with(
        &runtime.validate_safe_point(1, other, root),
        "Err(ProgramRegistry("
    ));

    // The offset form finds the safe point itself.
    assert_eq!(
        runtime.execute_program_until_debugger_pause_at_root_offset(2, classic, 0),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause_at_root_offset(1, elsewhere, 0),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 1,
            handle: elsewhere
        })
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause_at_root_offset(1, classic, u32::MAX),
        Err(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly)
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause_at_root_offset(
            1,
            classic,
            root.bytecode_offset
        ),
        Ok(BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: root.bytecode_offset
        })
    );
    assert_eq!(
        runtime.resume_debugger_execution(2),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert_eq!(
        runtime.resume_debugger_execution(1),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
}

#[cfg_attr(test, test)]
fn a_module_graph_needs_owned_module_programs_including_its_entry() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let entry = install(
        &mut runtime,
        1,
        "page:///entry.js",
        &module("import { a } from './dep.js'; globalThis.result = a;"),
    );
    let dep = install(
        &mut runtime,
        1,
        "page:///dep.js",
        &module("export var a = 5;"),
    );
    let classic = install(&mut runtime, 1, "page:///classic.js", &script("1"));
    assert_eq!(
        runtime.execute_module_graph(2, entry, vec![entry, dep]),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert_eq!(
        runtime.execute_module_graph(1, entry, vec![entry, classic]),
        Err(BlueJsPageRuntimeError::ProgramShape)
    );
    assert_eq!(
        runtime.execute_module_graph(1, entry, vec![dep]),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 1,
            handle: entry
        })
    );
    assert_eq!(
        runtime.execute_module_graph(1, entry, vec![entry, dep, dep]),
        Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
            source("page:///dep.js").canonical_module_id().to_string()
        ))
    );
    runtime.discard_program(1, classic).unwrap();
    assert_eq!(
        runtime.execute_module_graph(1, entry, vec![entry, classic]),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 1,
            handle: classic
        })
    );
    runtime
        .execute_module_graph(1, entry, vec![entry, dep])
        .unwrap();
}

#[cfg_attr(test, test)]
fn unknown_realms_are_reported_by_every_query() {
    let runtime = BlueJsPageRuntime::default();
    assert_eq!(
        runtime.realm_stats(1),
        Err(BlueJsPageRuntimeError::UnknownRealm(1))
    );
    assert_eq!(
        runtime.program_handles(1),
        Err(BlueJsPageRuntimeError::UnknownRealm(1))
    );
}

#[cfg_attr(test, test)]
fn safe_point_queries_need_an_owned_program() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let handle = install(&mut runtime, 1, "page:///a.js", &script("1"));
    let point = runtime.safe_points(1, handle, 8).unwrap()[0];
    assert_eq!(
        runtime.safe_points(2, handle, 8),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    assert_eq!(
        runtime.validate_safe_point(2, handle, point),
        Err(BlueJsPageRuntimeError::UnknownRealm(2))
    );
    runtime.discard_program(1, handle).unwrap();
    assert_eq!(
        runtime.safe_points(1, handle, 8),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 1, handle })
    );
    assert_eq!(
        runtime.validate_safe_point(1, handle, point),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 1, handle })
    );
}

#[cfg_attr(test, test)]
fn every_error_describes_itself_and_names_the_error_it_wraps() {
    let handle = {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(1, origin()).unwrap();
        install(&mut runtime, 1, "page:///a.js", &script("1"))
    };
    let registry_error = BlueJsPageRuntime::default()
        .program_registry()
        .get(handle)
        .err()
        .unwrap();
    let wrapping = [
        BlueJsPageRuntimeError::VmInitialization(HeapError::InvalidConfig),
        BlueJsPageRuntimeError::HostBinding(RuntimeError::Unsupported("x")),
        BlueJsPageRuntimeError::ProgramRegistry(registry_error),
        BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported("x")),
    ];
    let plain = [
        BlueJsPageRuntimeError::InvalidConfiguration,
        BlueJsPageRuntimeError::InvalidOrigin,
        BlueJsPageRuntimeError::RealmAlreadyExists(1),
        BlueJsPageRuntimeError::UnknownRealm(1),
        BlueJsPageRuntimeError::RealmLimit { limit: 2 },
        BlueJsPageRuntimeError::OriginMismatch,
        BlueJsPageRuntimeError::ProgramLimit {
            tab_id: 1,
            limit: 2,
        },
        BlueJsPageRuntimeError::BytecodeLimit {
            tab_id: 1,
            limit: 2,
        },
        BlueJsPageRuntimeError::SafePointLimit {
            tab_id: 1,
            limit: 2,
        },
        BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 1, handle },
        BlueJsPageRuntimeError::ProgramShape,
        BlueJsPageRuntimeError::DebuggerRootScriptOnly,
        BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly,
        BlueJsPageRuntimeError::DuplicateModuleIdentity("m".into()),
    ];
    for error in &wrapping {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_some(), "{error:?}");
    }
    for error in &plain {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_none(), "{error:?}");
    }
    let _ = JsString::default();
}

impl BlueJsPageRuntime {
    /// Runs page runtime contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_page_runtime_boundary_contracts() {
        an_origin_is_a_non_empty_text_without_nul();
        a_runtime_needs_every_limit_to_be_positive();
        realms_are_opened_once_within_the_limit_and_with_a_valid_vm_configuration();
        a_host_function_and_a_failing_configuration_are_installed_per_realm();
        installing_needs_a_realm_a_free_slot_and_a_program_that_compiles();
        a_module_identity_is_owned_once_and_released_by_discarding();
        executing_needs_an_owned_program_in_a_live_realm();
        the_debugger_seam_runs_classic_root_code_only();
        a_module_graph_needs_owned_module_programs_including_its_entry();
        unknown_realms_are_reported_by_every_query();
        safe_point_queries_need_an_owned_program();
        every_error_describes_itself_and_names_the_error_it_wraps();
    }
}
