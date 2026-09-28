// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig,
    BlueJsPageRuntimeError, BlueJsProgramDebugError, BlueJsProgramV1, BlueJsSourceIdentity,
    HeapError, HostObjectKey, RuntimeError, Value,
};

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://example.test").unwrap()
}

fn source(name: &str) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new(name, format!("sha256:{name}")).unwrap()
}

#[test]
fn page_origin_validation_preserves_the_authorized_identity() {
    assert_eq!(
        BlueJsPageOrigin::new(""),
        Err(BlueJsPageRuntimeError::InvalidOrigin)
    );
    assert_eq!(
        BlueJsPageOrigin::new("https://example.test\0extra"),
        Err(BlueJsPageRuntimeError::InvalidOrigin)
    );
    assert_eq!(origin().as_str(), "https://example.test");
}

#[test]
fn page_limits_reject_admission_without_losing_a_live_program() {
    for config in [
        BlueJsPageRuntimeConfig {
            max_realms: 0,
            ..BlueJsPageRuntimeConfig::default()
        },
        BlueJsPageRuntimeConfig {
            max_programs_per_realm: 0,
            ..BlueJsPageRuntimeConfig::default()
        },
        BlueJsPageRuntimeConfig {
            max_bytecode_bytes_per_realm: 0,
            ..BlueJsPageRuntimeConfig::default()
        },
    ] {
        assert!(matches!(
            BlueJsPageRuntime::new(config),
            Err(BlueJsPageRuntimeError::InvalidConfiguration)
        ));
    }

    let mut runtime = BlueJsPageRuntime::new(BlueJsPageRuntimeConfig {
        max_realms: 1,
        max_programs_per_realm: 1,
        ..BlueJsPageRuntimeConfig::default()
    })
    .unwrap();
    assert_eq!(
        runtime.navigate(7, origin()),
        Err(BlueJsPageRuntimeError::UnknownRealm(7))
    );
    runtime.open_realm(7, origin()).unwrap();
    assert_eq!(
        runtime.open_realm(7, origin()),
        Err(BlueJsPageRuntimeError::RealmAlreadyExists(7))
    );
    assert_eq!(
        runtime.open_realm(8, origin()),
        Err(BlueJsPageRuntimeError::RealmLimit { limit: 1 })
    );
    let script = BlueJsProgramV1::Script(parse("42").unwrap());
    let first = runtime
        .install_program(7, &origin(), source("page:///first.js"), &script)
        .unwrap();
    assert_eq!(
        runtime.install_program(7, &origin(), source("page:///second.js"), &script),
        Err(BlueJsPageRuntimeError::ProgramLimit {
            tab_id: 7,
            limit: 1
        })
    );
    assert_eq!(runtime.realm_stats(7).unwrap().program_count, 1);
    assert_eq!(runtime.execute_program(7, first), Ok(Value::Number(42.0)));
    runtime.discard_program(7, first).unwrap();
    assert_eq!(
        runtime.discard_program(7, first),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 7,
            handle: first
        })
    );
}

#[test]
fn debugger_admission_requires_the_owning_realm_and_matching_root_kind() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let script = runtime
        .install_program(
            7,
            &origin(),
            source("page:///debug-script.js"),
            &BlueJsProgramV1::Script(parse("function child() { return 1; } child();").unwrap()),
        )
        .unwrap();
    let script_root = runtime
        .safe_points(7, script, 128)
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit.ordinal() == 0)
        .unwrap();
    let wrong_realm = BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
        tab_id: 8,
        handle: script,
    };
    assert_eq!(
        runtime.safe_points(8, script, 128),
        Err(wrong_realm.clone())
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause(8, script, script_root),
        Err(wrong_realm.clone())
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause_at_root_offset(
            8,
            script,
            script_root.bytecode_offset
        ),
        Err(wrong_realm)
    );
    assert_eq!(
        runtime.execute_program_until_debugger_pause_at_root_offset(7, script, u32::MAX),
        Err(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly)
    );
    assert_eq!(
        runtime.execute_program_until_nested_debugger_pause(7, script, script_root),
        Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
    );

    let module = runtime
        .install_program(
            7,
            &origin(),
            source("page:///debug-module.js"),
            &BlueJsProgramV1::Module(
                parse_module("export function child() { return 1; } child();").unwrap(),
            ),
        )
        .unwrap();
    let points = runtime.safe_points(7, module, 128).unwrap();
    let module_root = *points
        .iter()
        .find(|point| point.code_unit.ordinal() == 0)
        .unwrap();
    let module_child = *points
        .iter()
        .find(|point| point.code_unit.ordinal() != 0)
        .unwrap();
    assert_eq!(
        runtime.execute_program_until_debugger_pause(7, module, module_root),
        Err(BlueJsPageRuntimeError::DebuggerRootScriptOnly)
    );
    assert_eq!(
        runtime.execute_program_until_nested_debugger_pause(7, module, module_child),
        Err(BlueJsPageRuntimeError::DebuggerRootScriptOnly)
    );
}

