// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Takes an injectable argument iterator (rather than reading
/// `std::env::args()` directly) so every flag-parsing branch is a
/// plain unit test, not something only exercisable by actually
/// spawning the binary -- the subprocess-level integration test in
/// `tests/core_binary.rs` covers `main`'s own process wiring (bind,
/// accept, cleanup) instead, which this function deliberately knows
/// nothing about.
#[cfg(unix)]
pub(super) fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut width = 800.0;
    let mut height = 600.0;
    let mut frame_dir = None;
    let mut gatekeeper_socket = None;
    let mut script_socket = None;
    let mut script_session_token = None;
    let mut debugger_socket = None;
    let mut debugger_bounded_values = false;
    let mut debugger_static_metadata_inventory = false;
    let mut debugger_static_metadata_summary = false;
    let mut debugger_static_metadata_source_inventory = false;
    let mut debugger_static_metadata_source_provenance = false;
    let mut debugger_static_metadata_type_inventory = false;
    let mut debugger_static_metadata_type_display = false;
    let mut debugger_static_metadata_symbol_inventory = false;
    let mut debugger_static_metadata_contract_inventory = false;
    let mut debugger_static_metadata_contract_display = false;
    let mut debugger_static_metadata_contract_validation = false;
    let mut debugger_static_metadata_lowering_summary = false;
    let mut debugger_static_metadata_symbol_display = false;
    let mut debugger_static_metadata_symbol_location = false;
    let mut debugger_static_metadata_safe_point_span = false;
    let mut debugger_static_metadata_source_breakpoint = false;
    let mut debugger_static_metadata_source_span_step = false;
    let mut debugger_static_metadata_contract_location = false;
    let mut debugger_static_metadata_symbol_type = false;
    let mut debugger_static_metadata_symbol_contract = false;
    let mut debugger_static_scope_relation = false;
    let mut compiler_socket = None;
    let mut compiler_project_profile = None;
    let mut compiler_catalog_stdin = false;
    let mut owner_bootstrap_stdin = false;
    let mut inline_bluets_profile = None;
    let mut inline_bluejs = false;
    let mut out_of_process_bluejs_socket = None;
    let mut out_of_process_bluejs_token = None;
    let mut out_of_process_bluejs_page_script_profile = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--width" => {
                width = value()?
                    .parse()
                    .map_err(|_| "--width must be a number".to_string())?
            }
            "--height" => {
                height = value()?
                    .parse()
                    .map_err(|_| "--height must be a number".to_string())?
            }
            "--frame-dir" => frame_dir = Some(PathBuf::from(value()?)),
            "--gatekeeper-socket" => gatekeeper_socket = Some(PathBuf::from(value()?)),
            "--script-socket" => script_socket = Some(PathBuf::from(value()?)),
            "--script-session-token" => script_session_token = Some(value()?),
            "--debugger-socket" => debugger_socket = Some(PathBuf::from(value()?)),
            "--debugger-bounded-values" => debugger_bounded_values = true,
            "--debugger-static-metadata-inventory" => debugger_static_metadata_inventory = true,
            "--debugger-static-metadata-summary" => debugger_static_metadata_summary = true,
            "--debugger-static-metadata-source-inventory" => {
                debugger_static_metadata_source_inventory = true
            }
            "--debugger-static-metadata-source-provenance" => {
                debugger_static_metadata_source_provenance = true
            }
            "--debugger-static-metadata-type-inventory" => {
                debugger_static_metadata_type_inventory = true
            }
            "--debugger-static-metadata-type-display" => {
                debugger_static_metadata_type_display = true
            }
            "--debugger-static-metadata-symbol-inventory" => {
                debugger_static_metadata_symbol_inventory = true
            }
            "--debugger-static-metadata-contract-inventory" => {
                debugger_static_metadata_contract_inventory = true
            }
            "--debugger-static-metadata-contract-display" => {
                debugger_static_metadata_contract_display = true
            }
            "--debugger-static-metadata-contract-validation" => {
                debugger_static_metadata_contract_validation = true
            }
            "--debugger-static-metadata-lowering-summary" => {
                debugger_static_metadata_lowering_summary = true
            }
            "--debugger-static-metadata-symbol-display" => {
                debugger_static_metadata_symbol_display = true
            }
            "--debugger-static-metadata-symbol-location" => {
                debugger_static_metadata_symbol_location = true
            }
            "--debugger-static-metadata-safe-point-span" => {
                debugger_static_metadata_safe_point_span = true
            }
            "--debugger-static-metadata-source-breakpoint" => {
                debugger_static_metadata_source_breakpoint = true
            }
            "--debugger-static-metadata-source-span-step" => {
                debugger_static_metadata_source_span_step = true
            }
            "--debugger-static-metadata-contract-location" => {
                debugger_static_metadata_contract_location = true
            }
            "--debugger-static-metadata-symbol-type" => debugger_static_metadata_symbol_type = true,
            "--debugger-static-metadata-symbol-contract" => {
                debugger_static_metadata_symbol_contract = true
            }
            "--debugger-static-scope-relation" => debugger_static_scope_relation = true,
            "--compiler-socket" => compiler_socket = Some(PathBuf::from(value()?)),
            "--compiler-project-profile" => compiler_project_profile = Some(value()?),
            "--compiler-catalog-stdin" => compiler_catalog_stdin = true,
            "--owner-bootstrap-stdin" => owner_bootstrap_stdin = true,
            "--inline-bluets-profile" => inline_bluets_profile = Some(value()?),
            "--inline-bluejs" => inline_bluejs = true,
            "--out-of-process-bluejs-socket" => {
                out_of_process_bluejs_socket = Some(PathBuf::from(value()?))
            }
            "--out-of-process-bluejs-token" => out_of_process_bluejs_token = Some(value()?),
            "--out-of-process-bluejs-page-script-profile" => {
                out_of_process_bluejs_page_script_profile = Some(value()?)
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    if script_socket.is_some() != script_session_token.is_some() {
        return Err(
            "--script-socket and --script-session-token must be provided together".to_string(),
        );
    }
    if script_session_token
        .as_deref()
        .is_some_and(|token| !blueice_ipc::script::valid_script_session_token(token))
    {
        return Err("--script-session-token must be 64 lowercase hexadecimal bytes".to_string());
    }
    if inline_bluets_profile.is_some() && inline_bluejs {
        return Err("--inline-bluejs cannot be combined with --inline-bluets-profile".to_string());
    }
    if out_of_process_bluejs_socket.is_some() != out_of_process_bluejs_token.is_some() {
        return Err(
            "--out-of-process-bluejs-socket and --out-of-process-bluejs-token must be provided together"
                .to_string(),
        );
    }
    if out_of_process_bluejs_token
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err("--out-of-process-bluejs-token must not be empty".to_string());
    }
    if let (Some(script_token), Some(page_host_token)) = (
        script_session_token.as_deref(),
        out_of_process_bluejs_token.as_deref(),
    ) {
        if script_token != page_host_token {
            return Err(
                "--script-session-token must match the supervised BlueJS child capability"
                    .to_string(),
            );
        }
    }
    if out_of_process_bluejs_page_script_profile.is_some() && out_of_process_bluejs_socket.is_none()
    {
        return Err(
            "--out-of-process-bluejs-page-script-profile requires the private out-of-process BlueJS host"
                .to_string(),
        );
    }
    if let Some(profile) = out_of_process_bluejs_page_script_profile.as_deref() {
        if profile != script::http_resource_authorizer::CORE_HTTP_PAGE_SCRIPT_FIXTURE_PROFILE {
            return Err(
                "--out-of-process-bluejs-page-script-profile must name the fixed core HTTP page-script profile"
                    .to_string(),
            );
        }
    }
    if out_of_process_bluejs_socket.is_some() && (inline_bluets_profile.is_some() || inline_bluejs)
    {
        return Err(
            "--out-of-process-bluejs-socket cannot be combined with --inline-bluejs or --inline-bluets-profile"
                .to_string(),
        );
    }
    if debugger_bounded_values && debugger_socket.is_none() {
        return Err("--debugger-bounded-values requires --debugger-socket".to_string());
    }
    if debugger_static_metadata_inventory && debugger_socket.is_none() {
        return Err("--debugger-static-metadata-inventory requires --debugger-socket".to_string());
    }
    if debugger_static_metadata_summary && debugger_socket.is_none() {
        return Err("--debugger-static-metadata-summary requires --debugger-socket".to_string());
    }
    if debugger_static_metadata_summary && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-summary requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_source_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-source-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_source_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-source-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_source_provenance && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-source-provenance requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_source_provenance && !debugger_static_metadata_source_inventory {
        return Err(
            "--debugger-static-metadata-source-provenance requires --debugger-static-metadata-source-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_type_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-type-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_type_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-type-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_type_display && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-type-display requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_type_display && !debugger_static_metadata_type_inventory {
        return Err(
            "--debugger-static-metadata-type-display requires --debugger-static-metadata-type-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-symbol-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_contract_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-contract-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_contract_display && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-display requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_display && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-contract-display requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_contract_validation && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-validation requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_validation && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-contract-validation requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_lowering_summary && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-lowering-summary requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_lowering_summary && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-lowering-summary requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_display && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-display requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_display && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-symbol-display requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_location && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-location requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_location && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-symbol-location requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_location && !debugger_static_metadata_source_inventory {
        return Err(
            "--debugger-static-metadata-symbol-location requires --debugger-static-metadata-source-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_location && !debugger_static_metadata_symbol_inventory {
        return Err(
            "--debugger-static-metadata-symbol-location requires --debugger-static-metadata-symbol-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_safe_point_span && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-safe-point-span requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_safe_point_span && !debugger_static_metadata_inventory {
        return Err("--debugger-static-metadata-safe-point-span requires --debugger-static-metadata-inventory".to_string());
    }
    if debugger_static_metadata_safe_point_span && !debugger_static_metadata_source_inventory {
        return Err("--debugger-static-metadata-safe-point-span requires --debugger-static-metadata-source-inventory".to_string());
    }
    if debugger_static_metadata_source_breakpoint && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-source-breakpoint requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_source_breakpoint && !debugger_static_metadata_inventory {
        return Err("--debugger-static-metadata-source-breakpoint requires --debugger-static-metadata-inventory".to_string());
    }
    if debugger_static_metadata_source_breakpoint && !debugger_static_metadata_source_inventory {
        return Err("--debugger-static-metadata-source-breakpoint requires --debugger-static-metadata-source-inventory".to_string());
    }
    if debugger_static_metadata_source_span_step && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-source-span-step requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_source_span_step && !debugger_static_metadata_safe_point_span {
        return Err("--debugger-static-metadata-source-span-step requires --debugger-static-metadata-safe-point-span".to_string());
    }
    if debugger_static_metadata_contract_location && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-location requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_location && !debugger_static_metadata_inventory {
        return Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-inventory".to_string());
    }
    if debugger_static_metadata_contract_location && !debugger_static_metadata_source_inventory {
        return Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-source-inventory".to_string());
    }
    if debugger_static_metadata_contract_location && !debugger_static_metadata_contract_inventory {
        return Err("--debugger-static-metadata-contract-location requires --debugger-static-metadata-contract-inventory".to_string());
    }
    if debugger_static_metadata_symbol_type && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-type requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_type && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-symbol-type requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_type && !debugger_static_metadata_type_inventory {
        return Err("--debugger-static-metadata-symbol-type requires --debugger-static-metadata-type-inventory".to_string());
    }
    if debugger_static_metadata_symbol_type && !debugger_static_metadata_symbol_inventory {
        return Err("--debugger-static-metadata-symbol-type requires --debugger-static-metadata-symbol-inventory".to_string());
    }
    if debugger_static_metadata_symbol_contract && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-contract requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_contract && !debugger_static_metadata_inventory {
        return Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-inventory".to_string());
    }
    if debugger_static_metadata_symbol_contract && !debugger_static_metadata_symbol_inventory {
        return Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-symbol-inventory".to_string());
    }
    if debugger_static_metadata_symbol_contract && !debugger_static_metadata_contract_inventory {
        return Err("--debugger-static-metadata-symbol-contract requires --debugger-static-metadata-contract-inventory".to_string());
    }
    if debugger_static_scope_relation && debugger_socket.is_none() {
        return Err("--debugger-static-scope-relation requires --debugger-socket".to_string());
    }
    if debugger_static_scope_relation && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-scope-relation requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_scope_relation && !debugger_static_metadata_type_inventory {
        return Err(
            "--debugger-static-scope-relation requires --debugger-static-metadata-type-inventory"
                .to_string(),
        );
    }
    if debugger_static_scope_relation && !debugger_static_metadata_symbol_inventory {
        return Err(
            "--debugger-static-scope-relation requires --debugger-static-metadata-symbol-inventory"
                .to_string(),
        );
    }
    if owner_bootstrap_stdin
        && (compiler_catalog_stdin || out_of_process_bluejs_page_script_profile.is_some())
    {
        return Err(
            "--owner-bootstrap-stdin cannot be combined with other compiler/page startup selectors"
                .to_string(),
        );
    }
    if (compiler_project_profile.is_some() || compiler_catalog_stdin) && compiler_socket.is_none()
        || (compiler_project_profile.is_some() && compiler_catalog_stdin)
        || (compiler_socket.is_some()
            && !(compiler_project_profile.is_some()
                || compiler_catalog_stdin
                || owner_bootstrap_stdin))
    {
        return Err("--compiler-socket requires exactly one compiler startup selector".to_string());
    }
    Ok(Args {
        socket,
        width,
        height,
        frame_dir,
        gatekeeper_socket,
        script_socket,
        script_session_token,
        debugger_socket,
        debugger_bounded_values,
        debugger_static_metadata_inventory,
        debugger_static_metadata_summary,
        debugger_static_metadata_source_inventory,
        debugger_static_metadata_source_provenance,
        debugger_static_metadata_type_inventory,
        debugger_static_metadata_type_display,
        debugger_static_metadata_symbol_inventory,
        debugger_static_metadata_contract_inventory,
        debugger_static_metadata_contract_display,
        debugger_static_metadata_contract_validation,
        debugger_static_metadata_lowering_summary,
        debugger_static_metadata_symbol_display,
        debugger_static_metadata_symbol_location,
        debugger_static_metadata_safe_point_span,
        debugger_static_metadata_source_breakpoint,
        debugger_static_metadata_source_span_step,
        debugger_static_metadata_contract_location,
        debugger_static_metadata_symbol_type,
        debugger_static_metadata_symbol_contract,
        debugger_static_scope_relation,
        compiler_socket,
        compiler_project_profile,
        compiler_catalog_stdin,
        owner_bootstrap_stdin,
        inline_bluets_profile,
        inline_bluejs,
        out_of_process_bluejs_socket,
        out_of_process_bluejs_token,
        out_of_process_bluejs_page_script_profile,
    })
}
