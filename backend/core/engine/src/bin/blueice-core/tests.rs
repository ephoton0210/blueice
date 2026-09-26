// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn args(flags: &[&str]) -> Result<Args, String> {
    parse_args(flags.iter().map(|s| s.to_string()))
}

#[test]
fn socket_is_required() {
    assert_eq!(args(&[]), Err("--socket <path> is required".to_string()));
}

#[test]
fn static_scope_relation_requires_independent_owner_prerequisites() {
    let flag = "--debugger-static-scope-relation";
    let socket = ["--socket", "/tmp/x.sock"];
    assert_eq!(
        args(&[socket[0], socket[1], flag]),
        Err(format!("{flag} requires --debugger-socket"))
    );
    let mut flags = vec![
        socket[0],
        socket[1],
        "--debugger-socket",
        "/tmp/debugger.sock",
    ];
    flags.push(flag);
    assert_eq!(
        args(&flags),
        Err(format!(
            "{flag} requires --debugger-static-metadata-inventory"
        ))
    );
    flags.insert(flags.len() - 1, "--debugger-static-metadata-inventory");
    assert_eq!(
        args(&flags),
        Err(format!(
            "{flag} requires --debugger-static-metadata-type-inventory"
        ))
    );
    flags.insert(flags.len() - 1, "--debugger-static-metadata-type-inventory");
    assert_eq!(
        args(&flags),
        Err(format!(
            "{flag} requires --debugger-static-metadata-symbol-inventory"
        ))
    );
    flags.insert(
        flags.len() - 1,
        "--debugger-static-metadata-symbol-inventory",
    );
    let parsed = args(&flags).unwrap();
    assert!(parsed.debugger_static_scope_relation);
    assert!(!parsed.debugger_bounded_values);
    assert!(!args(&socket).unwrap().debugger_static_scope_relation);
}

#[test]
fn socket_alone_uses_default_width_height_and_frame_dir() {
    let parsed = args(&["--socket", "/tmp/x.sock"]).unwrap();
    assert_eq!(parsed.socket, PathBuf::from("/tmp/x.sock"));
    assert_eq!(parsed.width, 800.0);
    assert_eq!(parsed.height, 600.0);
    assert_eq!(parsed.frame_dir, None);
    assert_eq!(parsed.gatekeeper_socket, None);
    assert_eq!(parsed.script_socket, None);
    assert_eq!(parsed.script_session_token, None);
    assert_eq!(parsed.debugger_socket, None);
    assert!(!parsed.debugger_bounded_values);
    assert!(!parsed.debugger_static_metadata_inventory);
    assert!(!parsed.debugger_static_metadata_summary);
    assert!(!parsed.debugger_static_metadata_source_inventory);
    assert!(!parsed.debugger_static_metadata_source_provenance);
    assert!(!parsed.debugger_static_metadata_safe_point_span);
    assert!(!parsed.debugger_static_metadata_type_inventory);
    assert!(!parsed.debugger_static_metadata_type_display);
    assert!(!parsed.debugger_static_metadata_symbol_inventory);
    assert!(!parsed.debugger_static_metadata_contract_inventory);
    assert!(!parsed.debugger_static_metadata_contract_display);
    assert!(!parsed.debugger_static_metadata_contract_validation);
    assert!(!parsed.debugger_static_metadata_lowering_summary);
    assert!(!parsed.debugger_static_metadata_symbol_display);
    assert!(!parsed.debugger_static_metadata_symbol_location);
    assert!(!parsed.debugger_static_metadata_symbol_type);
    assert!(!parsed.debugger_static_metadata_symbol_contract);
    assert_eq!(parsed.compiler_socket, None);
    assert_eq!(parsed.compiler_project_profile, None);
    assert_eq!(parsed.inline_bluets_profile, None);
    assert!(!parsed.inline_bluejs);
    assert_eq!(parsed.out_of_process_bluejs_socket, None);
    assert_eq!(parsed.out_of_process_bluejs_token, None);
    assert_eq!(parsed.out_of_process_bluejs_page_script_profile, None);
}

#[test]
fn bounded_values_owner_policy_requires_an_explicit_debugger_socket() {
    assert_eq!(
        args(&["--socket", "/tmp/core.sock", "--debugger-bounded-values"]),
        Err("--debugger-bounded-values requires --debugger-socket".to_string())
    );
    let parsed = args(&[
        "--socket",
        "/tmp/core.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-bounded-values",
    ])
    .unwrap();
    assert!(parsed.debugger_bounded_values);
    assert!(!parsed.debugger_static_metadata_inventory);
}

