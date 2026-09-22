// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `about:downloads` page (`phase-10-download-manager/PLAN.md`'s
//! "Visualization: `about:downloads`"): a built-in page, generated per
//! request like `about:credits`, rendered through BlueIce's own
//! HTML/CSS/layout/paint pipeline. That is the point of doing it this
//! way: the progress bars a human sees and the accessibility-tree
//! snapshot an AI agent reads come from the same render pass, not from
//! a native panel only one frontend could draw.
//!
//! Two halves. [`downloads_html`] is a pure function from a list of
//! [`TransferInfo`] (or "the service is unreachable") to HTML, localized
//! through `blueice-i18n`'s `downloads` namespace. [`DownloadsSource`]
//! fetches that list from the downloads process -- with hard time bounds,
//! since it is called from `core`'s session loop and a hung downloads
//! process must never be able to stall the tabs and clients sharing it.
//!
//! **Everything from a remote server is escaped** ([`escape_html`]):
//! file names, error messages, and the gatekeeper's reasons are
//! attacker-influenced text, and this page is rendered as HTML by the
//! engine itself.

use blueice_ipc::downloads::{
    DownloadsClient, SegmentInfo, SingleStreamReason, TransferInfo, TransferMode, TransferState,
    default_downloads_socket_path, format_bytes, format_duration, format_speed,
};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The well-known URL `Page` recognizes as a request for the downloads
/// page. An optional `?lang=<locale>` picks a locale, exactly like
/// `about:credits`.
pub const DOWNLOADS_URL: &str = "about:downloads";

pub fn is_downloads_url(url: &str) -> bool {
    url == DOWNLOADS_URL || url.starts_with("about:downloads?")
}

/// How long the *quick* read used when a page is first opened may take: a
/// local socket answers in about a millisecond, so this only ever matters
/// when the process is hung, and then it bounds the stall.
const QUICK_TIMEOUT: Duration = Duration::from_millis(300);
/// How long opening the page may wait for a not-yet-running downloads
/// process to start and answer.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(5);
/// The most per-connection bars shown for one transfer.
const MAX_SEGMENT_BARS: usize = 32;

/// What the page shows.
pub enum DownloadsView<'a> {
    Transfers(&'a [TransferInfo]),
    /// The downloads process could not be reached -- not the same as having
    /// no downloads, and said so.
    Unavailable,
}

/// HTML-escapes text for use in element content or a quoted attribute.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

const STYLE: &str = "body { padding: 12px; font-size: 15px; color: #222222; } \
    .summary { color: #666666; } \
    .transfer { margin-top: 12px; padding: 8px; border-width: 1px; border-style: solid; border-color: #cccccc; } \
    .name { font-weight: bold; } \
    .status { margin-top: 4px; } \
    .bar { height: 12px; background-color: #dddddd; } \
    .fill { height: 12px; background-color: #2f7de1; } \
    .state-completed .fill { background-color: #3a9d3a; } \
    .state-failed .fill, .state-blocked .fill { background-color: #c0392b; } \
    .state-paused .fill, .state-queued .fill, .state-awaiting-clearance .fill { background-color: #999999; } \
    .detail { color: #444444; font-size: 13px; margin-top: 3px; } \
    .error { color: #b00020; } \
    .seg { height: 5px; margin-top: 2px; background-color: #eeeeee; } \
    .segfill { height: 5px; background-color: #6aa6ee; }";

/// Running transfers first, then everything else; newest first within each.
fn sorted(transfers: &[TransferInfo]) -> Vec<&TransferInfo> {
    let running = |t: &TransferInfo| {
        matches!(
            t.state,
            TransferState::Active | TransferState::AwaitingClearance | TransferState::Queued
        )
    };
    let mut all: Vec<&TransferInfo> = transfers.iter().collect();
    all.sort_by_key(|t| (!running(t), std::cmp::Reverse(t.id)));
    all
}

/// The name shown for a transfer: the destination's file name, else the
/// URL's last path segment, else the whole URL.
fn display_name(info: &TransferInfo) -> String {
    let from_dest = info.dest_path.rsplit('/').next().filter(|n| !n.is_empty());
    match from_dest {
        Some(name) => name.to_string(),
        None => blueice_net::download::file_name::file_name_from_url(&info.url)
            .unwrap_or_else(|| info.url.clone()),
    }
}

/// Whole percent complete. Never `100` before a transfer is actually
/// complete, and `0` while the total is unknown.
fn percent(info: &TransferInfo) -> u32 {
    match info.fraction_complete() {
        Some(_) if info.state == TransferState::Completed => 100,
        Some(fraction) => ((fraction * 100.0).round() as u32).min(99),
        None if info.state == TransferState::Completed => 100,
        None => 0,
    }
}

