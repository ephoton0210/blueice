// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional lifetime pipe for a native frontend that owns the entire stack.
//! Browser/MCP disconnection alone still has the existing shared-broker meaning.

use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) fn stop_on_stdin_eof(socket: PathBuf) {
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        let mut buffer = [0_u8; 256];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        drop(input);
        // This listener is already bound. Once the broker starts, request its
        // existing normal shutdown so all supervised services are reaped by
        // their owners. No signals affect the invoking shell/process group.
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let deadline = Instant::now() + Duration::from_secs(5);
            let hello_id = u64::MAX;
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            if blueice_ipc::write_client_message_with_id(
                &mut stream,
                Some(hello_id),
                &blueice_ipc::ClientMessage::Hello {
                    protocol_version: blueice_ipc::PROTOCOL_VERSION,
                },
            )
            .is_err()
            {
                return;
            }
            // The broker can broadcast pending page/frame replies before our
            // own Hello. Match its request ID instead of assuming the first
            // frame on a shared connection is the handshake response.
            while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
                    break;
                }
                match blueice_ipc::read_server_message_with_id(&mut stream) {
                    Ok((Some(id), blueice_ipc::ServerMessage::Hello { protocol_version }))
                        if id == hello_id && protocol_version == blueice_ipc::PROTOCOL_VERSION =>
                    {
                        let _ = blueice_ipc::write_client_message(
                            &mut stream,
                            &blueice_ipc::ClientMessage::Shutdown,
                        );
                        break;
                    }
                    Ok((Some(id), _)) if id == hello_id => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }
    });
}
