// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn args(flags: &[&str]) -> Result<Args, String> {
    parse_args(flags.iter().map(|s| s.to_string()))
}

#[test]
fn bounded_values_owner_policy_requires_an_explicit_debugger_socket() {
    assert_eq!(
        args(&["--debugger-bounded-values"]),
        Err("--debugger-bounded-values requires --debugger-socket".to_string())
    );
    let parsed = args(&[
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-bounded-values",
    ])
    .unwrap();
    assert!(parsed.debugger_bounded_values);
    assert!(!parsed.debugger_static_metadata_inventory);
}

#[test]
fn owner_catalog_requires_a_compiler_endpoint() {
    assert!(args(&["--compiler-catalog-file", "/tmp/catalog.json"]).is_err());
    let parsed = args(&[
        "--compiler-mcp-socket",
        "/tmp/compiler.sock",
        "--compiler-catalog-file",
        "/tmp/catalog.json",
    ])
    .unwrap();
    assert_eq!(
        parsed.compiler_catalog_file,
        Some(PathBuf::from("/tmp/catalog.json"))
    );
    assert!(read_owner_compiler_catalog_file(std::path::Path::new("relative.json")).is_err());
}

#[test]
fn owner_http_policy_requires_the_supervised_page_host() {
    assert!(args(&["--page-http-policy-file", "/tmp/http-policy.json"]).is_err());
    let parsed = args(&[
        "--out-of-process-bluejs",
        "--page-http-policy-file",
        "/tmp/http-policy.json",
    ])
    .unwrap();
    assert_eq!(
        parsed.page_http_policy_file,
        Some(PathBuf::from("/tmp/http-policy.json"))
    );
    assert!(read_owner_http_policy_file(std::path::Path::new("relative.json")).is_err());
}

