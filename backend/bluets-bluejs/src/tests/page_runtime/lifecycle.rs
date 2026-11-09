// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lifecycle page-realm regressions.

use super::*;

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