fn segment_percent(segment: &SegmentInfo) -> u32 {
    let length = segment.end.saturating_sub(segment.start);
    if length == 0 {
        return 0;
    }
    ((segment.completed.min(length) * 100) / length) as u32
}

/// Builds the downloads page for `locale`, pulling every string through
/// `blueice-i18n`'s `downloads` namespace and escaping everything that came
/// from outside.
pub fn downloads_html(view: &DownloadsView<'_>, locale: &str) -> String {
    let locale = if blueice_i18n::SUPPORTED_LOCALES.contains(&locale) {
        locale
    } else {
        blueice_i18n::DEFAULT_LOCALE
    };
    let t = |key: &str| blueice_i18n::translate(locale, "downloads", key, &[]);

    let mut body = format!("<h1>{}</h1>", escape_html(&t("downloads-title")));
    match view {
        DownloadsView::Unavailable => body.push_str(&format!(
            "<p class=\"error\">{}</p>",
            escape_html(&t("downloads-unavailable"))
        )),
        DownloadsView::Transfers([]) => body.push_str(&format!(
            "<p class=\"summary\">{}</p>",
            escape_html(&t("downloads-empty"))
        )),
        DownloadsView::Transfers(transfers) => {
            let count = if transfers.len() == 1 {
                t("downloads-count-one")
            } else {
                format!("{} {}", transfers.len(), t("downloads-count-other"))
            };
            body.push_str(&format!("<p class=\"summary\">{}</p>", escape_html(&count)));
            for info in sorted(transfers) {
                push_transfer(&mut body, info, &t);
            }
        }
    }
    format!(
        "<html><head><title>{}</title><style>{STYLE}</style></head><body>{body}</body></html>",
        escape_html(&t("downloads-title"))
    )
}

fn state_label(state: TransferState, t: &dyn Fn(&str) -> String) -> String {
    t(match state {
        TransferState::Queued => "state-queued",
        TransferState::AwaitingClearance => "state-awaiting-clearance",
        TransferState::Active => "state-active",
        TransferState::Paused => "state-paused",
        TransferState::Completed => "state-completed",
        TransferState::Failed => "state-failed",
        TransferState::Cancelled => "state-cancelled",
        TransferState::Blocked => "state-blocked",
        TransferState::Unknown => "state-unknown",
    })
}

fn push_transfer(body: &mut String, info: &TransferInfo, t: &dyn Fn(&str) -> String) {
    let pct = percent(info);
    let progress = match info.total_bytes {
        Some(total) => format!(
            "{pct}% ({} {} {})",
            format_bytes(info.completed_bytes),
            t("label-of"),
            format_bytes(total)
        ),
        None => format!(
            "{} ({})",
            format_bytes(info.completed_bytes),
            t("label-total-unknown")
        ),
    };
    let class = format!("transfer state-{}", info.state.as_str().replace('_', "-"));
    body.push_str(&format!(
        "<div class=\"{class}\"><p class=\"name\">{}</p>",
        escape_html(&display_name(info))
    ));
    // "0 B (total size unknown)" says nothing about a transfer that has not
    // moved a byte or learned its size; show just its state then.
    let status = if info.completed_bytes > 0 || info.total_bytes.is_some() {
        format!("{} \u{2014} {progress}", state_label(info.state, t))
    } else {
        state_label(info.state, t)
    };
    body.push_str(&format!("<p class=\"status\">{}</p>", escape_html(&status)));
    body.push_str(&format!(
        "<div class=\"bar\"><div class=\"fill\" style=\"width: {pct}%\"></div></div>"
    ));

    let mut live: Vec<String> = Vec::new();
    if info.speed_bps > 0 {
        live.push(format!(
            "{} {}",
            t("label-speed"),
            format_speed(info.speed_bps)
        ));
    }
    if let Some(eta) = info.eta_secs {
        live.push(format!("{} {}", t("label-eta"), format_duration(eta)));
    }
    if info.connections > 0 {
        live.push(format!("{} {}", t("label-connections"), info.connections));
    }
    if !live.is_empty() {
        body.push_str(&format!(
            "<p class=\"detail\">{}</p>",
            escape_html(&live.join(" \u{b7} "))
        ));
    }

    let mode = match info.mode {
        TransferMode::Segmented => Some(t("mode-segmented")),
        TransferMode::SingleStream {
            reason: SingleStreamReason::UnknownLength,
        } => Some(t("mode-single-unknown-length")),
        TransferMode::SingleStream { .. } => Some(t("mode-single-stream")),
        TransferMode::Undetermined | TransferMode::Unknown => None,
    };
    if let Some(mode) = mode {
        body.push_str(&format!(
            "<p class=\"detail\">{} {}</p>",
            escape_html(&t("label-mode")),
            escape_html(&mode)
        ));
    }
    if info.retries > 0 {
        body.push_str(&format!(
            "<p class=\"detail\">{} {}</p>",
            escape_html(&t("label-retries")),
            info.retries
        ));
    }
    if let Some(error) = &info.last_error {
        body.push_str(&format!(
            "<p class=\"detail error\">{} {}</p>",
            escape_html(&t("label-last-error")),
            escape_html(error)
        ));
    }
    if let Some(blocked) = &info.blocked {
        body.push_str(&format!(
            "<p class=\"detail error\">{} {} ({})</p>",
            escape_html(&t("label-blocked")),
            escape_html(&blocked.reason),
            escape_html(&blocked.category)
        ));
    }
    if info.state == TransferState::Completed {
        body.push_str(&format!(
            "<p class=\"detail\">{} {}</p>",
            escape_html(&t("label-saved-to")),
            escape_html(&info.dest_path)
        ));
    }
    if info.state == TransferState::Paused && !info.resume_safe {
        body.push_str(&format!(
            "<p class=\"detail\">{}</p>",
            escape_html(&t("note-resume-unsafe"))
        ));
    }
    if info.segments.len() >= 2
        && matches!(
            info.state,
            TransferState::Active | TransferState::Paused | TransferState::Failed
        )
    {
        body.push_str(&format!(
            "<p class=\"detail\">{}</p>",
            escape_html(&t("segments-heading"))
        ));
        for segment in info.segments.iter().take(MAX_SEGMENT_BARS) {
            body.push_str(&format!(
                "<div class=\"seg\"><div class=\"segfill\" style=\"width: {}%\"></div></div>",
                segment_percent(segment)
            ));
        }
    }
    body.push_str("</div>");
}

