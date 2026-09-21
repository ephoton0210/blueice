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
