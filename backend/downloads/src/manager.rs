// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The downloads process's brain (`phase-10-download-manager/PLAN.md`'s
//! "Layering and process shape"): ids, the queue, each transfer's
//! lifecycle, the gatekeeper calls, the destination policy, persistence,
//! and fan-out of changes to subscribers. All transfer *mechanics*
//! (probing, segments, retries, resume validation) live in
//! `blueice_net::download`; this module decides *whether and where* a
//! transfer runs and keeps the one [`TransferInfo`] record every client
//! reads.
//!
//! Each running transfer gets a **job thread** that walks it through
//! `AwaitingClearance` -> `review_url` -> `probe` (retrying transient
//! failures) -> pick the destination -> `review_download` -> `Transfer::
//! begin` -> wait. The manager reviews every start *itself*: a client -- the
//! MCP adapter included -- is never trusted to have asked the gatekeeper,
//! because a compile-time token doesn't cross a process boundary
//! (`research/safe-browsing-enforcement.md`). A resume re-enters the same
//! sequence, since a verdict can change between a pause and a resume.
//!
//! Lock discipline: the state mutex is never held across network or
//! blocking engine calls (`Transfer::pause`/`cancel`/`wait`, the
//! gatekeeper, the probe) -- only across bookkeeping, the (small) store
//! write, and non-blocking pushes into each subscriber's own pending set,
//! so one slow client can never delay another.

use crate::policy::{resolve_requested, unique_path};
use crate::store::{Store, StoredTransfer};
use blueice_ipc::downloads::{BlockedInfo, ErrorCode, TransferEvent, TransferInfo, TransferState};
use blueice_net::download::backend::validate_url;
use blueice_net::download::clearance::{Blocked, Reviewer};
use blueice_net::download::credentials::{delete_ftps_password, delete_sftp_password, delete_sftp_private_key_passphrase, save_ftps_password, save_sftp_password, save_sftp_private_key_passphrase, FtpsCredentialRef, SftpCredentialRef, SftpPrivateKeyPassphraseRef};
use blueice_net::download::file_name::choose_file_name;
use blueice_net::download::probe::probe;
use blueice_net::download::sidecar::remove_partials;
use blueice_net::download::transfer::{DownloadSpec, Snapshot, Transfer};
use blueice_net::download::DownloadOptions;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How many events a transfer's log keeps (matches the engine's own bound).
const MAX_EVENTS: usize = 32;
/// How long an operation waits for a job thread to settle after asking it to.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// Where finished downloads land, and the only place a requested
    /// destination may point inside.
    pub download_dir: PathBuf,
    /// Where `transfers.json` lives.
    pub data_dir: PathBuf,
    pub gatekeeper_socket: PathBuf,
    /// Transfers running at once; the rest stay `Queued`.
    pub max_concurrent: usize,
    /// Per-transfer engine settings (connections, retries, timeouts, ...).
    pub options: DownloadOptions,
    /// How long a gatekeeper check may take before it counts as a rejection.
    pub review_timeout: Duration,
}

impl ManagerConfig {
    pub fn new(download_dir: impl AsRef<Path>, data_dir: impl AsRef<Path>, gatekeeper_socket: impl AsRef<Path>) -> Self {
        ManagerConfig {
            download_dir: download_dir.as_ref().to_path_buf(),
            data_dir: data_dir.as_ref().to_path_buf(),
            gatekeeper_socket: gatekeeper_socket.as_ref().to_path_buf(),
            max_concurrent: 3,
            options: DownloadOptions::default(),
            review_timeout: Duration::from_secs(10),
        }
    }
}

/// A request the manager refused, in the wire protocol's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerError {
    pub code: ErrorCode,
    pub message: String,
}

impl ManagerError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ManagerError { code, message: message.into() }
    }

    fn not_found(id: u64) -> Self {
        ManagerError::new(ErrorCode::NotFound, format!("there is no transfer {id}"))
    }

    fn invalid_state(message: impl Into<String>) -> Self {
        ManagerError::new(ErrorCode::InvalidState, message)
    }
}

impl fmt::Display for ManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ManagerError {}