#[test]
fn script_listener_requires_one_fixed_shape_owner_capability() {
    let base = [
        "--socket",
        "/tmp/core.sock",
        "--script-socket",
        "/tmp/script.sock",
    ];
    assert!(args(&base).is_err());
    assert!(args(&[
        "--socket",
        "/tmp/core.sock",
        "--script-session-token",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ])
    .is_err());
    assert!(args(&[
        "--socket",
        "/tmp/core.sock",
        "--script-socket",
        "/tmp/script.sock",
        "--script-session-token",
        "short",
    ])
    .is_err());
    assert!(args(&[
        "--socket",
        "/tmp/core.sock",
        "--script-socket",
        "/tmp/script.sock",
        "--script-session-token",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ])
    .is_ok());
    assert!(args(&[
        "--socket",
        "/tmp/core.sock",
        "--script-socket",
        "/tmp/script.sock",
        "--script-session-token",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--out-of-process-bluejs-socket",
        "/tmp/child.sock",
        "--out-of-process-bluejs-token",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ])
    .is_err());
}

#[test]
fn every_flag_is_parsed() {
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--width",
        "100",
        "--height",
        "50",
        "--frame-dir",
        "/tmp/frames",
        "--gatekeeper-socket",
        "/tmp/gk.sock",
        "--script-socket",
        "/tmp/script.sock",
        "--script-session-token",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--compiler-socket",
        "/tmp/compiler.sock",
        "--compiler-project-profile",
        "core-closed-fixture-v1",
        "--inline-bluets-profile",
        "core-script-document-text-v1",
    ])
    .unwrap();
    assert_eq!(
        parsed,
        Args {
            socket: PathBuf::from("/tmp/x.sock"),
            width: 100.0,
            height: 50.0,
            frame_dir: Some(PathBuf::from("/tmp/frames")),
            gatekeeper_socket: Some(PathBuf::from("/tmp/gk.sock")),
            script_socket: Some(PathBuf::from("/tmp/script.sock")),
            script_session_token: Some("a".repeat(64)),
            debugger_socket: Some(PathBuf::from("/tmp/debugger.sock")),
            debugger_bounded_values: false,
            debugger_static_metadata_inventory: false,
            debugger_static_metadata_summary: false,
            debugger_static_metadata_source_inventory: false,
            debugger_static_metadata_source_provenance: false,
            debugger_static_metadata_type_inventory: false,
            debugger_static_metadata_type_display: false,
            debugger_static_metadata_symbol_inventory: false,
            debugger_static_metadata_contract_inventory: false,
            debugger_static_metadata_contract_display: false,
            debugger_static_metadata_contract_validation: false,
            debugger_static_metadata_lowering_summary: false,
            debugger_static_metadata_symbol_display: false,
            debugger_static_metadata_symbol_location: false,
            debugger_static_metadata_safe_point_span: false,
            debugger_static_metadata_source_breakpoint: false,
            debugger_static_metadata_source_span_step: false,
            debugger_static_metadata_contract_location: false,
            debugger_static_metadata_symbol_type: false,
            debugger_static_metadata_symbol_contract: false,
            debugger_static_scope_relation: false,
            compiler_socket: Some(PathBuf::from("/tmp/compiler.sock")),
            compiler_project_profile: Some("core-closed-fixture-v1".to_string()),
            compiler_catalog_stdin: false,
            owner_bootstrap_stdin: false,
            inline_bluets_profile: Some("core-script-document-text-v1".to_string()),
            inline_bluejs: false,
            out_of_process_bluejs_socket: None,
            out_of_process_bluejs_token: None,
            out_of_process_bluejs_page_script_profile: None,
        }
    );
}

#[test]
fn inline_javascript_host_is_opt_in_and_cannot_share_a_session_with_bluets() {
    assert!(
        args(&["--socket", "/tmp/x.sock", "--inline-bluejs"])
            .unwrap()
            .inline_bluejs
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--inline-bluejs",
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ]),
        Err("--inline-bluejs cannot be combined with --inline-bluets-profile".to_string())
    );
}

