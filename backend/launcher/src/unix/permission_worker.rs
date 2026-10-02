// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) type PermissionExchange = (
    PermissionControlRequest,
    Sender<io::Result<PermissionControlReply>>,
);

/// Owns the only parent-side handles of one core generation's permission
/// pipe. The worker serializes bounded requests; its sender is not exposed
/// through frontend IPC or to an extension guest. Grant/Revoke callers must
/// still prove they originate from the launcher's native confirmation UI.
pub(super) struct PermissionControlChannel {
    pub(super) requests: SyncSender<PermissionExchange>,
}

pub(super) fn serve_permission_control_worker<R: Read, W: Write>(
    mut input: W,
    mut output: R,
    pending: Receiver<PermissionExchange>,
) {
    for (request, answer) in pending {
        let result = write_permission_control_request(&mut input, &request)
            .and_then(|()| read_permission_control_reply(&mut output));
        let failed = result.is_err();
        let _ = answer.send(result);
        if failed {
            break;
        }
    }
    // Closing input tells core to revoke every optional grant made on
    // this parent's pipe, including after a failed or unanswered exchange.
}

impl PermissionControlChannel {
    pub(super) fn new(input: ChildStdin, output: ChildStdout) -> io::Result<Self> {
        let (requests, pending) = mpsc::sync_channel::<PermissionExchange>(1);
        thread::Builder::new()
            .name("blueice-permission-control".into())
            .spawn(move || serve_permission_control_worker(input, output, pending))?;
        Ok(Self { requests })
    }

    pub(super) fn inspect(&self) -> io::Result<PermissionControlReply> {
        self.exchange(
            PermissionControlRequest::Inspect,
            PERMISSION_INSPECT_TIMEOUT,
        )
    }

    pub(super) fn exchange(
        &self,
        request: PermissionControlRequest,
        timeout: Duration,
    ) -> io::Result<PermissionControlReply> {
        let (answer, reply) = mpsc::channel();
        self.requests
            .try_send((request, answer))
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "core permission control is busy")
                }
                TrySendError::Disconnected(_) => {
                    io::Error::new(io::ErrorKind::BrokenPipe, "core permission pipe is closed")
                }
            })?;
        match reply.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "core permission control timed out",
            )),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "core permission control ended without a reply",
            )),
        }
    }
}

/// The sole compiler profile the launcher may ask its child to register.
/// This label is an implementation detail of the trusted startup edge;
/// it is not a caller-selectable profile or a compiler IPC field.
pub(super) const CORE_CLOSED_COMPILER_PROJECT_PROFILE: &str = "core-closed-fixture-v1";

/// Keep selected filesystem endpoints comfortably below the smallest
/// common `sockaddr_un.sun_path` capacity.  The actual platform capacity
/// varies, so a conservative launcher-side limit rejects a bad setup
/// before a core (or an optional BlueJS host) is spawned.
pub(super) const MAX_STABLE_ENDPOINT_SOCKET_PATH_BYTES: usize = 100;