#[test]
fn module_graph_admission_rejects_unowned_nonmodule_and_duplicate_handles() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let script = runtime
        .install_program(
            7,
            &origin(),
            source("page:///graph-script.js"),
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        )
        .unwrap();
    let module = runtime
        .install_program(
            7,
            &origin(),
            source("page:///graph-module.js"),
            &BlueJsProgramV1::Module(parse_module("export const answer = 42;").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_module_graph(8, module, [module]),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 8,
            handle: module
        })
    );
    assert_eq!(
        runtime.execute_module_graph(7, module, [script]),
        Err(BlueJsPageRuntimeError::ProgramShape)
    );
    assert_eq!(
        runtime.execute_module_graph(7, module, [module, module]),
        Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
            "page:///graph-module.js".into()
        ))
    );
    assert_eq!(
        runtime.execute_module_graph(7, module, []),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 7,
            handle: module
        })
    );
}

#[test]
fn page_runtime_errors_preserve_public_messages_and_sources() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let handle = runtime
        .install_program(
            7,
            &origin(),
            source("page:///error.js"),
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        )
        .unwrap();
    let cases = [
        (BlueJsPageRuntimeError::InvalidConfiguration, "invalid BlueJS page runtime configuration", false),
        (BlueJsPageRuntimeError::InvalidOrigin, "page origin identity is empty or contains NUL", false),
        (BlueJsPageRuntimeError::RealmAlreadyExists(7), "page realm for tab 7 already exists", false),
        (BlueJsPageRuntimeError::UnknownRealm(7), "page realm for tab 7 is unavailable", false),
        (BlueJsPageRuntimeError::RealmLimit { limit: 2 }, "page realm limit 2 reached", false),
        (BlueJsPageRuntimeError::OriginMismatch, "script origin does not match its live page realm", false),
        (BlueJsPageRuntimeError::ProgramLimit { tab_id: 7, limit: 2 }, "tab 7 reached program limit 2", false),
        (BlueJsPageRuntimeError::BytecodeLimit { tab_id: 7, limit: 2 }, "tab 7 exceeds bytecode limit 2", false),
        (BlueJsPageRuntimeError::SafePointLimit { tab_id: 7, limit: 2 }, "tab 7 exceeds safe-point limit 2", false),
        (BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 7, handle }, "program is not owned by tab 7", false),
        (BlueJsPageRuntimeError::ProgramShape, "compiled page program has no script or module root", false),
        (BlueJsPageRuntimeError::DebuggerRootScriptOnly, "native debugger continuation supports classic scripts only", false),
        (BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly, "native debugger continuation supports root code-unit safe points only", false),
        (BlueJsPageRuntimeError::DebuggerModuleEntrySafePointOnly, "native debugger module pause requires its first evaluate-body safe point", false),
        (BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly, "native nested debugger pause requires a child code unit", false),
        (BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable, "nested debugger frame is not active in this page realm", false),
        (BlueJsPageRuntimeError::DuplicateModuleIdentity("page:///module.js".into()), "page realm already owns or linked canonical module `page:///module.js`", false),
        (BlueJsPageRuntimeError::VmInitialization(HeapError::InvalidConfig), "cannot initialize page VM: invalid BlueJS heap configuration", true),
        (BlueJsPageRuntimeError::HostBinding(RuntimeError::TypeError("bad host".into())), "cannot install page realm host binding: TypeError: bad host", true),
        (BlueJsPageRuntimeError::ProgramRegistry(BlueJsProgramDebugError::UnknownProgram), "BlueJS program registry rejected page program: BlueJS program handle is stale or unknown", true),
        (BlueJsPageRuntimeError::Runtime(RuntimeError::InstructionLimit), "BlueJS page program failed: BlueJS instruction budget exhausted", true),
    ];
    for (error, message, has_source) in cases {
        assert_eq!(error.to_string(), message, "{error:?}");
        assert_eq!(
            std::error::Error::source(&error).is_some(),
            has_source,
            "{error:?}"
        );
    }
}