#[test]
fn static_metadata_inventory_is_an_explicit_debugger_owner_opt_in() {
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-static-metadata-inventory",
        ]),
        Err("--debugger-static-metadata-inventory requires --debugger-socket".to_string())
    );
    assert!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
        ])
        .unwrap()
        .debugger_static_metadata_inventory
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-summary",
        ]),
        Err(
            "--debugger-static-metadata-summary requires --debugger-static-metadata-inventory"
                .to_string()
        )
    );
    let summary = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-summary",
    ])
    .unwrap();
    assert!(summary.debugger_static_metadata_inventory);
    assert!(summary.debugger_static_metadata_summary);
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-source-inventory",
        ]),
        Err(
            "--debugger-static-metadata-source-inventory requires --debugger-static-metadata-inventory"
                .to_string()
        )
    );
    let source_inventory = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-summary",
        "--debugger-static-metadata-source-inventory",
    ])
    .unwrap();
    assert!(source_inventory.debugger_static_metadata_inventory);
    assert!(source_inventory.debugger_static_metadata_summary);
    assert!(source_inventory.debugger_static_metadata_source_inventory);
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-source-provenance",
        ]),
        Err(
            "--debugger-static-metadata-source-provenance requires --debugger-static-metadata-source-inventory"
                .to_string()
        )
    );
    let provenance = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-source-inventory",
        "--debugger-static-metadata-source-provenance",
    ])
    .unwrap();
    assert!(provenance.debugger_static_metadata_source_provenance);
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-contract-validation",
        ]),
        Err(
            "--debugger-static-metadata-contract-validation requires --debugger-static-metadata-inventory"
                .to_string()
        )
    );
    let contract_validation = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-contract-validation",
    ])
    .unwrap();
    assert!(contract_validation.debugger_static_metadata_contract_validation);
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-lowering-summary",
        ]),
        Err(
            "--debugger-static-metadata-lowering-summary requires --debugger-static-metadata-inventory"
                .to_string()
        )
    );
    let lowering_summary = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-lowering-summary",
    ])
    .unwrap();
    assert!(lowering_summary.debugger_static_metadata_lowering_summary);
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-symbol-location",
        ]),
        Err(
            "--debugger-static-metadata-symbol-location requires --debugger-static-metadata-inventory"
                .to_string()
        )
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-symbol-location",
        ]),
        Err(
            "--debugger-static-metadata-symbol-location requires --debugger-static-metadata-source-inventory"
                .to_string()
        )
    );
    let symbol_location = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-source-inventory",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-symbol-location",
    ])
    .unwrap();
    assert!(symbol_location.debugger_static_metadata_symbol_location);
    assert_eq!(
        args(&[
            "--socket", "/tmp/x.sock",
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-contract-location",
        ]),
        Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--socket", "/tmp/x.sock",
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-contract-location",
        ]),
        Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-source-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--socket", "/tmp/x.sock",
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-source-inventory",
            "--debugger-static-metadata-contract-location",
        ]),
        Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-contract-inventory".to_string())
    );
    assert!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-source-inventory",
            "--debugger-static-metadata-contract-inventory",
            "--debugger-static-metadata-contract-location",
        ])
        .unwrap()
        .debugger_static_metadata_contract_location
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-symbol-type",
        ]),
        Err(
            "--debugger-static-metadata-symbol-type requires --debugger-static-metadata-inventory"
                .to_string()
        )
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-symbol-type",
        ]),
        Err(
            "--debugger-static-metadata-symbol-type requires --debugger-static-metadata-type-inventory"
                .to_string()
        )
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-type-inventory",
            "--debugger-static-metadata-symbol-type",
        ]),
        Err(
            "--debugger-static-metadata-symbol-type requires --debugger-static-metadata-symbol-inventory"
                .to_string()
        )
    );
    let symbol_type = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-type-inventory",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-symbol-type",
    ])
    .unwrap();
    assert!(symbol_type.debugger_static_metadata_symbol_type);
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-symbol-contract",
        ]),
        Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-symbol-contract",
        ]),
        Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-symbol-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-symbol-inventory",
            "--debugger-static-metadata-symbol-contract",
        ]),
        Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-contract-inventory".to_string())
    );
    let symbol_contract = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-contract-inventory",
        "--debugger-static-metadata-symbol-contract",
    ])
    .unwrap();
    assert!(symbol_contract.debugger_static_metadata_symbol_contract);
}

#[test]
fn out_of_process_javascript_host_requires_an_explicit_complete_trusted_endpoint() {
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--out-of-process-bluejs-socket",
        "/tmp/bluejs-host.sock",
        "--out-of-process-bluejs-token",
        "launcher-issued-capability",
    ])
    .unwrap();
    assert_eq!(
        parsed.out_of_process_bluejs_socket,
        Some(PathBuf::from("/tmp/bluejs-host.sock"))
    );
    assert_eq!(
        parsed.out_of_process_bluejs_token.as_deref(),
        Some("launcher-issued-capability")
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--out-of-process-bluejs-socket",
            "/tmp/bluejs-host.sock",
        ]),
        Err(
            "--out-of-process-bluejs-socket and --out-of-process-bluejs-token must be provided together"
                .to_string()
        )
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--out-of-process-bluejs-token",
            "launcher-issued-capability",
        ]),
        Err(
            "--out-of-process-bluejs-socket and --out-of-process-bluejs-token must be provided together"
                .to_string()
        )
    );
}

