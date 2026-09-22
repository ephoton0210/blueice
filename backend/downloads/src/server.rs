// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Unix-socket front of the downloads process: one thread per
//! connection, the `blueice_ipc::downloads` protocol in and out, and a
//! writer thread per *subscribed* connection so pushes never wait behind a
//! request handler (`phase-10-download-manager/PLAN.md`'s "Wire protocol").
//!
//! A slow or stuck client only ever delays itself: the manager fans updates
//! out into a per-subscriber set that keeps only the latest record of each
//! transfer, and each connection writes on its own threads.

use crate::manager::{ManagerError, SubscriptionUpdate, TransferManager};
use blueice_ipc::downloads::{
    read_downloads_request, write_downloads_reply, DownloadsReply, DownloadsRequest, ErrorCode,
    DOWNLOADS_PROTOCOL_VERSION,
};
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Local clients are still untrusted peers.  Bound their resource use so a
/// process on the same account cannot create arbitrarily many stuck reader
/// and writer threads.
const MAX_CONNECTIONS: usize = 64;
/// A peer that never speaks must not occupy one of `MAX_CONNECTIONS` forever.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Accepts connections on `listener` until `stop` is set (by the caller, or
/// by a client's `Shutdown` request). Blocks; run it on its own thread.
pub fn serve(listener: UnixListener, manager: Arc<TransferManager>, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    let connections = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let Some(permit) = ConnectionPermit::try_acquire(connections.clone()) else {
                    // Dropping the accepted socket is deliberate: admitting it
                    // would exceed the bounded worker budget.
                    drop(stream);
                    continue;
                };
                let (manager, stop) = (manager.clone(), stop.clone());
                thread::spawn(move || {
                    let _permit = permit;
                    let _ = handle_connection(stream, &manager, &stop);
                });
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

struct ConnectionPermit(Arc<AtomicUsize>);

impl ConnectionPermit {
    fn try_acquire(active: Arc<AtomicUsize>) -> Option<Self> {
        let mut observed = active.load(Ordering::Relaxed);
        loop {
            if observed >= MAX_CONNECTIONS {
                return None;
            }
            match active.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(ConnectionPermit(active)),
                Err(current) => observed = current,
            }
        }
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

type SharedWriter = Arc<Mutex<UnixStream>>;

fn send(writer: &SharedWriter, request_id: Option<u64>, reply: &DownloadsReply) -> io::Result<()> {
    write_downloads_reply(
        &mut *writer.lock().unwrap_or_else(|p| p.into_inner()),
        request_id,
        reply,
    )
}

fn error_reply(code: ErrorCode, message: impl Into<String>) -> DownloadsReply {
    DownloadsReply::Error {
        code,
        message: message.into(),
    }
}

fn refusal(e: ManagerError) -> DownloadsReply {
    DownloadsReply::Error {
        code: e.code,
        message: e.message,
    }
}

fn handle_connection(
    stream: UnixStream,
    manager: &Arc<TransferManager>,
    stop: &Arc<AtomicBool>,
) -> io::Result<()> {
    handle_connection_with_hello_timeout(stream, manager, stop, HELLO_TIMEOUT)
}

fn handle_connection_with_hello_timeout(
    stream: UnixStream,
    manager: &Arc<TransferManager>,
    stop: &Arc<AtomicBool>,
    hello_timeout: Duration,
) -> io::Result<()> {
    // `accept` inherits the listener's nonblocking flag on this platform;
    // the deadline reader below relies on a blocking read that `shutdown`
    // cancels, so normalize the accepted socket before handing it off.
    stream.set_nonblocking(false)?;
    let (mut reader, (id, first)) = read_hello_with_deadline(stream, hello_timeout)?;
    let write_half = reader.try_clone()?;
    write_half.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let writer: SharedWriter = Arc::new(Mutex::new(write_half));

    // The first message must be a Hello of a version this build speaks;
    // anything else ends the connection after saying why.
    match first {
        DownloadsRequest::Hello { protocol_version }
            if protocol_version == DOWNLOADS_PROTOCOL_VERSION =>
        {
            send(
                &writer,
                id,
                &DownloadsReply::Hello {
                    protocol_version: DOWNLOADS_PROTOCOL_VERSION,
                },
            )?;
        }
        DownloadsRequest::Hello { protocol_version } => {
            return send(&writer, id, &error_reply(ErrorCode::UnsupportedVersion, format!("this downloads process speaks protocol version {DOWNLOADS_PROTOCOL_VERSION}, not {protocol_version}")));
        }
        _ => {
            return send(
                &writer,
                id,
                &error_reply(
                    ErrorCode::InvalidRequest,
                    "the first message on a connection must be a Hello",
                ),
            )
        }
    }

    let closed = Arc::new(AtomicBool::new(false));
    let mut subscribed = false;
    while let Ok((id, request)) = read_downloads_request(&mut reader) {
        let reply = match request {
            DownloadsRequest::Hello { .. } => DownloadsReply::Hello {
                protocol_version: DOWNLOADS_PROTOCOL_VERSION,
            },
            DownloadsRequest::Start {
                url,
                dest,
                overwrite,
            } => manager
                .start(&url, dest.as_deref(), overwrite)
                .map(DownloadsReply::Started)
                .unwrap_or_else(refusal),
            DownloadsRequest::List { state } => DownloadsReply::Transfers(manager.list(state)),
            DownloadsRequest::Get { id } => manager
                .get(id)
                .map(DownloadsReply::Transfer)
                .unwrap_or_else(refusal),
            DownloadsRequest::Pause { id } => manager
                .pause(id)
                .map(DownloadsReply::Transfer)
                .unwrap_or_else(refusal),
            DownloadsRequest::Resume { id } => manager
                .resume(id)
                .map(DownloadsReply::Transfer)
                .unwrap_or_else(refusal),
            DownloadsRequest::Cancel { id } => manager
                .cancel(id)
                .map(DownloadsReply::Transfer)
                .unwrap_or_else(refusal),
            DownloadsRequest::Remove { id } => manager
                .remove(id)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::SetSftpPassword {
                host,
                port,
                username,
                password,
            } => manager
                .set_sftp_password(&host, port, &username, &password)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::RemoveSftpPassword {
                host,
                port,
                username,
            } => manager
                .remove_sftp_password(&host, port, &username)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::SetSftpPrivateKeyPassphrase {
                host,
                port,
                username,
                passphrase,
            } => manager
                .set_sftp_private_key_passphrase(&host, port, &username, &passphrase)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::RemoveSftpPrivateKeyPassphrase {
                host,
                port,
                username,
            } => manager
                .remove_sftp_private_key_passphrase(&host, port, &username)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::SetFtpsPassword {
                host,
                port,
                username,
                password,
            } => manager
                .set_ftps_password(&host, port, &username, &password)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::RemoveFtpsPassword {
                host,
                port,
                username,
            } => manager
                .remove_ftps_password(&host, port, &username)
                .map(|()| DownloadsReply::Ok)
                .unwrap_or_else(refusal),
            DownloadsRequest::Subscribe => {
                if !subscribed {
                    subscribed = true;
                    spawn_pusher(manager, writer.clone(), closed.clone());
                }
                DownloadsReply::Ok
            }
            DownloadsRequest::Shutdown => {
                manager.shutdown();
                stop.store(true, Ordering::SeqCst);
                DownloadsReply::Ok
            }
            DownloadsRequest::Unknown => {
                error_reply(ErrorCode::InvalidRequest, "unrecognized request")
            }
        };
        if send(&writer, id, &reply).is_err() {
            break;
        }
    }
    closed.store(true, Ordering::SeqCst);
    Ok(())
}

/// Reads one complete Hello frame without relying on platform-specific socket
/// read-timeout support. On timeout the controlling thread shuts the socket
/// down, waits for the reader to leave its blocking read, and only then
/// returns, so neither a thread nor an fd can outlive the connection permit.
fn read_hello_with_deadline(
    stream: UnixStream,
    timeout: Duration,
) -> io::Result<(UnixStream, (Option<u64>, DownloadsRequest))> {
    let interrupt = stream.try_clone()?;
    let (sent, received) = sync_channel(1);
    thread::spawn(move || {
        let mut reader = stream;
        let result = read_downloads_request(&mut reader);
        let _ = sent.send((reader, result));
    });
    match received.recv_timeout(timeout) {
        Ok((reader, result)) => result.map(|hello| (reader, hello)),
        Err(RecvTimeoutError::Timeout) => {
            // `shutdown` is the cancellation mechanism for the reader above;
            // it makes even a partial frame's `read_exact` return promptly.
            let _ = interrupt.shutdown(std::net::Shutdown::Both);
            let _ = received.recv();
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for the downloads Hello frame",
            ))
        }
        Err(RecvTimeoutError::Disconnected) => Err(io::Error::other(
            "the downloads Hello reader stopped unexpectedly",
        )),
    }
}

