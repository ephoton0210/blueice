// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module graph page-realm regressions.

use super::*;

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