#[derive(Default)]
struct Control {
    pause: AtomicBool,
    cancel: AtomicBool,
}

struct Job {
    control: Arc<Control>,
    transfer: Option<Arc<Transfer>>,
}

struct Entry {
    info: TransferInfo,
    overwrite: bool,
    /// The event log as it stood when this run began, plus what the manager
    /// itself recorded since; the engine's own events are appended to it.
    events_before_engine: Vec<TransferEvent>,
    /// `Some` while a job thread exists for this transfer.
    job: Option<Job>,
}

/// One subscriber's pending updates. Deliberately *not* a queue of every
/// change: it keeps only the **latest** record of each transfer, in the
/// order they last changed. A subscriber that falls behind therefore skips
/// intermediate progress but always ends up seeing the newest state
/// (including a final `Completed`), and its memory is bounded by the number
/// of transfers, never by how long it stalls.
#[derive(Default)]
struct Pending {
    order: VecDeque<u64>,
    latest: HashMap<u64, TransferInfo>,
}

struct Subscriber {
    pending: Mutex<Pending>,
    wake: Condvar,
    closed: AtomicBool,
}

impl Subscriber {
    fn push(&self, info: &TransferInfo) {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if pending.latest.insert(info.id, info.clone()).is_none() {
            pending.order.push_back(info.id);
        }
        drop(pending);
        self.wake.notify_one();
    }

    fn pop(pending: &mut Pending) -> Option<TransferInfo> {
        let id = pending.order.pop_front()?;
        pending.latest.remove(&id)
    }
}

/// A subscription to changes in any transfer, from [`TransferManager::subscribe`].
/// Dropping it unsubscribes.
pub struct Subscription {
    inner: Arc<Subscriber>,
}

impl Subscription {
    /// The next pending update, waiting up to `timeout` for one; `None` if none came.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<TransferInfo> {
        let mut pending = self.inner.pending.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(info) = Subscriber::pop(&mut pending) {
            return Some(info);
        }
        pending = self.inner.wake.wait_timeout(pending, timeout).unwrap_or_else(|p| p.into_inner()).0;
        Subscriber::pop(&mut pending)
    }

    /// Everything pending right now, without waiting.
    pub fn drain(&self) -> Vec<TransferInfo> {
        let mut pending = self.inner.pending.lock().unwrap_or_else(|p| p.into_inner());
        std::iter::from_fn(|| Subscriber::pop(&mut pending)).collect()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::SeqCst);
    }
}

struct State {
    next_id: u64,
    generation: u64,
    entries: BTreeMap<u64, Entry>,
    subscribers: Vec<Arc<Subscriber>>,
    last_saved: Instant,
    shutting_down: bool,
}

struct Shared {
    config: ManagerConfig,
    /// The canonical download directory (symlinks resolved).
    root: PathBuf,
    store: Store,
    state: Mutex<State>,
    changed: Condvar,
}

pub struct TransferManager {
    shared: Arc<Shared>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn push_event(events: &mut Vec<TransferEvent>, message: impl Into<String>) {
    events.push(TransferEvent { at_ms: now_ms(), message: message.into() });
    if events.len() > MAX_EVENTS {
        events.remove(0);
    }
}

/// A destination is *claimed* while its transfer may still write there.
fn claims_destination(state: TransferState) -> bool {
    matches!(state, TransferState::Queued | TransferState::AwaitingClearance | TransferState::Active | TransferState::Paused)
}

/// Why a job thread stopped, when the engine's own final snapshot isn't
/// what says so.
enum JobEnd {
    /// `Transfer::wait` returned: the engine's final snapshot already set the state.
    Engine,
    Blocked(Blocked),
    Failed(String),
    Paused,
    Cancelled,
}

impl TransferManager {
    /// Opens the manager over its directories: creates the download
    /// directory, loads `transfers.json`, and brings interrupted transfers
    /// back as `Paused` -- never auto-started, so a restart can't quietly
    /// resume downloads the user has forgotten about.
    pub fn open(config: ManagerConfig) -> io::Result<Arc<TransferManager>> {
        std::fs::create_dir_all(&config.download_dir)?;
        let root = std::fs::canonicalize(&config.download_dir)?;
        let store = Store::new(&config.data_dir);
        let loaded = store.load();

        let mut entries = BTreeMap::new();
        for stored in loaded.transfers {
            let mut info = stored.info;
            let mut events = info.events.clone();
            if matches!(info.state, TransferState::Queued | TransferState::AwaitingClearance | TransferState::Active) {
                info.state = TransferState::Paused;
                info.speed_bps = 0;
                info.eta_secs = None;
                info.connections = 0;
                push_event(&mut events, "the downloads process restarted; resume to continue");
                info.events = events.clone();
            }
            entries.insert(info.id, Entry { info, overwrite: stored.overwrite, events_before_engine: events, job: None });
        }
        let state = State { next_id: loaded.next_id, generation: 0, entries, subscribers: Vec::new(), last_saved: Instant::now(), shutting_down: false };
        let shared = Arc::new(Shared { config, root, store, state: Mutex::new(state), changed: Condvar::new() });
        shared.persist(&mut shared.lock());
        Ok(Arc::new(TransferManager { shared }))
    }