#[test]
fn out_of_process_http_profile_is_fixed_and_requires_the_private_host() {
    let profile = script::http_resource_authorizer::CORE_HTTP_PAGE_SCRIPT_FIXTURE_PROFILE;
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--out-of-process-bluejs-page-script-profile",
            profile,
        ]),
        Err(
            "--out-of-process-bluejs-page-script-profile requires the private out-of-process BlueJS host"
                .to_string()
        )
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--out-of-process-bluejs-socket",
            "/tmp/bluejs-host.sock",
            "--out-of-process-bluejs-token",
            "launcher-issued-capability",
            "--out-of-process-bluejs-page-script-profile",
            "https://page.example.test/not-a-profile.js",
        ]),
        Err(
            "--out-of-process-bluejs-page-script-profile must name the fixed core HTTP page-script profile"
                .to_string()
        )
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--out-of-process-bluejs-socket",
            "/tmp/bluejs-host.sock",
            "--out-of-process-bluejs-token",
            "launcher-issued-capability",
            "--out-of-process-bluejs-page-script-profile",
            profile,
        ])
        .unwrap()
        .out_of_process_bluejs_page_script_profile
        .as_deref(),
        Some(profile)
    );
}

#[test]
fn out_of_process_javascript_host_cannot_share_a_page_with_other_executors() {
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--inline-bluejs",
            "--out-of-process-bluejs-socket",
            "/tmp/bluejs-host.sock",
            "--out-of-process-bluejs-token",
            "launcher-issued-capability",
        ]),
        Err(
            "--out-of-process-bluejs-socket cannot be combined with --inline-bluejs or --inline-bluets-profile"
                .to_string()
        )
    );
}

#[test]
fn compiler_profile_and_query_listener_are_an_indivisible_startup_pair() {
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--compiler-project-profile",
            "core-closed-fixture-v1",
        ]),
        Err("--compiler-socket requires exactly one compiler startup selector".to_string())
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--compiler-socket",
            "/tmp/compiler.sock",
        ]),
        Err("--compiler-socket requires exactly one compiler startup selector".to_string())
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--compiler-socket",
        "/tmp/compiler.sock",
        "--compiler-project-profile",
        "core-closed-fixture-v1",
    ])
    .unwrap();
    assert_eq!(
        parsed.compiler_project_profile.as_deref(),
        Some("core-closed-fixture-v1")
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--compiler-socket",
        "/tmp/compiler.sock",
        "--compiler-catalog-stdin",
    ])
    .unwrap();
    assert!(parsed.compiler_catalog_stdin);
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--compiler-socket",
        "/tmp/compiler.sock",
        "--owner-bootstrap-stdin",
    ])
    .unwrap();
    assert!(parsed.owner_bootstrap_stdin);
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--compiler-socket",
        "/tmp/compiler.sock",
        "--compiler-project-profile",
        "core-closed-fixture-v1",
        "--owner-bootstrap-stdin",
    ])
    .unwrap();
    assert!(parsed.owner_bootstrap_stdin);
    assert_eq!(
        parsed.compiler_project_profile.as_deref(),
        Some("core-closed-fixture-v1")
    );
    assert!(args(&[
        "--socket",
        "/tmp/x.sock",
        "--compiler-socket",
        "/tmp/compiler.sock",
        "--compiler-catalog-stdin",
        "--compiler-project-profile",
        "core-closed-fixture-v1",
    ])
    .is_err());
}

#[test]
fn compiler_startup_accepts_only_the_compiled_in_closed_profile() {
    let mut catalog = CoreCompilerProjectCatalog::default();
    assert!(
        register_compiler_startup_profile(&mut catalog, "../untrusted-project")
            .unwrap_err()
            .contains("unsupported compiler project profile")
    );
    assert_eq!(catalog.registered_project_count(), 0);
    register_compiler_startup_profile(&mut catalog, "core-closed-fixture-v1").unwrap();
    assert_eq!(catalog.registered_project_count(), 1);
}

