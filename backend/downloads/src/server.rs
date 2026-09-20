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

use crate::manager::{ManagerError, TransferManager};
use blueice_ipc::downloads::{read_downloads_request, write_downloads_reply, DownloadsReply, DownloadsRequest, ErrorCode, DOWNLOADS_PROTOCOL_VERSION};
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Accepts connections on `listener` until `stop` is set (by the caller, or
/// by a client's `Shutdown` request). Blocks; run it on its own thread.
pub fn serve(listener: UnixListener, manager: Arc<TransferManager>, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let (manager, stop) = (manager.clone(), stop.clone());
                thread::spawn(move || {
                    let _ = handle_connection(stream, &manager, &stop);
                });
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

type SharedWriter = Arc<Mutex<UnixStream>>;

fn send(writer: &SharedWriter, request_id: Option<u64>, reply: &DownloadsReply) -> io::Result<()> {
    write_downloads_reply(&mut *writer.lock().unwrap_or_else(|p| p.into_inner()), request_id, reply)
}

fn error_reply(code: ErrorCode, message: impl Into<String>) -> DownloadsReply {
    DownloadsReply::Error { code, message: message.into() }
}

fn refusal(e: ManagerError) -> DownloadsReply {
    DownloadsReply::Error { code: e.code, message: e.message }
}

fn handle_connection(stream: UnixStream, manager: &Arc<TransferManager>, stop: &Arc<AtomicBool>) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let writer: SharedWriter = Arc::new(Mutex::new(stream.try_clone()?));
    let mut reader = stream;

    // The first message must be a Hello of a version this build speaks;
    // anything else ends the connection after saying why.
    let (id, first) = read_downloads_request(&mut reader)?;
    match first {
        DownloadsRequest::Hello { protocol_version } if protocol_version == DOWNLOADS_PROTOCOL_VERSION => {
            send(&writer, id, &DownloadsReply::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION })?;
        }
        DownloadsRequest::Hello { protocol_version } => {
            return send(&writer, id, &error_reply(ErrorCode::UnsupportedVersion, format!("this downloads process speaks protocol version {DOWNLOADS_PROTOCOL_VERSION}, not {protocol_version}")));
        }
        _ => return send(&writer, id, &error_reply(ErrorCode::InvalidRequest, "the first message on a connection must be a Hello")),
    }

    let closed = Arc::new(AtomicBool::new(false));
    let mut subscribed = false;
    while let Ok((id, request)) = read_downloads_request(&mut reader) {
        let reply = match request {
            DownloadsRequest::Hello { .. } => DownloadsReply::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION },
            DownloadsRequest::Start { url, dest, overwrite } => manager.start(&url, dest.as_deref(), overwrite).map(DownloadsReply::Started).unwrap_or_else(refusal),
            DownloadsRequest::List { state } => DownloadsReply::Transfers(manager.list(state)),
            DownloadsRequest::Get { id } => manager.get(id).map(DownloadsReply::Transfer).unwrap_or_else(refusal),
            DownloadsRequest::Pause { id } => manager.pause(id).map(DownloadsReply::Transfer).unwrap_or_else(refusal),
            DownloadsRequest::Resume { id } => manager.resume(id).map(DownloadsReply::Transfer).unwrap_or_else(refusal),
            DownloadsRequest::Cancel { id } => manager.cancel(id).map(DownloadsReply::Transfer).unwrap_or_else(refusal),
            DownloadsRequest::Remove { id } => manager.remove(id).map(|()| DownloadsReply::Ok).unwrap_or_else(refusal),
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
            DownloadsRequest::Unknown => error_reply(ErrorCode::InvalidRequest, "unrecognized request"),
        };
        if send(&writer, id, &reply).is_err() {
            break;
        }
    }
    closed.store(true, Ordering::SeqCst);
    Ok(())
}

/// Forwards every update the manager publishes to this connection, until
/// the connection closes or a write fails. Pushes carry no request id.
fn spawn_pusher(manager: &Arc<TransferManager>, writer: SharedWriter, closed: Arc<AtomicBool>) {
    let updates = manager.subscribe();
    thread::spawn(move || loop {
        match updates.recv_timeout(Duration::from_millis(500)) {
            Some(info) => {
                if send(&writer, None, &DownloadsReply::Updated(info)).is_err() {
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
