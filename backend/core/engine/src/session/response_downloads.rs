// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An owned navigation offer, followed by a bounded original-body handoff.

use super::navigation::{Completion, PendingKind};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_FORWARDERS: usize = 16;
static FORWARDERS: AtomicUsize = AtomicUsize::new(0);

struct Forwarder;
impl Drop for Forwarder {
    fn drop(&mut self) {
        FORWARDERS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct PendingDownload {
    pub sequence: u64,
    pub document: u64,
    pub response: blueice_net::FetchedDownload,
    pub created: Instant,
}
impl PendingDownload {
    pub fn is_current(
        &self,
        tabs: &TabManager,
        sequences: &HashMap<TabId, u64>,
        tab: TabId,
    ) -> bool {
        self.created.elapsed() < Duration::from_secs(30)
            && sequences.get(&tab) == Some(&self.sequence)
            && tabs
                .get(tab)
                .is_some_and(|page| page.document_generation() == self.document)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn continue_download<S: Write>(
    tabs: &TabManager,
    stream: &mut S,
    tab: TabId,
    request_id: Option<u64>,
    sequence: u64,
    accept: bool,
    sequences: &HashMap<TabId, u64>,
    pending: &mut HashMap<TabId, PendingDownload>,
    tx: &mpsc::Sender<Completion>,
) -> io::Result<()> {
    if !pending
        .get(&tab)
        .is_some_and(|offer| offer.sequence == sequence && offer.is_current(tabs, sequences, tab))
    {
        return write_error(
            stream,
            Some(tab.as_u64()),
            request_id,
            "The navigation download offer is stale".into(),
        );
    }
    // Taking the response retires this exact offer; duplicate decisions can
    // never consume the same body twice. After acceptance the manager owns it.
    let offer = pending.remove(&tab).expect("validated offer");
    if !accept {
        return write_error(
            stream,
            Some(tab.as_u64()),
            request_id,
            "The native download service is unavailable".into(),
        );
    }
    if FORWARDERS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_FORWARDERS).then_some(count + 1)
        })
        .is_err()
    {
        return write_error(
            stream,
            Some(tab.as_u64()),
            request_id,
            "Too many original responses are still being transferred".into(),
        );
    }
    let socket = tabs
        .downloads_source()
        .map(|source| source.socket().to_owned())
        .unwrap_or_else(blueice_ipc::downloads::default_downloads_socket_path);
    let permit = Forwarder;
    let tx = tx.clone();
    thread::spawn(move || {
        let _permit = permit;
        let report = |outcome| {
            let _ = tx.send(Completion {
                tab_id: tab,
                seq: sequence,
                request_id,
                kind: PendingKind::Navigate,
                outcome,
                translations: None,
            });
        };
        let mut started = false;
        let result = forward(&socket, offer.response, |id| {
            started = true;
            report(NavOutcome::DownloadStarted { transfer_id: id });
        });
        if let Err(error) = result {
            // Once Started is acknowledged, the download service reports any
            // body failure through its normal transfer record and native shelf.
            if !started {
                report(NavOutcome::FetchFailed {
                    message: format!("Original response download failed: {error}"),
                });
            }
        }
    });
    Ok(())
}

#[cfg(unix)]
fn forward(
    socket_path: &Path,
    response: blueice_net::FetchedDownload,
    mut started: impl FnMut(u64),
) -> io::Result<()> {
    use blueice_ipc::downloads::{
        read_downloads_reply, write_downloads_request, write_response_chunk, DownloadsReply,
        DownloadsRequest, ResponseDownload, MAX_RESPONSE_CHUNK_BYTES,
    };
    use std::os::unix::net::UnixStream;
    let metadata = ResponseDownload {
        url: response.final_url.clone(),
        status: response.status,
        content_type: response.content_type.clone(),
        content_disposition: response.content_disposition.clone(),
        total_bytes: response.content_length,
    };
    let mut socket = UnixStream::connect(socket_path)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    write_downloads_request(
        &mut socket,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: 1,
        },
    )?;
    if !matches!(
        read_downloads_reply(&mut socket)?,
        (
            Some(1),
            DownloadsReply::Hello {
                protocol_version: 1
            }
        )
    ) {
        return Err(io::Error::other("download service handshake failed"));
    }
    write_downloads_request(
        &mut socket,
        Some(2),
        &DownloadsRequest::StartResponse { response: metadata },
    )?;
    let (request, reply) = read_downloads_reply(&mut socket)?;
    let DownloadsReply::Started(info) = reply else {
        return Err(io::Error::other(
            "download service refused the original response",
        ));
    };
    if request != Some(2) {
        return Err(io::Error::other("download service reply is stale"));
    }
    let _watch = ResponseWatch::start(socket.try_clone()?, response.cancellation())?;
    started(info.id);
    let mut body = response.into_reader();
    let mut buffer = [0; MAX_RESPONSE_CHUNK_BYTES];
    loop {
        let count = body.read(&mut buffer)?;
        write_response_chunk(&mut socket, &buffer[..count])?;
        if count == 0 {
            return Ok(());
        }
    }
}

#[cfg(unix)]
struct ResponseWatch {
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    socket: std::os::unix::net::UnixStream,
}
#[cfg(unix)]
impl ResponseWatch {
    fn start(
        mut socket: std::os::unix::net::UnixStream,
        cancellation: blueice_net::ResponseCancellation,
    ) -> io::Result<Self> {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        socket.set_read_timeout(Some(Duration::from_millis(100)))?;
        let interrupt = socket.try_clone()?;
        let finished = Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        let thread = thread::spawn(move || {
            let mut byte = [0];
            while !done.load(Ordering::Acquire) {
                match socket.read(&mut byte) {
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue
                    }
                    _ => {
                        // No frames follow Started on this dedicated socket.
                        // EOF/refusal cancels a body even while its origin stalls.
                        if !done.load(Ordering::Acquire) {
                            cancellation.cancel();
                        }
                        break;
                    }
                }
            }
        });
        Ok(Self {
            finished,
            thread: Some(thread),
            socket: interrupt,
        })
    }
}
#[cfg(unix)]
impl Drop for ResponseWatch {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
        let _ = self.socket.shutdown(std::net::Shutdown::Read);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(not(unix))]
fn forward(
    _socket_path: &Path,
    _response: blueice_net::FetchedDownload,
    _started: impl FnMut(u64),
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "original response streaming is unavailable on this platform",
    ))
}