/// Starts the downloads process if nobody has.
pub type Spawner = Box<dyn Fn() -> io::Result<()> + Send + Sync>;

/// Spawns `blueice-downloads` from next to this binary (a `cargo test`
/// binary sits one level deeper, in `deps/`, so step back out of that),
/// listening on `socket`, detached: the process is shared with every other
/// client, and a helper thread reaps it when it exits.
fn spawn_sibling_downloads_process(socket: &Path) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    let mut child = Command::new(dir.join("blueice-downloads"))
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Where the page's data comes from: the downloads process's socket. Every
/// read is bounded in time, because `core`'s session loop calls it and one
/// hung process must not be able to stall every tab and client sharing that
/// loop.
pub struct DownloadsSource {
    socket: PathBuf,
    spawner: Option<Spawner>,
    timeout: Duration,
}

impl DownloadsSource {
    /// The production source: the well-known socket, starting the sibling
    /// `blueice-downloads` binary when a page opens and nothing is listening.
    pub fn new() -> Self {
        DownloadsSource::at(default_downloads_socket_path())
    }

    /// Like [`Self::new`] for a downloads process on `socket` -- which is
    /// also where a process started on the page's behalf will listen.
    pub fn at(socket: PathBuf) -> Self {
        let for_spawn = socket.clone();
        DownloadsSource::with_spawner(
            socket,
            Box::new(move || spawn_sibling_downloads_process(&for_spawn)),
            SPAWN_TIMEOUT,
        )
    }

    /// A source that never starts anything (tests, and callers that only
    /// want to observe a process someone else runs).
    pub fn without_spawner(socket: PathBuf) -> Self {
        DownloadsSource {
            socket,
            spawner: None,
            timeout: SPAWN_TIMEOUT,
        }
    }