/// Forwards every update the manager publishes to this connection, until
/// the connection closes or a write fails. Pushes carry no request id.
fn spawn_pusher(manager: &Arc<TransferManager>, writer: SharedWriter, closed: Arc<AtomicBool>) {
    let updates = manager.subscribe();
    thread::spawn(move || loop {
        match updates.recv_timeout(Duration::from_millis(500)) {
            Some(SubscriptionUpdate::Updated(info)) => {
                if send(&writer, None, &DownloadsReply::Updated(*info)).is_err() {
                    return;
                }
            }
            Some(SubscriptionUpdate::Removed { id }) => {
                if send(&writer, None, &DownloadsReply::Removed { id }).is_err() {
                    return;
                }
            }
            None => {
                if closed.load(Ordering::SeqCst) {
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_permits_are_bounded_and_returned_on_drop() {
        let active = Arc::new(AtomicUsize::new(0));
        let mut permits: Vec<ConnectionPermit> = (0..MAX_CONNECTIONS)
            .map(|_| ConnectionPermit::try_acquire(active.clone()).expect("capacity remains"))
            .collect();
        assert!(ConnectionPermit::try_acquire(active.clone()).is_none());
        permits.pop();
        assert!(ConnectionPermit::try_acquire(active).is_some());
    }

    #[test]
    fn a_silent_or_partial_hello_times_out_and_joins_its_reader() {
        let (mut client, server) = UnixStream::pair().unwrap();
        // Send only half the frame length: the deadline applies to the whole
        // frame, not merely its first readable byte.
        use std::io::Write;
        client.write_all(&[0, 0]).unwrap();
        let error = read_hello_with_deadline(server, Duration::from_millis(20)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