#[test]
fn page_registrar_installs_global_and_wrapper_capabilities_in_one_realm() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let text = Rc::new(RefCell::new(String::from("before")));
    let getter_text = Rc::clone(&text);
    let setter_text = Rc::clone(&text);
    runtime
        .configure_realm_bindings(7, |bindings| {
            bindings.install_global_function(
                "hostAnswer",
                0,
                |_args: &[blueice_bluejs::HostValue]| Ok(blueice_bluejs::HostValue::Number(42.0)),
            )?;
            let family = bindings.create_host_object_family()?;
            bindings.install_global_object_factory(
                "makeNode",
                1,
                family,
                |args: &[blueice_bluejs::HostValue]| {
                    let [blueice_bluejs::HostValue::Number(id)] = args else {
                        return Err(blueice_bluejs::HostFunctionError::new(
                            "numeric node ID required",
                        ));
                    };
                    Ok(Some(HostObjectKey::new(7, 3, *id as u64)))
                },
            )?;
            bindings.install_host_object_method(
                family,
                "isOne",
                0,
                |key: HostObjectKey, _args: &[blueice_bluejs::HostValue]| {
                    Ok(blueice_bluejs::HostValue::Bool(
                        key == HostObjectKey::new(7, 3, 1),
                    ))
                },
            )?;
            bindings.install_host_object_pair_method(
                family,
                "appendChild",
                1,
                |parent: HostObjectKey, child: HostObjectKey| {
                    if parent != HostObjectKey::new(7, 3, 1) || child != HostObjectKey::new(7, 3, 2)
                    {
                        return Err(blueice_bluejs::HostFunctionError::new(
                            "unexpected node pair",
                        ));
                    }
                    Ok(())
                },
            )?;
            bindings.install_host_object_accessor(
                family,
                "textContent",
                move |_key: HostObjectKey, _args: &[blueice_bluejs::HostValue]| {
                    Ok(blueice_bluejs::HostValue::String(
                        getter_text.borrow().clone().into(),
                    ))
                },
                move |_key: HostObjectKey, args: &[blueice_bluejs::HostValue]| {
                    let [blueice_bluejs::HostValue::String(value)] = args else {
                        return Err(blueice_bluejs::HostFunctionError::new(
                            "string text required",
                        ));
                    };
                    *setter_text.borrow_mut() = value.to_utf8().unwrap();
                    Ok(blueice_bluejs::HostValue::Undefined)
                },
            )
        })
        .unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///capabilities.js"),
            &BlueJsProgramV1::Script(
                parse(
                    "let parent = makeNode(1); let child = makeNode(2); \
                       parent.textContent = 'after'; \
                       hostAnswer() === 42 && parent.isOne() && !child.isOne() && \
                       parent.appendChild(child) === child && parent.textContent === 'after';",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(runtime.execute_program(7, program), Ok(Value::Bool(true)));
    assert_eq!(&*text.borrow(), "after");
    let other = runtime
        .install_program(
            8,
            &origin(),
            source("page:///other-capabilities.js"),
            &BlueJsProgramV1::Script(parse("typeof hostAnswer + typeof makeNode;").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(8, other),
        Ok(Value::String("undefinedundefined".into()))
    );
}
