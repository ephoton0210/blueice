// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private isolated BlueJS page-host child launched only by
//! `blueice-launcher`. It owns no page loader, core DOM, network, filesystem,
//! or frontend socket. Owner-selected proof profiles can use the launcher's
//! generation-private script socket for bounded live DOM operations;
//! ordinary realms execute only complete authorized source graphs.

#[cfg(unix)]
use blueice_launcher::bluejs_host::{
    bind_bluejs_host_socket, serve_bluejs_host_listener, BlueJsChildHost, BlueJsHostRuntimeLimits,
};
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::ExitCode;

#[cfg(unix)]
#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    session_token: String,
    script_socket: Option<PathBuf>,
    enable_dom_lookup_probe: bool,
    enable_dom_text_profile: bool,
    enable_dom_mutation_profile: bool,
    enable_dom_event_profile: bool,
    runtime_limits: BlueJsHostRuntimeLimits,
}

#[cfg(unix)]
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut session_token = None;
    let mut script_socket = None;
    let mut enable_dom_lookup_probe = false;
    let mut enable_dom_text_profile = false;
    let mut enable_dom_mutation_profile = false;
    let mut enable_dom_event_profile = false;
    let mut max_realms = None;
    let mut max_programs_per_realm = None;
    let mut max_bytecode_bytes_per_realm = None;
    let mut max_heap_bytes_per_realm = None;
    let mut max_reserved_programs = None;
    let mut max_reserved_bytecode_bytes = None;
    let mut max_reserved_heap_bytes = None;
    let mut args = args;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--session-token" => session_token = Some(value()?),
            "--script-socket" => {
                if script_socket.is_some() {
                    return Err("--script-socket may be supplied only once".to_string());
                }
                script_socket = Some(PathBuf::from(value()?));
            }
            "--enable-dom-lookup-probe" => {
                if enable_dom_lookup_probe {
                    return Err("--enable-dom-lookup-probe may be supplied only once".to_string());
                }
                enable_dom_lookup_probe = true;
            }
            "--enable-dom-text-profile" => {
                if enable_dom_text_profile {
                    return Err("--enable-dom-text-profile may be supplied only once".to_string());
                }
                enable_dom_text_profile = true;
            }
            "--enable-dom-mutation-profile" => {
                if enable_dom_mutation_profile {
                    return Err(
                        "--enable-dom-mutation-profile may be supplied only once".to_string()
                    );
                }
                enable_dom_mutation_profile = true;
            }
            "--enable-dom-event-profile" => {
                if enable_dom_event_profile {
                    return Err("--enable-dom-event-profile may be supplied only once".to_string());
                }
                enable_dom_event_profile = true;
            }
            "--max-realms" => {
                if max_realms.is_some() {
                    return Err("--max-realms may be supplied only once".to_string());
                }
                max_realms = Some(parse_limit("--max-realms", value()?)?);
            }
            "--max-programs-per-realm" => {
                if max_programs_per_realm.is_some() {
                    return Err("--max-programs-per-realm may be supplied only once".to_string());
                }
                max_programs_per_realm = Some(parse_limit("--max-programs-per-realm", value()?)?)
            }
            "--max-bytecode-bytes-per-realm" => {
                if max_bytecode_bytes_per_realm.is_some() {
                    return Err(
                        "--max-bytecode-bytes-per-realm may be supplied only once".to_string()
                    );
                }
                max_bytecode_bytes_per_realm =
                    Some(parse_limit("--max-bytecode-bytes-per-realm", value()?)?)
            }
            "--max-heap-bytes-per-realm" => {
                if max_heap_bytes_per_realm.is_some() {
                    return Err("--max-heap-bytes-per-realm may be supplied only once".to_string());
                }
                max_heap_bytes_per_realm =
                    Some(parse_limit("--max-heap-bytes-per-realm", value()?)?)
            }
            "--max-reserved-programs" => {
                if max_reserved_programs.is_some() {
                    return Err("--max-reserved-programs may be supplied only once".to_string());
                }
                max_reserved_programs = Some(parse_limit("--max-reserved-programs", value()?)?)
            }
            "--max-reserved-bytecode-bytes" => {
                if max_reserved_bytecode_bytes.is_some() {
                    return Err(
                        "--max-reserved-bytecode-bytes may be supplied only once".to_string()
                    );
                }
                max_reserved_bytecode_bytes =
                    Some(parse_limit("--max-reserved-bytecode-bytes", value()?)?)
            }
            "--max-reserved-heap-bytes" => {
                if max_reserved_heap_bytes.is_some() {
                    return Err("--max-reserved-heap-bytes may be supplied only once".to_string());
                }
                max_reserved_heap_bytes = Some(parse_limit("--max-reserved-heap-bytes", value()?)?)
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    let session_token =
        session_token.ok_or_else(|| "--session-token <token> is required".to_string())?;
    if session_token.len() < 32 || !session_token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--session-token must be a non-short hexadecimal capability".to_string());
    }
    if enable_dom_lookup_probe && script_socket.is_none() {
        return Err("--enable-dom-lookup-probe requires --script-socket".to_string());
    }
    if enable_dom_text_profile && script_socket.is_none() {
        return Err("--enable-dom-text-profile requires --script-socket".to_string());
    }
    if enable_dom_mutation_profile && script_socket.is_none() {
        return Err("--enable-dom-mutation-profile requires --script-socket".to_string());
    }
    if enable_dom_event_profile && script_socket.is_none() {
        return Err("--enable-dom-event-profile requires --script-socket".to_string());
    }
    if [
        enable_dom_lookup_probe,
        enable_dom_text_profile,
        enable_dom_mutation_profile,
        enable_dom_event_profile,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count()
        > 1
    {
        return Err("DOM proof profiles are mutually exclusive".to_string());
    }
    if let Some(script_socket) = script_socket.as_ref() {
        if !script_socket.is_absolute() {
            return Err("--script-socket must be absolute".to_string());
        }
        if !blueice_ipc::script::valid_script_session_token(&session_token) {
            return Err("--script-socket requires a fixed-shape lowercase capability".to_string());
        }
    }
    let runtime_limits = match (
        max_realms,
        max_programs_per_realm,
        max_bytecode_bytes_per_realm,
        max_heap_bytes_per_realm,
        max_reserved_programs,
        max_reserved_bytecode_bytes,
        max_reserved_heap_bytes,
    ) {
        (None, None, None, None, None, None, None) => BlueJsHostRuntimeLimits::default(),
        (
            Some(max_realms),
            Some(max_programs_per_realm),
            Some(max_bytecode_bytes_per_realm),
            Some(max_heap_bytes_per_realm),
            Some(max_reserved_programs),
            Some(max_reserved_bytecode_bytes),
            Some(max_reserved_heap_bytes),
        ) => BlueJsHostRuntimeLimits {
            max_realms,
            max_programs_per_realm,
            max_bytecode_bytes_per_realm,
            max_heap_bytes_per_realm,
            max_reserved_programs,
            max_reserved_bytecode_bytes,
            max_reserved_heap_bytes,
        },
        _ => {
            return Err("all BlueJS page-host runtime limits must be supplied together".to_string())
        }
    };
    runtime_limits.runtime_config().map_err(str::to_string)?;
    Ok(Args {
        socket,
        session_token,
        script_socket,
        enable_dom_lookup_probe,
        enable_dom_text_profile,
        enable_dom_mutation_profile,
        enable_dom_event_profile,
        runtime_limits,
    })
}

