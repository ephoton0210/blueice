// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct BlueTS classic-script page-realm regressions.

use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};

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

    runtime.navigate(7, origin()).unwrap();
    assert_eq!(debug.prune_invalid(runtime.program_registry()), 2);
    assert!(debug.is_empty());
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
