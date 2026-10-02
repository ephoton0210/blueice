// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Serves one long-lived BlueJS script connection. Frame parsing lives at the
/// IPC boundary, but every request waits for the owning core session to apply
/// it against its live tab manager. A bad initial handshake gets a structured
/// reply and no DOM request is forwarded.
#[cfg(unix)]
pub(super) fn serve_script_connection(
    mut stream: UnixStream,
    sender: script::ScriptRequestSender,
    expected_token: &str,
) -> io::Result<()> {
    // An unauthenticated peer must not monopolize this serialized listener
    // indefinitely by connecting without sending a complete Hello frame.
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let first = blueice_ipc::script::read_script_request(&mut stream)?;
    if !matches!(
        &first,
        blueice_ipc::script::ScriptRequest::Hello { protocol_version, session_token }
            if *protocol_version == blueice_ipc::script::SCRIPT_PROTOCOL_VERSION
                && script_capability_matches(expected_token, session_token)
    ) {
        blueice_ipc::script::write_script_reply(
            &mut stream,
            &blueice_ipc::script::ScriptReply::Error {
                message: "script handshake denied".to_string(),
            },
        )?;
        return Ok(());
    }
    stream.set_read_timeout(None)?;
    blueice_ipc::script::write_script_reply(
        &mut stream,
        &blueice_ipc::script::ScriptReply::HelloAck {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
        },
    )?;

    let mut next_call_id = 1u64;
    loop {
        let request = match blueice_ipc::script::read_script_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let blueice_ipc::script::ScriptRequest::Call {
            request_id,
            request,
        } = request
        else {
            blueice_ipc::script::write_script_reply(
                &mut stream,
                &blueice_ipc::script::ScriptReply::Error {
                    message: "script DOM calls require a post-handshake envelope".to_string(),
                },
            )?;
            return Ok(());
        };
        let Some(target) = request.document_target() else {
            blueice_ipc::script::write_script_reply(
                &mut stream,
                &blueice_ipc::script::ScriptReply::Error {
                    message: "nested or target-free script call denied".to_string(),
                },
            )?;
            return Ok(());
        };
        if request_id != next_call_id {
            blueice_ipc::script::write_script_reply(
                &mut stream,
                &blueice_ipc::script::ScriptReply::Error {
                    message: "script call ID is not the next connection ID".to_string(),
                },
            )?;
            return Ok(());
        }
        next_call_id = next_call_id.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "script call ID space exhausted")
        })?;
        let reply = sender.request(*request)?;
        blueice_ipc::script::write_script_reply(
            &mut stream,
            &blueice_ipc::script::ScriptReply::CallResult {
                request_id,
                target,
                reply: Box::new(reply),
            },
        )?;
    }
}

/// Avoid prefix matches and data-dependent early exits at the capability
/// boundary. Both strings have the same validated fixed length in production.
#[cfg(unix)]
pub(super) fn script_capability_matches(expected: &str, presented: &str) -> bool {
    if !blueice_ipc::script::valid_script_session_token(presented) {
        return false;
    }
    expected
        .bytes()
        .zip(presented.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

/// Accepts successive script-host connections. A malformed or disconnected
/// host ends only its own connection; it never tears down the core session.
#[cfg(unix)]
pub(super) fn serve_script_listener(
    listener: UnixListener,
    sender: script::ScriptRequestSender,
    expected_token: String,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_script_connection(stream, sender.clone(), &expected_token);
    }
}

/// Serves one native debugger discovery connection. The first-message
/// negotiation belongs at this transport boundary; every later target lookup
/// is forwarded to the core session thread, which owns live tab state.
#[cfg(unix)]
pub(super) fn serve_debugger_connection(
    mut stream: UnixStream,
    sender: blueice_engine::debugger::DebuggerRequestSender,
    allowed_metadata_capabilities: &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest,
    allow_bounded_values: bool,
) -> io::Result<()> {
    let first = blueice_ipc::debugger::read_debugger_request(&mut stream)?;
    let reply = blueice_ipc::debugger::negotiate_with_values(
        &first,
        allowed_metadata_capabilities,
        allow_bounded_values,
    );
    blueice_ipc::debugger::write_debugger_reply(&mut stream, &reply)?;
    let Some(metadata_session) =
        blueice_ipc::debugger::metadata_session_authorization(&first, &reply)
    else {
        return Ok(());
    };

    loop {
        let request = match blueice_ipc::debugger::read_debugger_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = sender
            .request_with_metadata_session_authorization(request, metadata_session.clone())?;
        blueice_ipc::debugger::write_debugger_reply(&mut stream, &reply)?;
    }
}

/// Accepts successive debugger peers. A malformed/disconnected peer ends only
/// its connection and never interrupts the owning frontend session.
#[cfg(unix)]
pub(super) fn serve_debugger_listener(
    listener: UnixListener,
    sender: blueice_engine::debugger::DebuggerRequestSender,
    allowed_metadata_capabilities: blueice_ipc::debugger::DebuggerMetadataCapabilityManifest,
    allow_bounded_values: bool,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_debugger_connection(
            stream,
            sender.clone(),
            &allowed_metadata_capabilities,
            allow_bounded_values,
        );
    }
}