#[test]
fn owner_http_policy_file_rejects_symlink_and_malformed_content() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!(
        "blueice-owner-http-policy-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let file = base.with_extension("json");
    let link = base.with_extension("link");
    std::fs::write(&file, b"not JSON").unwrap();
    symlink(&file, &link).unwrap();
    assert!(read_owner_http_policy_file(&file).is_err());
    assert!(read_owner_http_policy_file(&link).is_err());
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn owner_catalog_file_rejects_symlink_and_malformed_content() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!(
        "blueice-owner-catalog-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let file = base.with_extension("json");
    let link = base.with_extension("link");
    std::fs::write(&file, b"not JSON").unwrap();
    symlink(&file, &link).unwrap();
    assert!(read_owner_compiler_catalog_file(&file).is_err());
    assert!(read_owner_compiler_catalog_file(&link).is_err());
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn no_flags_uses_the_default_rendezvous_socket_and_default_size() {
    let parsed = args(&[]).unwrap();
    assert_eq!(parsed.rendezvous_socket, default_rendezvous_socket_path());
    assert_eq!(parsed.control_socket, default_control_socket_path());
    assert_eq!(parsed.width, 800.0);
    assert_eq!(parsed.height, 600.0);
    assert_eq!(parsed.frame_dir, None);
    assert_eq!(parsed.gatekeeper_socket, None);
    assert!(!parsed.out_of_process_bluejs);
    assert_eq!(parsed.compiler_mcp_socket, None);
    assert_eq!(parsed.compiler_catalog_file, None);
    assert_eq!(parsed.page_http_policy_file, None);
    assert_eq!(parsed.debugger_socket, None);
    assert!(!parsed.debugger_static_metadata_inventory);
    assert!(!parsed.debugger_static_metadata_summary);
    assert!(!parsed.debugger_static_metadata_source_inventory);
    assert!(!parsed.debugger_static_metadata_source_provenance);
    assert!(!parsed.debugger_static_metadata_type_inventory);
    assert!(!parsed.debugger_static_metadata_type_display);
    assert!(!parsed.debugger_static_metadata_symbol_inventory);
    assert!(!parsed.debugger_static_metadata_contract_inventory);
    assert!(!parsed.debugger_static_metadata_contract_display);
    assert!(!parsed.debugger_static_metadata_contract_validation);
    assert!(!parsed.debugger_static_metadata_lowering_summary);
    assert!(!parsed.debugger_static_metadata_symbol_display);
    assert!(!parsed.debugger_static_metadata_symbol_location);
    assert!(!parsed.debugger_static_metadata_safe_point_span);
    assert!(!parsed.debugger_static_metadata_symbol_type);
    assert!(!parsed.debugger_static_metadata_symbol_contract);
    assert!(!parsed.simulate_low_memory);
    assert_eq!(
        parsed.memory_poll_interval,
        memory_pressure::DEFAULT_POLL_INTERVAL
    );
}

#[test]
fn every_flag_is_parsed() {
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--control-socket",
        "/tmp/x-control.sock",
        "--width",
        "100",
        "--height",
        "50",
        "--frame-dir",
        "/tmp/frames",
        "--gatekeeper-socket",
        "/tmp/gatekeeper.sock",
        "--out-of-process-bluejs",
        "--compiler-mcp-socket",
        "/tmp/compiler-mcp.sock",
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-summary",
        "--debugger-static-metadata-source-inventory",
        "--debugger-static-metadata-source-provenance",
        "--debugger-static-metadata-type-inventory",
        "--debugger-static-metadata-type-display",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-contract-inventory",
        "--debugger-static-metadata-contract-display",
        "--debugger-static-metadata-contract-validation",
        "--debugger-static-metadata-lowering-summary",
        "--debugger-static-metadata-symbol-display",
        "--debugger-static-metadata-symbol-location",
        "--debugger-static-metadata-safe-point-span",
        "--debugger-static-metadata-contract-location",
        "--debugger-static-metadata-symbol-type",
        "--debugger-static-metadata-symbol-contract",
        "--simulate-low-memory",
        "--memory-poll-interval-ms",
        "50",
    ])
    .unwrap();
    assert_eq!(
        parsed,
        Args {
            rendezvous_socket: PathBuf::from("/tmp/x.sock"),
            control_socket: PathBuf::from("/tmp/x-control.sock"),
            width: 100.0,
            height: 50.0,
            frame_dir: Some(PathBuf::from("/tmp/frames")),
            gatekeeper_socket: Some(PathBuf::from("/tmp/gatekeeper.sock")),
            out_of_process_bluejs: true,
            compiler_mcp_socket: Some(PathBuf::from("/tmp/compiler-mcp.sock")),
            compiler_catalog_file: None,
            page_http_policy_file: None,
            debugger_socket: Some(PathBuf::from("/tmp/debugger.sock")),
            debugger_bounded_values: false,
            debugger_static_metadata_inventory: true,
            debugger_static_metadata_summary: true,
            debugger_static_metadata_source_inventory: true,
            debugger_static_metadata_source_provenance: true,
            debugger_static_metadata_type_inventory: true,
            debugger_static_metadata_type_display: true,
            debugger_static_metadata_symbol_inventory: true,
            debugger_static_metadata_contract_inventory: true,
            debugger_static_metadata_contract_display: true,
            debugger_static_metadata_contract_validation: true,
            debugger_static_metadata_lowering_summary: true,
            debugger_static_metadata_symbol_display: true,
            debugger_static_metadata_symbol_location: true,
            debugger_static_metadata_safe_point_span: true,
            debugger_static_metadata_source_breakpoint: false,
            debugger_static_metadata_source_span_step: false,
            debugger_static_metadata_contract_location: true,
            debugger_static_metadata_symbol_type: true,
            debugger_static_metadata_symbol_contract: true,
            debugger_static_scope_relation: false,
            simulate_low_memory: true,
            memory_poll_interval: Duration::from_millis(50),
        }
    );
}

#[test]
fn a_control_socket_flag_missing_its_value_is_an_error() {
    assert_eq!(
        args(&["--control-socket"]),
        Err("--control-socket requires a value".to_string())
    );
}