    /// Queues a download. `dest`, if given, is a path relative to the
    /// download directory and is validated now, so a bad request fails
    /// immediately rather than after a probe; without it, the file name
    /// comes from the server's response or the URL.
    pub fn start(&self, url: &str, dest: Option<&str>, overwrite: bool) -> Result<TransferInfo, ManagerError> {
        validate_url(url).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        let mut state = self.shared.lock();
        if state.shutting_down {
            return Err(ManagerError::new(ErrorCode::Internal, "the downloads process is shutting down"));
        }
        let dest_path = match dest {
            Some(requested) => {
                let path = resolve_requested(&self.shared.root, requested).map_err(|m| ManagerError::new(ErrorCode::InvalidRequest, m))?;
                if !overwrite && path.exists() {
                    return Err(ManagerError::new(ErrorCode::InvalidRequest, format!("{} already exists; ask to overwrite it to replace it", path.display())));
                }
                if let Some(other) = state.entries.values().find(|e| claims_destination(e.info.state) && Path::new(&e.info.dest_path) == path) {
                    return Err(ManagerError::new(ErrorCode::InvalidRequest, format!("{} is already the destination of transfer {}", path.display(), other.info.id)));
                }
                path.to_string_lossy().into_owned()
            }
            None => String::new(),
        };

        let id = state.next_id;
        state.next_id += 1;
        let info = TransferInfo { id, url: url.to_string(), dest_path, state: TransferState::Queued, created_at_ms: now_ms(), ..TransferInfo::default() };
        state.entries.insert(id, Entry { info, overwrite, events_before_engine: Vec::new(), job: None });
        let created = self.shared.commit(&mut state, id, true);
        Shared::schedule(&self.shared, &mut state);
        Ok(created)
    }

    /// Writes a password to the platform credential store. No transfer state
    /// carries the password; later SFTP workers derive this reference from
    /// their `sftp://user@host/path` URL after host-key verification.
    pub fn set_sftp_password(&self, host: &str, port: u16, username: &str, password: &str) -> Result<(), ManagerError> {
        let reference = SftpCredentialRef::new(host, port, username).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        save_sftp_password(&reference, password).map_err(|error| ManagerError::new(ErrorCode::Internal, error.to_string()))
    }

    pub fn remove_sftp_password(&self, host: &str, port: u16, username: &str) -> Result<(), ManagerError> {
        let reference = SftpCredentialRef::new(host, port, username).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        delete_sftp_password(&reference).map_err(|error| ManagerError::new(ErrorCode::Internal, error.to_string()))
    }

    /// Stores a passphrase for the local SFTP private key selected at process
    /// startup. The private-key path itself never crosses this socket or enters
    /// transfer state.
    pub fn set_sftp_private_key_passphrase(&self, host: &str, port: u16, username: &str, passphrase: &str) -> Result<(), ManagerError> {
        let reference = SftpPrivateKeyPassphraseRef::new(host, port, username).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        save_sftp_private_key_passphrase(&reference, passphrase).map_err(|error| ManagerError::new(ErrorCode::Internal, error.to_string()))
    }