/// Serves one query-only registered-project compiler peer. Its `Hello`
/// negotiation is intentionally completed on the listener side, while every
/// later decoded request is synchronously handed to the sealed core catalog on
/// the session thread under its core-minted stream attestation. Abandoned
/// pagination cursors are revoked when this stream closes. The worker owns no
/// source, project registration, or incremental compiler cache.
#[cfg(unix)]
pub(super) fn serve_compiler_connection(
    mut stream: UnixStream,
    sender: CompilerServiceIpcRequestSender,
) -> io::Result<()> {
    let first = blueice_ipc::compiler::read_compiler_request(&mut stream)?;
    let accepted = matches!(
        first,
        blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
        }
    );
    let session_evidence = accepted
        .then(mint_compiler_session_hello_evidence)
        .transpose()?;
    let session_sender = session_evidence
        .as_ref()
        .map(|evidence| sender.bind_session(evidence.session_attestation.clone()))
        .transpose()?;
    let reply = blueice_ipc::compiler::negotiate(&first, session_evidence);
    blueice_ipc::compiler::write_compiler_reply(&mut stream, &reply)?;
    if !accepted {
        return Ok(());
    }
    let session_sender = session_sender.expect("an accepted Hello mints a bound compiler stream");

    loop {
        let request = match blueice_ipc::compiler::read_compiler_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = session_sender.request(request)?;
        blueice_ipc::compiler::write_compiler_reply(&mut stream, &reply)?;
    }
}

/// Mints all handshake evidence for one accepted compiler stream. The core
/// creates it only after the exact v6 `Hello`: an opaque per-stream
/// attestation and the canonical fixed query-only manifest. Neither is tied
/// to a project, source graph, catalog, path, or any extra authority.
#[cfg(unix)]
pub(super) fn mint_compiler_session_hello_evidence(
) -> io::Result<blueice_ipc::compiler::CompilerSessionHelloEvidence> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut id = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    let session_attestation = blueice_ipc::compiler::CompilerSessionAttestation { id };
    debug_assert!(session_attestation.is_well_formed());
    let capability_manifest =
        blueice_ipc::compiler::CompilerSessionCapabilityManifest::fixed_query_only();
    debug_assert!(capability_manifest.is_well_formed());
    Ok(blueice_ipc::compiler::CompilerSessionHelloEvidence {
        session_attestation,
        capability_manifest,
    })
}

/// Accepts successive compiler query peers. Bad handshakes and disconnected
/// peers affect only their own stream; they cannot tear down the frontend or
/// alter the catalog registered by core startup.
#[cfg(unix)]
pub(super) fn serve_compiler_listener(
    listener: UnixListener,
    sender: CompilerServiceIpcRequestSender,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_compiler_connection(stream, sender.clone());
    }
}

/// Binds either private capability-bearing listener with an explicit
/// owner-only filesystem mode. A bearer token or opaque project ID is not a
/// reason to rely on a permissive ambient umask. Existing live or non-socket
/// paths are preserved; only an abandoned socket inode can be reclaimed.
#[cfg(unix)]
pub(super) fn bind_owner_only_listener(
    path: &std::path::Path,
    label: &str,
) -> io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => match UnixStream::connect(path) {
            // Never unlink a working peer merely because a second core was
            // pointed at its endpoint.  The launcher preflights this too, but
            // direct core invocation must preserve the same boundary.
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{label} socket is already active: {}", path.display()),
                ));
            }
            // This is the one recoverable startup residue: a dead core can
            // leave its socket inode behind after a forceful stop.
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                remove_owned_socket_if_owned(path);
            }
            Err(error) => return Err(error),
        },
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{label} socket is occupied by a non-socket path: {}",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        remove_owned_socket_if_owned(path);
        return Err(error);
    }
    Ok(listener)
}

#[cfg(unix)]
pub(super) fn bind_compiler_listener(path: &std::path::Path) -> io::Result<UnixListener> {
    bind_owner_only_listener(path, "compiler")
}

#[cfg(unix)]
pub(super) fn bind_script_listener(path: &std::path::Path) -> io::Result<UnixListener> {
    bind_owner_only_listener(path, "script")
}

/// Removes a private listener endpoint only if it is still a Unix socket.
/// The core may be force-killed by its supervisor, but lifecycle cleanup must
/// never unlink a regular file, directory, or symlink that has appeared at a
/// caller-selected path since the listener was created.
#[cfg(unix)]
pub(super) fn remove_owned_socket_if_owned(path: &std::path::Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_socket() {
        let _ = std::fs::remove_file(path);
    }
}