#[cfg(unix)]
fn parse_limit(flag: &str, value: String) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| format!("{flag} must be a non-zero unsigned integer"))?;
    if limit == 0 {
        return Err(format!("{flag} must be a non-zero unsigned integer"));
    }
    Ok(limit)
}

#[cfg(unix)]
fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-bluejs-host: {message}");
            return ExitCode::FAILURE;
        }
    };
    let mut host = match BlueJsChildHost::with_runtime_limits(args.runtime_limits) {
        Ok(host) => host,
        Err(error) => {
            eprintln!("blueice-bluejs-host: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(script_socket) = args.script_socket {
        if let Err(error) = host.configure_script_dom_capability(
            script_socket,
            args.session_token.clone(),
            args.enable_dom_lookup_probe,
            args.enable_dom_text_profile,
            args.enable_dom_mutation_profile,
            args.enable_dom_event_profile,
        ) {
            eprintln!("blueice-bluejs-host: invalid private script capability: {error}");
            return ExitCode::FAILURE;
        }
    }
    let listener = match bind_bluejs_host_socket(&args.socket) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "blueice-bluejs-host: failed to bind {}: {error}",
                args.socket.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let result = serve_bluejs_host_listener(listener, args.session_token, &mut host);
    let _ = std::fs::remove_file(&args.socket);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("blueice-bluejs-host: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("blueice-bluejs-host is currently supported only on Unix platforms");
    std::process::exit(1);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|flag| flag.to_string()))
    }

    #[test]
    fn child_requires_both_private_startup_arguments() {
        assert_eq!(args(&[]), Err("--socket <path> is required".to_string()));
        assert_eq!(
            args(&["--socket", "/tmp/host.sock"]),
            Err("--session-token <token> is required".to_string())
        );
    }

    #[test]
    fn child_rejects_a_short_or_non_hex_capability() {
        assert_eq!(
            args(&["--socket", "/tmp/host.sock", "--session-token", "short"]),
            Err("--session-token must be a non-short hexadecimal capability".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            ]),
            Err("--session-token must be a non-short hexadecimal capability".to_string())
        );
    }

    #[test]
    fn dom_lookup_probe_requires_an_absolute_private_script_socket_and_exact_capability() {
        let token = "0123456789abcdef".repeat(4);
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--enable-dom-lookup-probe",
            ]),
            Err("--enable-dom-lookup-probe requires --script-socket".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--script-socket",
                "relative.sock",
            ]),
            Err("--script-socket must be absolute".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                "0123456789abcdef0123456789abcdef",
                "--script-socket",
                "/tmp/script.sock",
            ]),
            Err("--script-socket requires a fixed-shape lowercase capability".to_string())
        );
        let parsed = args(&[
            "--socket",
            "/tmp/host.sock",
            "--session-token",
            &token,
            "--script-socket",
            "/tmp/script.sock",
            "--enable-dom-lookup-probe",
        ])
        .unwrap();
        assert_eq!(
            parsed.script_socket,
            Some(PathBuf::from("/tmp/script.sock"))
        );
        assert!(parsed.enable_dom_lookup_probe);
        assert!(!parsed.enable_dom_text_profile);
    }

    #[test]
    fn live_dom_text_profile_requires_a_socket_and_excludes_the_lookup_probe() {
        let token = "0123456789abcdef".repeat(4);
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--enable-dom-text-profile",
            ]),
            Err("--enable-dom-text-profile requires --script-socket".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--script-socket",
                "/tmp/script.sock",
                "--enable-dom-text-profile",
                "--enable-dom-lookup-probe",
            ]),
            Err("DOM proof profiles are mutually exclusive".to_string())
        );
        let parsed = args(&[
            "--socket",
            "/tmp/host.sock",
            "--session-token",
            &token,
            "--script-socket",
            "/tmp/script.sock",
            "--enable-dom-text-profile",
        ])
        .unwrap();
        assert!(parsed.enable_dom_text_profile);
        assert!(!parsed.enable_dom_lookup_probe);
        assert!(!parsed.enable_dom_mutation_profile);
    }

    #[test]
    fn live_dom_mutation_profile_requires_a_socket_and_excludes_other_profiles() {
        let token = "0123456789abcdef".repeat(4);
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--enable-dom-mutation-profile",
            ]),
            Err("--enable-dom-mutation-profile requires --script-socket".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--script-socket",
                "/tmp/script.sock",
                "--enable-dom-mutation-profile",
                "--enable-dom-text-profile",
            ]),
            Err("DOM proof profiles are mutually exclusive".to_string())
        );
        let parsed = args(&[
            "--socket",
            "/tmp/host.sock",
            "--session-token",
            &token,
            "--script-socket",
            "/tmp/script.sock",
            "--enable-dom-mutation-profile",
        ])
        .unwrap();
        assert!(parsed.enable_dom_mutation_profile);
        assert!(!parsed.enable_dom_text_profile);
        assert!(!parsed.enable_dom_lookup_probe);
    }

    #[test]
    fn live_dom_event_profile_requires_socket_and_is_exclusive() {
        let token = "0123456789abcdef".repeat(4);
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--enable-dom-event-profile",
            ]),
            Err("--enable-dom-event-profile requires --script-socket".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                &token,
                "--script-socket",
                "/tmp/script.sock",
                "--enable-dom-mutation-profile",
                "--enable-dom-event-profile",
            ]),
            Err("DOM proof profiles are mutually exclusive".to_string())
        );
        let parsed = args(&[
            "--socket",
            "/tmp/host.sock",
            "--session-token",
            &token,
            "--script-socket",
            "/tmp/script.sock",
            "--enable-dom-event-profile",
        ])
        .unwrap();
        assert!(parsed.enable_dom_event_profile);
    }

    #[test]
    fn child_accepts_only_one_complete_runtime_limit_envelope() {
        let token = "0123456789abcdef0123456789abcdef";
        let parsed = args(&[
            "--socket",
            "/tmp/host.sock",
            "--session-token",
            token,
            "--max-realms",
            "2",
            "--max-programs-per-realm",
            "3",
            "--max-bytecode-bytes-per-realm",
            "4096",
            "--max-heap-bytes-per-realm",
            "8192",
            "--max-reserved-programs",
            "3",
            "--max-reserved-bytecode-bytes",
            "4096",
            "--max-reserved-heap-bytes",
            "8192",
        ])
        .expect("a complete valid runtime envelope must parse");
        assert_eq!(
            parsed.runtime_limits,
            BlueJsHostRuntimeLimits {
                max_realms: 2,
                max_programs_per_realm: 3,
                max_bytecode_bytes_per_realm: 4096,
                max_heap_bytes_per_realm: 8192,
                max_reserved_programs: 3,
                max_reserved_bytecode_bytes: 4096,
                max_reserved_heap_bytes: 8192,
            }
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                token,
                "--max-realms",
                "2",
            ]),
            Err("all BlueJS page-host runtime limits must be supplied together".to_string())
        );
        let missing_reservation = args(&[
            "--socket",
            "/tmp/host.sock",
            "--session-token",
            token,
            "--max-realms",
            "2",
            "--max-programs-per-realm",
            "3",
            "--max-bytecode-bytes-per-realm",
            "4096",
            "--max-heap-bytes-per-realm",
            "8192",
        ]);
        assert_eq!(
            missing_reservation,
            Err("all BlueJS page-host runtime limits must be supplied together".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                token,
                "--max-realms",
                "2",
                "--max-programs-per-realm",
                "3",
                "--max-bytecode-bytes-per-realm",
                "4096",
                "--max-heap-bytes-per-realm",
                "8192",
                "--max-reserved-programs",
                "2",
                "--max-reserved-bytecode-bytes",
                "4096",
                "--max-reserved-heap-bytes",
                "8192",
            ]),
            Err("BlueJS child-wide reservations must cover one full realm".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                token,
                "--max-realms",
                "0",
            ]),
            Err("--max-realms must be a non-zero unsigned integer".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                token,
                "--max-realms",
                "2",
                "--max-realms",
                "3",
            ]),
            Err("--max-realms may be supplied only once".to_string())
        );
    }
}