#[test]
fn a_non_numeric_memory_poll_interval_is_an_error() {
    assert_eq!(
        args(&["--memory-poll-interval-ms", "not-a-number"]),
        Err("--memory-poll-interval-ms must be a number".to_string())
    );
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
        args(&["--width", "not-a-number"]),
        Err("--width must be a number".to_string())
    );
}

#[test]
fn a_non_numeric_height_is_an_error() {
    assert_eq!(
        args(&["--height", "not-a-number"]),
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

#[test]
fn static_scope_relation_requires_independent_owner_prerequisites() {
    let flag = "--debugger-static-scope-relation";
    assert_eq!(
        args(&[flag]),
        Err(format!("{flag} requires --debugger-socket"))
    );
    let mut flags = vec!["--debugger-socket", "/tmp/debugger.sock", flag];
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
    assert!(!args(&[]).unwrap().debugger_static_scope_relation);
}

#[test]
fn static_metadata_symbol_location_requires_its_owner_prerequisites() {
    assert_eq!(
        args(&[
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
    let parsed = args(&[
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-source-inventory",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-symbol-location",
    ])
    .unwrap();
    assert!(parsed.debugger_static_metadata_symbol_location);
}

#[test]
fn static_metadata_safe_point_span_requires_explicit_owner_prerequisites() {
    assert_eq!(
        args(&["--debugger-static-metadata-safe-point-span"]),
        Err("--debugger-static-metadata-safe-point-span requires --debugger-socket".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-safe-point-span",
        ]),
        Err("--debugger-static-metadata-safe-point-span requires --debugger-static-metadata-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory", "--debugger-static-metadata-safe-point-span",
        ]),
        Err("--debugger-static-metadata-safe-point-span requires --debugger-static-metadata-source-inventory".to_string())
    );
    let parsed = args(&[
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
        args(&[flag]),
        Err(format!("{flag} requires --debugger-socket"))
    );
    assert_eq!(
        args(&["--debugger-socket", "/tmp/debugger.sock", flag]),
        Err(format!(
            "{flag} requires --debugger-static-metadata-inventory"
        ))
    );
    assert_eq!(
        args(&[
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
        args(&[flag]),
        Err(format!("{flag} requires --debugger-socket"))
    );
    assert_eq!(
        args(&["--debugger-socket", "/tmp/debugger.sock", flag]),
        Err(format!(
            "{flag} requires --debugger-static-metadata-safe-point-span"
        ))
    );
    let parsed = args(&[
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
fn static_metadata_contract_location_requires_its_owner_prerequisites() {
    assert_eq!(
        args(&["--debugger-static-metadata-contract-location"]),
        Err("--debugger-static-metadata-contract-location requires --debugger-socket".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-contract-location",
        ]),
        Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-contract-location",
        ]),
        Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-source-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket", "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-source-inventory",
            "--debugger-static-metadata-contract-location",
        ]),
        Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-contract-inventory".to_string())
    );
    assert!(
        args(&[
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
}

#[test]
fn static_metadata_symbol_type_requires_its_owner_prerequisites() {
    assert_eq!(
        args(&[
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
    let parsed = args(&[
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-type-inventory",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-symbol-type",
    ])
    .unwrap();
    assert!(parsed.debugger_static_metadata_symbol_type);
}

#[test]
fn static_metadata_symbol_contract_requires_its_owner_prerequisites() {
    assert_eq!(
        args(&[
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-symbol-contract",
        ]),
        Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-symbol-contract",
        ]),
        Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-symbol-inventory".to_string())
    );
    assert_eq!(
        args(&[
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-symbol-inventory",
            "--debugger-static-metadata-symbol-contract",
        ]),
        Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-contract-inventory".to_string())
    );
    let parsed = args(&[
        "--debugger-socket",
        "/tmp/debugger.sock",
        "--debugger-static-metadata-inventory",
        "--debugger-static-metadata-symbol-inventory",
        "--debugger-static-metadata-contract-inventory",
        "--debugger-static-metadata-symbol-contract",
    ])
    .unwrap();
    assert!(parsed.debugger_static_metadata_symbol_contract);
}