#[test]
fn owner_http_policy_rejects_noncanonical_resource_and_origin_before_listener_setup() {
    use blueice_ipc::owner_bootstrap::{
        OwnerHttpOriginRule, OwnerHttpPolicyBootstrap, OwnerHttpResource,
    };

    let mut policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![OwnerHttpResource {
            canonical_url: "https://example.test/app.js".into(),
            integrity: format!("sha256:{}", "0".repeat(64)),
        }],
    };
    assert!(construct_owner_http_page_policy(policy.clone()).is_ok());
    policy.resources[0].canonical_url = "https://example.test/app.js?query=1".into();
    assert!(construct_owner_http_page_policy(policy.clone()).is_err());
    policy.resources[0].canonical_url = "https://example.test/app.js".into();
    policy.origin_rule = OwnerHttpOriginRule::ExactOrigin("https://example.test/path".into());
    assert!(construct_owner_http_page_policy(policy).is_err());
}

#[test]
fn compiler_listener_does_not_unlink_an_active_owner_selected_endpoint() {
    let path = PathBuf::from("/tmp").join(format!(
        "blueice-core-active-compiler-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener = UnixListener::bind(&path).unwrap();

    let error = bind_compiler_listener(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert!(path.exists(), "the active endpoint must remain reachable");

    drop(listener);
    let _ = std::fs::remove_file(path);
}

#[test]
fn script_listener_is_owner_only_and_preserves_occupied_paths() {
    let path = PathBuf::from("/tmp").join(format!(
        "blueice-core-script-endpoint-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"keep this file").unwrap();
    assert_eq!(
        bind_script_listener(&path).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
    std::fs::remove_file(&path).unwrap();

    let listener = bind_script_listener(&path).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        bind_script_listener(&path).unwrap_err().kind(),
        io::ErrorKind::AddrInUse
    );
    drop(listener);
    remove_owned_socket_if_owned(&path);
}

#[test]
fn static_metadata_safe_point_span_requires_explicit_owner_prerequisites() {
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-static-metadata-safe-point-span"
        ]),
        Err("--debugger-static-metadata-safe-point-span requires --debugger-socket".to_string())
    );
    assert_eq!(
        args(&[
            "--socket", "/tmp/x.sock", "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-safe-point-span",
        ]),
        Err("--debugger-static-metadata-safe-point-span requires --debugger-static-metadata-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--socket", "/tmp/x.sock", "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory", "--debugger-static-metadata-safe-point-span",
        ]),
        Err("--debugger-static-metadata-safe-point-span requires --debugger-static-metadata-source-inventory".to_string())
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-source-inventory",
        "--debugger-static-metadata-safe-point-span",
    ])
    .unwrap();
    assert!(parsed.debugger_static_metadata_safe_point_span);
}

#[test]
fn static_metadata_source_breakpoint_requires_independent_owner_prerequisites() {
    let flag = "--debugger-static-metadata-source-breakpoint";
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", flag]),
        Err(format!("{flag} requires --debugger-socket"))
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            flag,
        ]),
        Err(format!(
            "{flag} requires --debugger-static-metadata-inventory"
        ))
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            flag,
        ]),
        Err(format!(
            "{flag} requires --debugger-static-metadata-source-inventory"
        ))
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-source-inventory",
        flag,
    ])
    .unwrap();
    assert!(parsed.debugger_static_metadata_source_breakpoint);
    assert!(!parsed.debugger_static_metadata_safe_point_span);
}

#[test]
fn static_metadata_source_span_step_requires_separate_owner_prerequisites() {
    let flag = "--debugger-static-metadata-source-span-step";
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", flag]),
        Err(format!("{flag} requires --debugger-socket"))
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            flag,
        ]),
        Err(format!(
            "{flag} requires --debugger-static-metadata-safe-point-span"
        ))
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-source-inventory",
        "--debugger-static-metadata-safe-point-span",
        flag,
    ])
    .unwrap();
    assert!(parsed.debugger_static_metadata_source_span_step);
}

#[test]
fn a_flag_missing_its_value_is_an_error() {
    assert_eq!(
        args(&["--socket"]),
        Err("--socket requires a value".to_string())
    );
}

#[test]
fn a_non_numeric_width_is_an_error() {
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", "--width", "not-a-number"]),
        Err("--width must be a number".to_string())
    );
}

#[test]
fn a_non_numeric_height_is_an_error() {
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", "--height", "not-a-number"]),
        Err("--height must be a number".to_string())
    );
}

#[test]
fn an_unrecognized_flag_is_an_error() {
    assert_eq!(
        args(&["--bogus"]),
        Err("unrecognized argument: --bogus".to_string())
    );
}