    pub fn with_spawner(socket: PathBuf, spawner: Spawner, timeout: Duration) -> Self {
        DownloadsSource {
            socket,
            spawner: Some(spawner),
            timeout,
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The list right now, without starting anything and giving up after a
    /// fraction of a second -- what a navigation to the page uses, so the
    /// first paint already has real data when the process is up.
    pub fn fetch_quick(&self) -> Result<Vec<TransferInfo>, String> {
        self.fetch_within(false, QUICK_TIMEOUT)
    }

    /// The list, starting the downloads process first if it is not running
    /// (`research/multi-process-memory.md` names opening the downloads panel
    /// as a legitimate reason to start it). Meant for a background thread.
    pub fn fetch_spawning(&self) -> Result<Vec<TransferInfo>, String> {
        self.fetch_within(true, self.timeout)
    }

    /// The list, given the source's full time budget but never starting
    /// anything: how an already-open page keeps itself current. Meant for a
    /// background thread.
    pub fn fetch_observing(&self) -> Result<Vec<TransferInfo>, String> {
        self.fetch_within(false, self.timeout)
    }

    fn fetch_within(&self, spawn: bool, limit: Duration) -> Result<Vec<TransferInfo>, String> {
        let deadline = Instant::now() + limit;
        let stream = match UnixStream::connect(&self.socket) {
            Ok(stream) => stream,
            Err(e) if !spawn => return Err(format!("the downloads service is not running ({e})")),
            Err(_) => {
                let Some(spawner) = &self.spawner else {
                    return Err(
                        "the downloads service is not running and cannot be started from here"
                            .to_string(),
                    );
                };
                spawner()
                    .map_err(|e| format!("the downloads service could not be started: {e}"))?;
                loop {
                    match UnixStream::connect(&self.socket) {
                        Ok(stream) => break stream,
                        Err(_) if Instant::now() < deadline => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => return Err(
                            "the downloads service was started but did not start listening in time"
                                .to_string(),
                        ),
                    }
                }
            }
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("the downloads service did not answer in time".to_string());
        }
        // `UnixStream::set_read_timeout` is not portable: macOS rejects it
        // for these sockets.  Instead, run the complete exchange on a
        // bounded helper and cancel its blocking read *or write* by shutting
        // down a clone.  We then join it before returning, so a peer that
        // sends a partial frame cannot accumulate a thread or descriptor on
        // every page refresh.
        let interrupt = stream
            .try_clone()
            .map_err(|e| format!("could not prepare the downloads request: {e}"))?;
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let result = DownloadsClient::connect(stream)
                .and_then(|mut client| client.list(None))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
        match rx.recv_timeout(remaining) {
            Ok(result) => {
                reader
                    .join()
                    .map_err(|_| "the downloads request worker panicked".to_string())?;
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = interrupt.shutdown(Shutdown::Both);
                // `shutdown` makes the blocking exchange return; wait and
                // join before this source call releases its resources.
                let _ = rx.recv();
                let _ = reader.join();
                Err("the downloads service did not answer in time".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = reader.join();
                Err("the downloads request worker stopped unexpectedly".to_string())
            }
        }
    }
}

impl Default for DownloadsSource {
    fn default() -> Self {
        DownloadsSource::new()
    }
}

/// A fake downloads process on a real Unix socket, shared by this module's
/// own tests and by `Page`'s and the session's.
#[cfg(test)]
pub(crate) mod test_support {
    use blueice_ipc::downloads::{
        DownloadsReply, DownloadsRequest, TransferInfo, read_downloads_request,
        write_downloads_reply,
    };
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    pub(crate) struct Scratch(pub(crate) std::path::PathBuf);

    impl Scratch {
        pub(crate) fn new(label: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "be-dl-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        pub(crate) fn socket(&self) -> std::path::PathBuf {
            self.0.join("d.sock")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What a fake downloads process serves, and how often it was asked --
    /// shared with the test so the list can change while a session runs.
    #[derive(Clone, Default)]
    pub(crate) struct FakeState {
        pub(crate) transfers: Arc<Mutex<Vec<TransferInfo>>>,
        /// `List` requests answered so far.
        pub(crate) lists: Arc<AtomicUsize>,
        /// Connections that reached the deliberately non-responsive server.
        /// Tests use this as a synchronization point before asserting that a
        /// hung background exchange cannot block the core session loop.
        pub(crate) stalls: Arc<AtomicUsize>,
    }

    /// Answers `Hello` and `List` with `transfers`; `stall` makes it accept
    /// the connection and then never answer.
    pub(crate) fn fake_downloads(
        socket: &std::path::Path,
        transfers: Vec<TransferInfo>,
        stall: bool,
        version: u32,
    ) -> thread::JoinHandle<()> {
        fake_downloads_live(
            socket,
            FakeState {
                transfers: Arc::new(Mutex::new(transfers)),
                lists: Arc::default(),
                ..FakeState::default()
            },
            stall,
            version,
        )
    }

    pub(crate) fn fake_downloads_live(
        socket: &std::path::Path,
        state: FakeState,
        stall: bool,
        version: u32,
    ) -> thread::JoinHandle<()> {
        let listener = UnixListener::bind(socket).unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let state = state.clone();
                thread::spawn(move || {
                    while let Ok((id, request)) = read_downloads_request(&mut stream) {
                        if stall {
                            state.stalls.fetch_add(1, Ordering::SeqCst);
                            thread::sleep(Duration::from_secs(30));
                            return;
                        }
                        let reply = match request {
                            DownloadsRequest::Hello { .. } => DownloadsReply::Hello {
                                protocol_version: version,
                            },
                            DownloadsRequest::List { .. } => {
                                state.lists.fetch_add(1, Ordering::SeqCst);
                                DownloadsReply::Transfers(state.transfers.lock().unwrap().clone())
                            }
                            _ => DownloadsReply::Ok,
                        };
                        if write_downloads_reply(&mut stream, id, &reply).is_err() {
                            return;
                        }
                    }
                });
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{Scratch, fake_downloads};
    use super::*;
    use blueice_ipc::downloads::{BlockedInfo, DOWNLOADS_PROTOCOL_VERSION, SegmentState};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Instant;

    const MIB: u64 = 1024 * 1024;

    fn transfer(id: u64, state: TransferState) -> TransferInfo {
        TransferInfo {
            id,
            url: format!("https://example.com/files/file{id}.iso"),
            dest_path: format!("/home/u/Downloads/BlueIce/file{id}.iso"),
            state,
            total_bytes: Some(3 * MIB),
            completed_bytes: MIB + MIB / 5,
            ..TransferInfo::default()
        }
    }

    fn en(transfers: &[TransferInfo]) -> String {
        downloads_html(&DownloadsView::Transfers(transfers), "en")
    }

    // ---- the URL ------------------------------------------------------

    #[test]
    fn the_downloads_url_uses_the_about_scheme_and_may_carry_a_language() {
        assert_eq!(DOWNLOADS_URL, "about:downloads");
        assert!(is_downloads_url("about:downloads"));
        assert!(is_downloads_url("about:downloads?lang=zh-TW"));
        for other in [
            "about:downloadsx",
            "about:credits",
            "about:blank",
            "https://example.com/about:downloads",
            "downloads",
        ] {
            assert!(!is_downloads_url(other), "{other}");
        }
    }

    // ---- the page ---------------------------------------------------------

    #[test]
    fn an_empty_list_says_so_and_shows_no_transfer_rows() {
        let html = en(&[]);
        assert!(
            html.contains("<title>Downloads</title>") && html.contains("<h1>Downloads</h1>"),
            "{html}"
        );
        assert!(html.contains("No downloads yet."));
        assert!(!html.contains("class=\"transfer"));
    }

    #[test]
    fn an_unreachable_service_is_reported_instead_of_an_empty_list() {
        let html = downloads_html(&DownloadsView::Unavailable, "en");
        assert!(
            html.contains("The downloads service is not running"),
            "{html}"
        );
        assert!(
            !html.contains("No downloads yet."),
            "an unreachable service is not the same as having no downloads"
        );
    }

    #[test]
    fn a_running_transfer_shows_name_status_progress_bar_and_live_figures() {
        let mut t = transfer(1, TransferState::Active);
        t.speed_bps = 12 * MIB + MIB / 4;
        t.eta_secs = Some(80);
        t.connections = 8;
        t.mode = TransferMode::Segmented;
        let html = en(&[t]);
        assert!(
            html.contains("file1.iso"),
            "the name is the destination's file name: {html}"
        );
        assert!(html.contains("Downloading"), "{html}");
        assert!(html.contains("40% (1.2 MiB of 3.0 MiB)"), "{html}");
        assert!(
            html.contains("style=\"width: 40%\""),
            "the bar's fill is 40% wide: {html}"
        );
        assert!(
            html.contains("Speed:") && html.contains("12.3 MiB/s"),
            "{html}"
        );
        assert!(
            html.contains("Time left:") && html.contains("1 min 20 s"),
            "{html}"
        );
        assert!(
            html.contains("Connections:") && html.contains('8'),
            "{html}"
        );
        assert!(html.contains("several connections at once"), "{html}");
    }

    #[test]
    fn each_connection_gets_its_own_progress_bar_when_there_are_several() {
        let mut t = transfer(1, TransferState::Active);
        t.segments = vec![
            SegmentInfo {
                start: 0,
                end: 1_000,
                completed: 750,
                state: SegmentState::Active,
            },
            SegmentInfo {
                start: 1_000,
                end: 2_000,
                completed: 1_000,
                state: SegmentState::Done,
            },
            SegmentInfo {
                start: 2_000,
                end: 3_000,
                completed: 0,
                state: SegmentState::Retrying,
            },
        ];
        let html = en(&[t]);
        assert!(html.contains("Progress of each connection"), "{html}");
        for width in ["75%", "100%", "0%"] {
            assert!(
                html.contains(&format!("class=\"segfill\" style=\"width: {width}\"")),
                "segment fill {width} in {html}"
            );
        }
        // a single segment is just the main bar again -- no redundant list
        let mut one = transfer(2, TransferState::Active);
        one.segments = vec![SegmentInfo {
            start: 0,
            end: 10,
            completed: 5,
            state: SegmentState::Active,
        }];
        assert!(!en(&[one]).contains("Progress of each connection"));
    }

    #[test]
    fn a_zero_length_segment_does_not_divide_by_zero() {
        let mut t = transfer(1, TransferState::Active);
        t.segments = vec![
            SegmentInfo {
                start: 5,
                end: 5,
                completed: 0,
                state: SegmentState::Pending,
            },
            SegmentInfo {
                start: 5,
                end: 10,
                completed: 1,
                state: SegmentState::Active,
            },
        ];
        assert!(en(&[t]).contains("width: 0%"));
    }

    #[test]
    fn a_completed_transfer_is_a_full_bar_and_says_where_the_file_is() {
        let mut t = transfer(1, TransferState::Completed);
        t.completed_bytes = 3 * MIB;
        let html = en(&[t]);
        assert!(
            html.contains("Completed")
                && html.contains("100%")
                && html.contains("style=\"width: 100%\""),
            "{html}"
        );
        assert!(
            html.contains("Saved to:") && html.contains("/home/u/Downloads/BlueIce/file1.iso"),
            "{html}"
        );
    }

    #[test]
    fn a_bar_never_claims_to_be_full_before_the_transfer_is_complete() {
        let mut t = transfer(1, TransferState::Active);
        t.completed_bytes = 3 * MIB - 1;
        let html = en(&[t]);
        assert!(html.contains("99%") && !html.contains("100%"), "{html}");
    }

    #[test]
    fn an_unknown_total_shows_bytes_and_an_empty_bar() {
        let mut t = transfer(1, TransferState::Active);
        t.total_bytes = None;
        let html = en(&[t]);
        assert!(html.contains("1.2 MiB (total size unknown)"), "{html}");
        assert!(html.contains("style=\"width: 0%\""), "{html}");
    }

    #[test]
    fn a_failed_transfer_shows_its_last_error() {
        let mut t = transfer(1, TransferState::Failed);
        t.last_error = Some("the server answered with HTTP status 404".to_string());
        let html = en(&[t]);
        assert!(
            html.contains("Failed")
                && html.contains("Last error:")
                && html.contains("HTTP status 404"),
            "{html}"
        );
    }

    #[test]
    fn a_blocked_transfer_says_the_gatekeeper_stopped_it_and_why() {
        let mut t = transfer(1, TransferState::Blocked);
        t.blocked = Some(BlockedInfo {
            reason: "executable from an untrusted origin".to_string(),
            category: "dangerous-file-type".to_string(),
        });
        let html = en(&[t]);
        assert!(
            html.contains("Blocked") && html.contains("Blocked by the safety gatekeeper:"),
            "{html}"
        );
        assert!(
            html.contains("dangerous-file-type")
                && html.contains("executable from an untrusted origin"),
            "{html}"
        );
    }

    #[test]
    fn retries_and_a_single_stream_mode_are_explained() {
        let mut t = transfer(1, TransferState::Active);
        t.retries = 2;
        t.mode = TransferMode::SingleStream {
            reason: SingleStreamReason::ServerIgnoresRange,
        };
        let html = en(&[t]);
        assert!(html.contains("Retries:"), "{html}");
        assert!(
            html.contains("the server does not support parallel downloads"),
            "{html}"
        );
        let mut u = transfer(2, TransferState::Active);
        u.mode = TransferMode::SingleStream {
            reason: SingleStreamReason::UnknownLength,
        };
        assert!(en(&[u]).contains("does not say how large the file is"));
    }

    #[test]
    fn a_paused_transfer_that_cannot_keep_its_progress_says_so() {
        let mut t = transfer(1, TransferState::Paused);
        t.resume_safe = false;
        assert!(
            en(&[t.clone()]).contains("Pausing will restart this download from the beginning.")
        );
        t.resume_safe = true;
        assert!(!en(&[t]).contains("Pausing will restart"));
    }

    #[test]
    fn a_transfer_that_has_moved_nothing_and_knows_no_size_shows_just_its_state() {
        for state in [
            TransferState::Queued,
            TransferState::AwaitingClearance,
            TransferState::Failed,
            TransferState::Blocked,
            TransferState::Cancelled,
        ] {
            let mut t = transfer(1, state);
            t.completed_bytes = 0;
            t.total_bytes = None;
            let html = en(&[t]);
            assert!(
                !html.contains("0 B") && !html.contains("total size unknown"),
                "{state}: {html}"
            );
        }
        // ...but once it has moved bytes, or knows its size, it says how far it got.
        let mut some = transfer(1, TransferState::Failed);
        some.total_bytes = None;
        assert!(en(&[some]).contains("total size unknown"));
        let mut sized = transfer(2, TransferState::Queued);
        sized.completed_bytes = 0;
        assert!(en(&[sized]).contains("0% (0 B of 3.0 MiB)"));
    }

    #[test]
    fn a_queued_transfer_has_no_speed_or_progress_figures_to_show() {
        let mut t = transfer(1, TransferState::Queued);
        t.completed_bytes = 0;
        t.total_bytes = None;
        let html = en(&[t]);
        assert!(html.contains("Queued"), "{html}");
        assert!(
            !html.contains("Speed:") && !html.contains("Time left:"),
            "{html}"
        );
    }

    #[test]
    fn every_state_has_its_own_label() {
        for (state, label) in [
            (TransferState::Queued, "Queued"),
            (
                TransferState::AwaitingClearance,
                "Waiting for safety review",
            ),
            (TransferState::Active, "Downloading"),
            (TransferState::Paused, "Paused"),
            (TransferState::Completed, "Completed"),
            (TransferState::Failed, "Failed"),
            (TransferState::Cancelled, "Cancelled"),
            (TransferState::Blocked, "Blocked"),
            (TransferState::Unknown, "Unknown"),
        ] {
            assert!(
                en(&[transfer(1, state)]).contains(label),
                "{state}: {label}"
            );
        }
    }

    #[test]
    fn the_count_is_singular_or_plural() {
        assert!(
            en(&[transfer(1, TransferState::Active)]).contains("1 download<"),
            "{}",
            en(&[transfer(1, TransferState::Active)])
        );
        assert!(
            en(&[
                transfer(1, TransferState::Active),
                transfer(2, TransferState::Paused),
                transfer(3, TransferState::Failed)
            ])
            .contains("3 downloads")
        );
    }

    #[test]
    fn running_transfers_come_first_then_the_rest_newest_first() {
        let html = en(&[
            transfer(1, TransferState::Completed),
            transfer(2, TransferState::Active),
            transfer(3, TransferState::Failed),
            transfer(4, TransferState::Paused),
            transfer(5, TransferState::Queued),
        ]);
        let at = |name: &str| html.find(name).unwrap_or_else(|| panic!("{name} missing"));
        // running (active, queued, waiting) newest first, then everything else newest first
        assert!(
            at("file5.iso") < at("file2.iso"),
            "queued 5 before active 2 (both running, newest first)"
        );
        assert!(at("file2.iso") < at("file4.iso"), "running before paused");
        assert!(
            at("file4.iso") < at("file3.iso") && at("file3.iso") < at("file1.iso"),
            "the rest newest first"
        );
    }

    #[test]
    fn text_a_remote_server_controls_cannot_inject_markup() {
        // The file name, the error, and the gatekeeper's reason all come from
        // outside, and this page is rendered as HTML by BlueIce's own engine.
        let mut t = transfer(1, TransferState::Blocked);
        t.dest_path = "/d/<script>alert(1)\"onmouseover=\"x.bin".to_string();
        t.last_error = Some("<b>bold</b> & \"quoted\" 'single'".to_string());
        t.blocked = Some(BlockedInfo {
            reason: "<img src=x onerror=y>".to_string(),
            category: "a&b".to_string(),
        });
        let html = en(&[t]);
        assert!(
            !html.contains("<script>") && !html.contains("<img") && !html.contains("<b>bold"),
            "{html}"
        );
        assert!(
            html.contains("&lt;script&gt;")
                && html
                    .contains("&lt;b&gt;bold&lt;/b&gt; &amp; &quot;quoted&quot; &#39;single&#39;"),
            "{html}"
        );
        assert!(html.contains("a&amp;b"));
    }

    #[test]
    fn a_sanitized_bidi_filename_stays_unambiguous_on_the_downloads_page() {
        // This is the same server-supplied Content-Disposition name exercised
        // over the real MCP/download-manager boundary. The page receives the
        // resulting TransferInfo and must show the ordinary, safe component.
        let hostile = "invoice\u{202e}fdp.exe";
        let safe = blueice_net::download::file_name::sanitize(hostile);
        let mut t = transfer(1, TransferState::Completed);
        t.dest_path = format!("/d/{safe}");
        let html = en(&[t]);
        assert_eq!(safe, "invoicefdp.exe");
        assert!(html.contains(&safe));
        assert!(!html.contains('\u{202e}'));
    }

    #[test]
    fn escape_html_handles_every_special_character_and_leaves_the_rest_alone() {
        assert_eq!(
            escape_html("a & b < c > d \" e ' f"),
            "a &amp; b &lt; c &gt; d &quot; e &#39; f"
        );
        assert_eq!(escape_html("plain text 日本語"), "plain text 日本語");
    }

    #[test]
    fn the_page_name_falls_back_to_the_url_when_there_is_no_destination_yet() {
        let mut t = transfer(1, TransferState::AwaitingClearance);
        t.dest_path.clear();
        t.url = "https://example.com/a/b/report.pdf?x=1".to_string();
        assert!(en(&[t.clone()]).contains("report.pdf"));
        t.url = "https://example.com/".to_string();
        assert!(
            en(&[t]).contains("https://example.com/"),
            "with nothing better, the whole URL"
        );
    }

    #[test]
    fn the_page_is_localized_through_i18n() {
        let html = downloads_html(
            &DownloadsView::Transfers(&[
                transfer(1, TransferState::Active),
                transfer(2, TransferState::Blocked),
            ]),
            "zh-TW",
        );
        assert!(html.contains("<title>下載</title>"), "{html}");
        assert!(html.contains("下載中") && html.contains("已封鎖"), "{html}");
        assert!(
            !html.contains("Downloading"),
            "no English state label leaks into a translated page: {html}"
        );
        assert!(downloads_html(&DownloadsView::Unavailable, "zh-TW").contains("下載服務尚未執行"));
        assert!(
            downloads_html(&DownloadsView::Transfers(&[]), "zh-TW").contains("目前沒有任何下載")
        );
    }

    #[test]
    fn an_unsupported_locale_falls_back_to_english() {
        assert!(
            downloads_html(&DownloadsView::Transfers(&[]), "klingon").contains("No downloads yet.")
        );
    }

    // ---- DownloadsSource: the live list, from a fake downloads process -------

    #[test]
    fn a_running_downloads_service_is_read_for_its_list() {
        let dir = Scratch::new("read");
        let _server = fake_downloads(
            &dir.socket(),
            vec![
                transfer(1, TransferState::Active),
                transfer(2, TransferState::Completed),
            ],
            false,
            DOWNLOADS_PROTOCOL_VERSION,
        );
        let source = DownloadsSource::without_spawner(dir.socket());
        let got = source.fetch_quick().unwrap();
        assert_eq!(got.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn a_missing_service_is_an_error_and_is_not_started_by_the_quick_read() {
        let dir = Scratch::new("missing");
        let spawned = Arc::new(AtomicUsize::new(0));
        let counter = spawned.clone();
        let source = DownloadsSource::with_spawner(
            dir.socket(),
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            Duration::from_millis(300),
        );
        assert!(source.fetch_quick().is_err());
        assert_eq!(
            spawned.load(Ordering::SeqCst),
            0,
            "merely rendering the page quickly must not start a process"
        );
    }

    #[test]
    fn opening_the_page_starts_the_service_when_it_is_not_running() {
        let dir = Scratch::new("spawn");
        let socket = dir.socket();
        let (spawned, started) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (counter, listener_started) = (spawned.clone(), started.clone());
        let spawn_socket = socket.clone();
        let spawner: Spawner = Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let (socket, listener_started) = (spawn_socket.clone(), listener_started.clone());
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                listener_started.fetch_add(1, Ordering::SeqCst);
                let _ = fake_downloads(
                    &socket,
                    vec![transfer(7, TransferState::Paused)],
                    false,
                    DOWNLOADS_PROTOCOL_VERSION,
                );
            });
            Ok(())
        });
        let source = DownloadsSource::with_spawner(socket, spawner, Duration::from_secs(5));
        let got = source.fetch_spawning().unwrap();
        assert_eq!(got[0].id, 7);
        assert_eq!(spawned.load(Ordering::SeqCst), 1);
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_spawn_that_fails_or_never_yields_a_socket_is_an_error_not_a_hang() {
        let dir = Scratch::new("nospawn");
        let failing = DownloadsSource::with_spawner(
            dir.socket(),
            Box::new(|| Err(std::io::Error::other("no such binary"))),
            Duration::from_secs(1),
        );
        assert!(
            failing
                .fetch_spawning()
                .unwrap_err()
                .contains("no such binary")
        );
        let silent = DownloadsSource::with_spawner(
            dir.socket(),
            Box::new(|| Ok(())),
            Duration::from_millis(300),
        );
        let started = Instant::now();
        assert!(silent.fetch_spawning().is_err());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_service_that_never_answers_is_given_up_on_at_the_timeout() {
        let dir = Scratch::new("stall");
        let _server = fake_downloads(&dir.socket(), Vec::new(), true, DOWNLOADS_PROTOCOL_VERSION);
        let source = DownloadsSource::with_spawner(
            dir.socket(),
            Box::new(|| Ok(())),
            Duration::from_millis(300),
        );
        let started = Instant::now();
        let error = source.fetch_quick().unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "must not stall the caller, took {:?}",
            started.elapsed()
        );
        assert!(error.contains("did not answer"), "{error}");
    }

    #[test]
    fn a_service_speaking_another_protocol_version_is_an_error() {
        let dir = Scratch::new("version");
        let _server = fake_downloads(
            &dir.socket(),
            Vec::new(),
            false,
            DOWNLOADS_PROTOCOL_VERSION + 1,
        );
        assert!(
            DownloadsSource::without_spawner(dir.socket())
                .fetch_quick()
                .is_err()
        );
    }

    #[test]
    fn the_production_source_points_at_the_well_known_socket() {
        let source = DownloadsSource::new();
        assert_eq!(
            source.socket(),
            blueice_ipc::downloads::default_downloads_socket_path()
        );
    }
}