    pub fn remove_sftp_private_key_passphrase(&self, host: &str, port: u16, username: &str) -> Result<(), ManagerError> {
        let reference = SftpPrivateKeyPassphraseRef::new(host, port, username).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        delete_sftp_private_key_passphrase(&reference).map_err(|error| ManagerError::new(ErrorCode::Internal, error.to_string()))
    }

    /// Writes an explicit-FTPS password to the platform credential store.
    /// Plain FTP is anonymous-only, so only the TLS-protected variant can
    /// create this kind of credential reference.
    pub fn set_ftps_password(&self, host: &str, port: u16, username: &str, password: &str) -> Result<(), ManagerError> {
        let reference = FtpsCredentialRef::new(host, port, username).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        save_ftps_password(&reference, password).map_err(|error| ManagerError::new(ErrorCode::Internal, error.to_string()))
    }

    pub fn remove_ftps_password(&self, host: &str, port: u16, username: &str) -> Result<(), ManagerError> {
        let reference = FtpsCredentialRef::new(host, port, username).map_err(|error| ManagerError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        delete_ftps_password(&reference).map_err(|error| ManagerError::new(ErrorCode::Internal, error.to_string()))
    }

    pub fn list(&self, filter: Option<TransferState>) -> Vec<TransferInfo> {
        self.shared.lock().entries.values().map(|e| e.info.clone()).filter(|i| filter.is_none_or(|f| i.state == f)).collect()
    }

    pub fn get(&self, id: u64) -> Result<TransferInfo, ManagerError> {
        self.shared.lock().entries.get(&id).map(|e| e.info.clone()).ok_or_else(|| ManagerError::not_found(id))
    }

    /// Pauses a queued, clearing, or running transfer and returns once it
    /// has settled. Pausing a paused transfer is a no-op.
    pub fn pause(&self, id: u64) -> Result<TransferInfo, ManagerError> {
        let transfer = {
            let mut state = self.shared.lock();
            let entry = state.entries.get_mut(&id).ok_or_else(|| ManagerError::not_found(id))?;
            match entry.info.state {
                TransferState::Paused => return Ok(entry.info.clone()),
                TransferState::Queued if entry.job.is_none() => {
                    entry.info.state = TransferState::Paused;
                    push_event(&mut entry.events_before_engine, "paused while waiting in the queue");
                    entry.info.events = entry.events_before_engine.clone();
                    return Ok(self.shared.commit(&mut state, id, true));
                }
                TransferState::Queued | TransferState::AwaitingClearance | TransferState::Active => {
                    let job = entry.job.as_ref().ok_or_else(|| ManagerError::invalid_state(format!("transfer {id} has no running job to pause")))?;
                    job.control.pause.store(true, Ordering::SeqCst);
                    job.transfer.clone()
                }
                other => return Err(ManagerError::invalid_state(format!("transfer {id} is {other} and cannot be paused"))),
            }
        };
        if let Some(transfer) = transfer {
            transfer.pause();
        }
        self.shared.wait_for_job_end(id);
        self.get(id)
    }

    /// Puts a paused, failed, or blocked transfer back in the queue. It goes
    /// through the whole review-probe-begin sequence again: a gatekeeper
    /// verdict can change between a pause and a resume.
    pub fn resume(&self, id: u64) -> Result<TransferInfo, ManagerError> {
        {
            let state = self.shared.lock();
            let entry = state.entries.get(&id).ok_or_else(|| ManagerError::not_found(id))?;
            if !matches!(entry.info.state, TransferState::Paused | TransferState::Failed | TransferState::Blocked) {
                return Err(ManagerError::invalid_state(format!("transfer {id} is {} and cannot be resumed", entry.info.state)));
            }
        }
        self.shared.wait_for_job_end(id);
        let mut state = self.shared.lock();
        let entry = state.entries.get_mut(&id).ok_or_else(|| ManagerError::not_found(id))?;
        if entry.job.is_some() {
            return Err(ManagerError::invalid_state(format!("transfer {id} is still settling; try again")));
        }
        entry.info.state = TransferState::Queued;
        entry.info.last_error = None;
        entry.info.blocked = None;
        entry.info.finished_at_ms = None;
        push_event(&mut entry.events_before_engine, "resuming: the download will be reviewed by the gatekeeper again");
        entry.info.events = entry.events_before_engine.clone();
        let queued = self.shared.commit(&mut state, id, true);
        Shared::schedule(&self.shared, &mut state);
        Ok(queued)
    }

    /// Stops a transfer and deletes its partial files. A completed transfer
    /// is left exactly as it is (its file is kept).
    pub fn cancel(&self, id: u64) -> Result<TransferInfo, ManagerError> {
        let (transfer, running) = {
            let mut state = self.shared.lock();
            let entry = state.entries.get_mut(&id).ok_or_else(|| ManagerError::not_found(id))?;
            if matches!(entry.info.state, TransferState::Completed | TransferState::Cancelled) {
                return Ok(entry.info.clone());
            }
            match entry.job.as_ref() {
                Some(job) => {
                    job.control.cancel.store(true, Ordering::SeqCst);
                    (job.transfer.clone(), true)
                }
                None => (None, false),
            }
        };
        if running {
            if let Some(transfer) = transfer {
                transfer.cancel();
            }
            self.shared.wait_for_job_end(id);
        }
        // Whether it was running or already at rest, make sure it ends up
        // cancelled with its partial files gone.
        let dest = {
            let mut state = self.shared.lock();
            let entry = state.entries.get_mut(&id).ok_or_else(|| ManagerError::not_found(id))?;
            if entry.info.state == TransferState::Completed {
                return Ok(entry.info.clone());
            }
            entry.info.state = TransferState::Cancelled;
            entry.info.finished_at_ms = Some(now_ms());
            entry.info.speed_bps = 0;
            entry.info.eta_secs = None;
            entry.info.connections = 0;
            push_event(&mut entry.events_before_engine, "cancelled");
            entry.info.events = entry.events_before_engine.clone();
            let dest = entry.info.dest_path.clone();
            self.shared.commit(&mut state, id, true);
            dest
        };
        if !dest.is_empty() {
            remove_partials(Path::new(&dest));
        }
        self.get(id)
    }

    /// Drops a finished transfer from history. Never a running or paused
    /// one -- cancel it first. A failed transfer's leftover partial files go
    /// with it; a completed transfer's file is kept.
    pub fn remove(&self, id: u64) -> Result<(), ManagerError> {
        // A transfer whose state is already terminal may still be inside its
        // job thread's last moments (the engine reports the final state
        // before the job thread has cleaned up), so let it settle first:
        // otherwise a client that sees "completed" and removes it at once
        // could be told it is still running.
        if self.shared.lock().entries.get(&id).is_some_and(|e| e.info.state.is_terminal()) {
            self.shared.wait_for_job_end(id);
        }
        let mut state = self.shared.lock();
        let entry = state.entries.get(&id).ok_or_else(|| ManagerError::not_found(id))?;
        if !entry.info.state.is_terminal() || entry.job.is_some() {
            return Err(ManagerError::invalid_state(format!("transfer {id} is {}; cancel it before removing it", entry.info.state)));
        }
        let (state_was, dest) = (entry.info.state, entry.info.dest_path.clone());
        state.entries.remove(&id);
        self.shared.persist(&mut state);
        drop(state);
        if state_was == TransferState::Failed && !dest.is_empty() {
            remove_partials(Path::new(&dest));
        }
        Ok(())
    }

    /// Updates to any transfer from now on. A subscriber that doesn't keep
    /// up skips intermediate progress but always sees the newest state of
    /// each transfer (see [`Pending`]); it never blocks anyone.
    pub fn subscribe(&self) -> Subscription {
        let inner = Arc::new(Subscriber { pending: Mutex::new(Pending::default()), wake: Condvar::new(), closed: AtomicBool::new(false) });
        self.shared.lock().subscribers.push(inner.clone());
        Subscription { inner }
    }

    /// Nothing queued, clearing, or running: the state in which the
    /// launcher may tear this process down (`research/multi-process-memory.md`).
    pub fn is_idle(&self) -> bool {
        !self.shared.lock().entries.values().any(|e| matches!(e.info.state, TransferState::Queued | TransferState::AwaitingClearance | TransferState::Active))
    }

    /// An orderly stop: pauses everything running (checkpointing it, so it
    /// can be resumed), puts queued transfers back to paused, persists,
    /// and refuses new starts.
    pub fn shutdown(&self) {
        let jobs: Vec<(u64, Arc<Control>, Option<Arc<Transfer>>)> = {
            let mut state = self.shared.lock();
            state.shutting_down = true;
            state.entries.iter().filter_map(|(id, e)| e.job.as_ref().map(|j| (*id, j.control.clone(), j.transfer.clone()))).collect()
        };
        for (_, control, transfer) in &jobs {
            control.pause.store(true, Ordering::SeqCst);
            if let Some(transfer) = transfer {
                transfer.pause();
            }
        }
        for (id, _, _) in &jobs {
            self.shared.wait_for_job_end(*id);
        }
        let mut state = self.shared.lock();
        let ids: Vec<u64> = state.entries.iter().filter(|(_, e)| e.job.is_none() && e.info.state == TransferState::Queued).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(entry) = state.entries.get_mut(&id) {
                entry.info.state = TransferState::Paused;
                push_event(&mut entry.events_before_engine, "paused: the downloads process shut down");
                entry.info.events = entry.events_before_engine.clone();
            }
            self.shared.commit(&mut state, id, false);
        }
        self.shared.persist(&mut state);
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Records that `id` changed: bumps the generation, tells subscribers,
    /// and saves (immediately for a state change, at most once a second for
    /// progress ticks). Returns the record as it now stands.
    fn commit(&self, state: &mut State, id: u64, force_save: bool) -> TransferInfo {
        state.generation += 1;
        let generation = state.generation;
        let info = match state.entries.get_mut(&id) {
            Some(entry) => {
                entry.info.generation = generation;
                entry.info.clone()
            }
            None => return TransferInfo::default(),
        };
        state.subscribers.retain(|s| !s.closed.load(Ordering::SeqCst));
        for subscriber in &state.subscribers {
            subscriber.push(&info);
        }
        if force_save || state.last_saved.elapsed() >= Duration::from_secs(1) {
            self.persist(state);
        }
        self.changed.notify_all();
        info
    }

    fn persist(&self, state: &mut State) {
        let transfers: Vec<StoredTransfer> = state.entries.values().map(|e| StoredTransfer { info: e.info.clone(), overwrite: e.overwrite }).collect();
        // Best effort: failing to save must not take a download down with it.
        let _ = self.store.save(state.next_id, &transfers);
        state.last_saved = Instant::now();
    }

    /// Blocks until `id` has no job thread (it has settled), or a timeout.
    fn wait_for_job_end(&self, id: u64) {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        let mut state = self.lock();
        while state.entries.get(&id).is_some_and(|e| e.job.is_some()) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            state = self.changed.wait_timeout(state, deadline - now).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    /// Starts jobs for queued transfers while slots are free.
    fn schedule(shared: &Arc<Shared>, state: &mut State) {
        if state.shutting_down {
            return;
        }
        let running = state.entries.values().filter(|e| e.job.is_some()).count();
        let mut free = shared.config.max_concurrent.max(1).saturating_sub(running);
        let queued: Vec<u64> = state.entries.iter().filter(|(_, e)| e.info.state == TransferState::Queued && e.job.is_none()).map(|(id, _)| *id).collect();
        for id in queued {
            if free == 0 {
                break;
            }
            free -= 1;
            let control = Arc::new(Control::default());
            if let Some(entry) = state.entries.get_mut(&id) {
                entry.job = Some(Job { control: control.clone(), transfer: None });
            }
            let for_job = shared.clone();
            thread::spawn(move || run_job(for_job, id, control));
        }
    }

    /// Folds an engine snapshot into the record clients read.
    fn apply_snapshot(&self, id: u64, snapshot: &Snapshot) {
        let mut state = self.lock();
        let Some(entry) = state.entries.get_mut(&id) else { return };
        let changed_state = entry.info.state != snapshot.state;
        let info = &mut entry.info;
        info.state = snapshot.state;
        info.final_url = Some(snapshot.final_url.clone());
        info.total_bytes = snapshot.total_bytes;
        info.completed_bytes = snapshot.completed_bytes;
        info.speed_bps = snapshot.speed_bps;
        info.eta_secs = snapshot.eta.map(|d| d.as_secs());
        info.connections = snapshot.connections;
        info.mode = snapshot.mode;
        info.resume_safe = snapshot.resume_safe;
        info.segments = snapshot.segments.clone();
        info.retries = snapshot.retries;
        info.last_error = snapshot.last_error.clone();
        info.content_type = snapshot.content_type.clone();
        let mut events = entry.events_before_engine.clone();
        events.extend(snapshot.events.iter().cloned());
        if events.len() > MAX_EVENTS {
            events.drain(..events.len() - MAX_EVENTS);
        }
        entry.info.events = events;
        if snapshot.state.is_terminal() {
            entry.info.finished_at_ms = Some(now_ms());
        }
        self.commit(&mut state, id, changed_state);
    }

    /// Adds a manager-level event to `id`'s log (and to the record).
    fn event(&self, id: u64, message: impl Into<String>) {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&id) {
            push_event(&mut entry.events_before_engine, message);
            entry.info.events = entry.events_before_engine.clone();
        }
        self.commit(&mut state, id, false);
    }
}

fn interrupted(control: &Control) -> Option<JobEnd> {
    if control.cancel.load(Ordering::SeqCst) {
        Some(JobEnd::Cancelled)
    } else if control.pause.load(Ordering::SeqCst) {
        Some(JobEnd::Paused)
    } else {
        None
    }
}

/// Sleeps `duration`, waking early if the job is paused or cancelled.
fn sleep_unless_interrupted(duration: Duration, control: &Control) -> Option<JobEnd> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if let Some(end) = interrupted(control) {
            return Some(end);
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

fn run_job(shared: Arc<Shared>, id: u64, control: Arc<Control>) {
    let end = job_steps(&shared, id, &control);
    let mut state = shared.lock();
    if let Some(entry) = state.entries.get_mut(&id) {
        // `Engine`: `Transfer::wait` returned, so the engine's final snapshot
        // has already set the state and merged its events. Every other end
        // happened before the engine started, and the manager says so itself.
        if !matches!(end, JobEnd::Engine) {
            match end {
                JobEnd::Blocked(blocked) => {
                    entry.info.state = TransferState::Blocked;
                    push_event(&mut entry.events_before_engine, format!("blocked by the gatekeeper ({}): {}", blocked.category, blocked.reason));
                    entry.info.blocked = Some(BlockedInfo { reason: blocked.reason, category: blocked.category });
                    entry.info.finished_at_ms = Some(now_ms());
                }
                JobEnd::Failed(message) => {
                    entry.info.state = TransferState::Failed;
                    push_event(&mut entry.events_before_engine, format!("failed: {message}"));
                    entry.info.last_error = Some(message);
                    entry.info.finished_at_ms = Some(now_ms());
                }
                JobEnd::Paused => {
                    entry.info.state = TransferState::Paused;
                    push_event(&mut entry.events_before_engine, "paused before the transfer started; resuming starts again from the review");
                }
                JobEnd::Cancelled => {
                    entry.info.state = TransferState::Cancelled;
                    push_event(&mut entry.events_before_engine, "cancelled");
                    entry.info.finished_at_ms = Some(now_ms());
                }
                JobEnd::Engine => {}
            }
            entry.info.events = entry.events_before_engine.clone();
            entry.info.speed_bps = 0;
            entry.info.eta_secs = None;
            entry.info.connections = 0;
        }
        entry.job = None;
    }
    shared.commit(&mut state, id, true);
    Shared::schedule(&shared, &mut state);
}

fn job_steps(shared: &Arc<Shared>, id: u64, control: &Arc<Control>) -> JobEnd {
    let config = &shared.config;
    let (url, existing_dest, overwrite) = {
        let mut state = shared.lock();
        let Some(entry) = state.entries.get_mut(&id) else { return JobEnd::Failed("the transfer was removed".to_string()) };
        entry.events_before_engine = entry.info.events.clone();
        entry.info.state = TransferState::AwaitingClearance;
        entry.info.last_error = None;
        entry.info.blocked = None;
        push_event(&mut entry.events_before_engine, "reviewing the URL with the gatekeeper");
        entry.info.events = entry.events_before_engine.clone();
        let fields = (entry.info.url.clone(), entry.info.dest_path.clone(), entry.overwrite);
        shared.commit(&mut state, id, true);
        fields
    };

    let reviewer = Reviewer::new(&config.gatekeeper_socket).with_timeout(config.review_timeout);
    let cleared = match reviewer.review_url(&url) {
        Ok(cleared) => cleared,
        Err(blocked) => return JobEnd::Blocked(blocked),
    };
    if let Some(end) = interrupted(control) {
        return end;
    }
    shared.event(id, "the URL is cleared; probing the server");

    let mut attempt = 0;
    let probed = loop {
        match probe(&cleared, &config.options) {
            Ok(probed) => break probed,
            Err(e) if e.is_retryable() && attempt < config.options.max_retries => {
                attempt += 1;
                let delay = config.options.retry_delay(attempt);
                shared.event(id, format!("the probe failed ({e}); retrying in {} ms (attempt {attempt}/{})", delay.as_millis(), config.options.max_retries));
                if let Some(end) = sleep_unless_interrupted(delay, control) {
                    return end;
                }
            }
            Err(e) => return JobEnd::Failed(e.to_string()),
        }
    };
    if let Some(end) = interrupted(control) {
        return end;
    }

    // The destination: a requested one was fixed at `start`; otherwise the
    // name comes from the response, made unique, and claimed under the lock
    // so two transfers racing for one name get different ones.
    let derived = existing_dest.is_empty();
    let dest = if derived {
        let name = choose_file_name(probed.content_disposition.as_deref(), &probed.url);
        let mut state = shared.lock();
        let claimed: HashSet<PathBuf> = state.entries.values().filter(|e| e.info.id != id && claims_destination(e.info.state)).map(|e| PathBuf::from(&e.info.dest_path)).collect();
        let path = unique_path(&shared.root, &name, &|p| claimed.contains(p));
        if let Some(entry) = state.entries.get_mut(&id) {
            entry.info.dest_path = path.to_string_lossy().into_owned();
        }
        shared.commit(&mut state, id, true);
        path
    } else {
        PathBuf::from(existing_dest)
    };
    let file_name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    let clearance = match reviewer.review_download(cleared, &probed, &file_name) {
        Ok(clearance) => clearance,
        Err(blocked) => {
            if derived {
                // Nothing was created; release the name this transfer had claimed.
                let mut state = shared.lock();
                if let Some(entry) = state.entries.get_mut(&id) {
                    entry.info.dest_path.clear();
                }
            }
            return JobEnd::Blocked(blocked);
        }
    };
    shared.event(id, "the gatekeeper cleared the download");
    if let Some(end) = interrupted(control) {
        return end;
    }

    let options = DownloadOptions { overwrite, ..config.options.clone() };
    let for_updates = shared.clone();
    let on_update = Arc::new(move |snapshot: &Snapshot| for_updates.apply_snapshot(id, snapshot));
    let transfer = match Transfer::begin(DownloadSpec { dest, options, on_update: Some(on_update) }, probed, clearance) {
        Ok(transfer) => Arc::new(transfer),
        Err(e) => return JobEnd::Failed(e.to_string()),
    };
    if let Some(entry) = shared.lock().entries.get_mut(&id) {
        if let Some(job) = entry.job.as_mut() {
            job.transfer = Some(transfer.clone());
        }
    }
    // A pause or cancel may have arrived while the transfer was starting.
    if control.cancel.load(Ordering::SeqCst) {
        transfer.cancel();
    } else if control.pause.load(Ordering::SeqCst) {
        transfer.pause();
    }
    transfer.wait();
    JobEnd::Engine
}
