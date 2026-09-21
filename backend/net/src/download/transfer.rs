// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One running download (`phase-10-download-manager/PLAN.md`'s
//! "Segmentation", "Coordinator", "Retries", and "Files, sidecar, and
//! resume").
//!
//! A [`Transfer`] owns a **coordinator** thread and up to
//! `max_connections` **worker** threads. All shared state lives in one
//! mutex-guarded [`Inner`]:
//!
//! * A **segment** is a byte range `[start, end)` with a write cursor
//!   `pos`, an `owner` token, and retry state. A worker claims a pending
//!   segment; when none is pending it *splits* the running segment with
//!   the most bytes left (the new segment takes the back half and the
//!   victim's `end` shrinks) -- dynamic re-splitting, which keeps every
//!   connection busy on a server whose connections run at different speeds.
//! * A worker commits progress only while it still holds the segment's
//!   `owner` token and the transfer is running. Revoking an owner (a
//!   stall, a pause, a cancel) therefore makes that worker's late bytes
//!   harmless: it may finish one buffer's write, but its progress is never
//!   counted and it exits at its next commit. This is what makes a worker
//!   blocked in a dead `read()` safe to abandon.
//! * The coordinator ticks: it samples speed, publishes a snapshot,
//!   revokes stalled segments, tops up workers, and checkpoints
//!   (`sync_data`, then an atomic sidecar write) so the sidecar never
//!   claims bytes that aren't durable.
//!
//! **One resume path.** [`Transfer::pause`] stops the workers and
//! checkpoints; resuming -- in the same session or after a restart -- is
//! [`Transfer::begin`] rebuilding from the sidecar, exactly as after a
//! crash. A transfer resumes only if [`Sidecar::check`] passes, and
//! otherwise restarts from byte 0 and says why in its event log.

use crate::download::clearance::DownloadClearance;
use crate::download::plan::{initial_split, split_point};
use crate::download::backend::{self, ByteRange, TransferBackend};
use crate::download::probe::Probe;
use crate::download::progress::Progress;
use crate::download::sidecar::{part_path, remove_partials, sidecar_path, RestartReason, Sidecar, SidecarSegment};
use crate::download::{DownloadError, DownloadOptions};
use blueice_ipc::downloads::{SegmentInfo, SegmentState, SingleStreamReason, TransferEvent, TransferMode, TransferState};
use std::collections::VecDeque;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How many events a transfer keeps: enough to explain a slow or failed
/// transfer, bounded so a flapping server can't grow it forever.
const MAX_EVENTS: usize = 32;

pub type UpdateCallback = Arc<dyn Fn(&Snapshot) + Send + Sync>;

/// What to download where, and how.
pub struct DownloadSpec {
    /// The finished file. Progress lives beside it in `<dest>.blueice-part`
    /// and `<dest>.blueice-part.json` until the transfer completes.
    pub dest: PathBuf,
    pub options: DownloadOptions,
    /// Called from the coordinator thread (never with a lock held) with a
    /// fresh snapshot whenever something changed, and once more with the
    /// final one.
    pub on_update: Option<UpdateCallback>,
}

/// A point-in-time view of a transfer, in the vocabulary of the wire
/// protocol (`blueice_ipc::downloads`) so the downloads process can map it
/// onto a `TransferInfo` without translating.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// `Active`, `Paused`, `Completed`, `Failed`, or `Cancelled`.
    pub state: TransferState,
    pub total_bytes: Option<u64>,
    pub completed_bytes: u64,
    pub speed_bps: u64,
    pub eta: Option<Duration>,
    /// Segments with a connection working on them right now.
    pub connections: u32,
    pub mode: TransferMode,
    pub resume_safe: bool,
    pub segments: Vec<SegmentInfo>,
    pub retries: u32,
    pub last_error: Option<String>,
    pub events: Vec<TransferEvent>,
    pub final_url: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Running,
    Pausing,
    Cancelling,
    Failing(String),
    Finalizing,
    Finished(TransferState),
}

struct Segment {
    start: u64,
    /// `u64::MAX` for a single stream of unknown length, until it ends.
    end: u64,
    pos: u64,
    owner: Option<u64>,
    /// Consecutive failures without progress.
    attempts: u32,
    not_before: Instant,
    last_progress: Instant,
}

struct Inner {
    phase: Phase,
    segments: Vec<Segment>,
    next_owner: u64,
    workers: usize,
    retries: u32,
    last_error: Option<String>,
    events: VecDeque<TransferEvent>,
    progress: Progress,
    dirty: bool,
    checkpoint_failed: bool,
}

struct Shared {
    dest: PathBuf,
    options: DownloadOptions,
    probe: Probe,
    segmented: bool,
    resume_safe: bool,
    mode: TransferMode,
    backend: Arc<dyn TransferBackend>,
    file: File,
    inner: Mutex<Inner>,
    changed: Condvar,
    on_update: Option<UpdateCallback>,
}

struct Claim {
    index: usize,
    owner: u64,
    pos: u64,
    end: u64,
}

enum Outcome {
    Done,
    Retry(DownloadError),
    Fatal(DownloadError),
    Revoked,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn push_event(inner: &mut Inner, message: String) {
    if inner.events.len() >= MAX_EVENTS {
        inner.events.pop_front();
    }
    inner.events.push_back(TransferEvent { at_ms: now_ms(), message });
}

fn completed_bytes(inner: &Inner) -> u64 {
    inner.segments.iter().map(|s| s.pos.min(s.end).saturating_sub(s.start)).sum()
}

/// A running (or paused, failed, finished) download. Dropping one that is
/// still running pauses it, so its progress is saved and it can be resumed
/// by a later [`Transfer::begin`].
pub struct Transfer {
    shared: Arc<Shared>,
    coordinator: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for Transfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transfer").field("dest", &self.shared.dest).field("state", &self.snapshot().state).finish()
    }
}

enum Saved {
    Nothing,
    Usable(Vec<SidecarSegment>),
    Unusable(RestartReason),
}

fn inspect_saved(dest: &std::path::Path, probe: &Probe) -> Saved {
    let Some(sidecar) = Sidecar::load(dest) else { return Saved::Nothing };
    let data_len = std::fs::metadata(part_path(dest)).map(|m| m.len()).unwrap_or(0);
    match sidecar.check(probe, data_len) {
        Ok(()) => Saved::Usable(sidecar.segments),
        Err(reason) => Saved::Unusable(reason),
    }
}

fn fresh_segment(start: u64, end: u64, pos: u64, now: Instant) -> Segment {
    Segment { start, end, pos, owner: None, attempts: 0, not_before: now, last_progress: now }
}

impl Transfer {
    /// Starts (or resumes) a download.
    ///
    /// Requires a [`DownloadClearance`], and checks that it is for exactly
    /// this download -- the same URL, file name, content type, and size --
    /// so a token for one file can't be spent on another. If a saved
    /// sidecar for `dest` passes [`Sidecar::check`] against `probe`, the
    /// transfer resumes from it; otherwise (and always for a server with
    /// nothing to validate against) it starts from byte 0, recording why.
    pub fn begin(spec: DownloadSpec, probe: Probe, clearance: DownloadClearance) -> Result<Transfer, DownloadError> {
        let DownloadSpec { dest, options, on_update } = spec;

        let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if clearance.url() != probe.url {
            return Err(DownloadError::ClearanceMismatch(format!("cleared for {}, asked to download {}", clearance.url(), probe.url)));
        }
        if clearance.file_name() != file_name {
            return Err(DownloadError::ClearanceMismatch(format!("cleared as {:?}, asked to write {file_name:?}", clearance.file_name())));
        }
        if clearance.total_bytes() != probe.total {
            return Err(DownloadError::ClearanceMismatch(format!("cleared at {:?} bytes, but the file is {:?} bytes", clearance.total_bytes(), probe.total)));
        }
        if clearance.content_type() != probe.content_type.as_deref() {
            return Err(DownloadError::ClearanceMismatch(format!("cleared as {:?}, but the file is {:?}", clearance.content_type(), probe.content_type)));
        }

        if !options.overwrite && dest.exists() {
            return Err(DownloadError::DestinationExists(dest));
        }
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }

        let now = Instant::now();
        let segmented = probe.can_segment();
        let resume_safe = probe.resume_safe();
        let mut events: Vec<String> = Vec::new();
        let mut segments: Vec<Segment> = Vec::new();

        let mode = if segmented { TransferMode::Segmented } else { TransferMode::SingleStream { reason: probe.single_stream_reason().unwrap_or(SingleStreamReason::Unknown) } };
        let file;
        let mut phase = Phase::Running;

        if probe.is_empty() {
            // Nothing to fetch: an empty destination is the whole download.
            File::create(&dest)?;
            remove_partials(&dest);
            file = File::open(&dest)?;
            phase = Phase::Finished(TransferState::Completed);
            events.push("completed: 0 bytes".to_string());
        } else if segmented {
            let part = part_path(&dest);
            match if resume_safe { inspect_saved(&dest, &probe) } else { Saved::Nothing } {
                Saved::Usable(saved) => {
                    file = OpenOptions::new().read(true).write(true).open(&part)?;
                    segments = saved.iter().map(|s| fresh_segment(s.start, s.end, s.pos, now)).collect();
                    let done: u64 = segments.iter().map(|s| s.pos - s.start).sum();
                    events.push(format!("resumed from {done} of {} bytes ({} segments)", probe.total.unwrap_or(0), segments.len()));
                }
                saved => {
                    if let Saved::Unusable(reason) = saved {
                        events.push(format!("restarting from the beginning: {reason}"));
                    }
                    remove_partials(&dest);
                    let total = probe.total.unwrap_or(0);
                    file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&part)?;
                    file.set_len(total)?;
                    segments = initial_split(total, options.max_connections, options.min_split_bytes).into_iter().map(|s| fresh_segment(s.start, s.end, s.start, now)).collect();
                    events.push(format!("server supports byte ranges: {} segments over up to {} connections", segments.len(), options.max_connections.max(1)));
                }
            }
        } else {
            remove_partials(&dest);
            file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(part_path(&dest))?;
            if let Some(total) = probe.total {
                file.set_len(total)?;
            }
            segments.push(fresh_segment(0, probe.total.unwrap_or(u64::MAX), 0, now));
            events.push(match probe.single_stream_reason() {
                Some(SingleStreamReason::UnknownLength) => "single stream: the server did not say how long the file is".to_string(),
                _ => "single stream: the server ignores Range requests".to_string(),
            });
        }
        if !resume_safe && !probe.is_empty() {
            events.push("the backend gave no revision marker that is safe across a restart: pausing will restart this download from the beginning".to_string());
        }

        let mut inner = Inner {
            phase,
            segments,
            next_owner: 1,
            workers: 0,
            retries: 0,
            last_error: None,
            events: VecDeque::new(),
            progress: Progress::new(),
            dirty: false,
            checkpoint_failed: false,
        };
        for message in events {
            push_event(&mut inner, message);
        }

        let shared = Arc::new(Shared {
            dest,
            backend: backend::for_url(&probe.final_url, &options)?,
            options,
            probe,
            segmented,
            resume_safe,
            mode,
            file,
            inner: Mutex::new(inner),
            changed: Condvar::new(),
            on_update,
        });

        if matches!(shared.lock().phase, Phase::Running) {
            if resume_safe {
                // A saved sidecar for this transfer exists from the first moment.
                let sidecar = shared.sidecar_of(&shared.lock());
                sidecar.save(&shared.dest)?;
            }
            let mut inner = shared.lock();
            Shared::top_up(&shared, &mut inner, now);
            drop(inner);
            let for_coordinator = shared.clone();
            let handle = thread::spawn(move || coordinate(for_coordinator));
            return Ok(Transfer { shared, coordinator: Mutex::new(Some(handle)) });
        }
        Ok(Transfer { shared, coordinator: Mutex::new(None) })
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.shared.lock();
        self.shared.build_snapshot(&inner)
    }

    pub fn state(&self) -> TransferState {
        self.snapshot().state
    }

    /// Stops the transfer, saves its progress, and returns once it has
    /// settled as `Paused`. Resuming is a new [`Transfer::begin`]. For a
    /// transfer that can't be resumed safely, the progress is discarded
    /// (and the snapshot says so). No effect on a transfer that already
    /// finished.
    pub fn pause(&self) {
        let mut inner = self.shared.lock();
        if inner.phase == Phase::Running {
            inner.phase = Phase::Pausing;
        }
        self.shared.changed.notify_all();
        while !matches!(inner.phase, Phase::Finished(_)) {
            inner = self.shared.wait(inner);
        }
    }

    /// Stops the transfer and deletes its partial files. A paused or
    /// failed transfer is cleaned up too; a completed one is left alone.
    pub fn cancel(&self) {
        loop {
            let mut inner = self.shared.lock();
            match inner.phase.clone() {
                Phase::Running => {
                    inner.phase = Phase::Cancelling;
                    self.shared.changed.notify_all();
                    while !matches!(inner.phase, Phase::Finished(_)) {
                        inner = self.shared.wait(inner);
                    }
                    return;
                }
                Phase::Finished(TransferState::Paused) | Phase::Finished(TransferState::Failed) => {
                    drop(inner);
                    remove_partials(&self.shared.dest);
                    let mut inner = self.shared.lock();
                    inner.phase = Phase::Finished(TransferState::Cancelled);
                    push_event(&mut inner, "cancelled".to_string());
                    self.shared.changed.notify_all();
                    return;
                }
                Phase::Finished(_) => return,
                // Mid-transition (pausing, failing, finalizing, ...): let it settle, then decide.
                _ => {
                    let _ = self.shared.changed.wait_timeout(inner, Duration::from_millis(20));
                }
            }
        }
    }

    /// Blocks until the transfer settles (completed, failed, cancelled, or
    /// paused) and returns its final snapshot.
    pub fn wait(&self) -> Snapshot {
        let mut inner = self.shared.lock();
        while !matches!(inner.phase, Phase::Finished(_)) {
            inner = self.shared.wait(inner);
        }
        self.shared.build_snapshot(&inner)
    }

    /// Like [`Self::wait`], giving up after `timeout`.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Snapshot> {
        let deadline = Instant::now() + timeout;
        let mut inner = self.shared.lock();
        loop {
            if matches!(inner.phase, Phase::Finished(_)) {
                return Some(self.shared.build_snapshot(&inner));
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            inner = self.shared.wait_for(inner, deadline - now);
        }
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        {
            let mut inner = self.shared.lock();
            if inner.phase == Phase::Running {
                inner.phase = Phase::Pausing;
            }
            self.shared.changed.notify_all();
        }
        let handle = self.coordinator.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wait<'a>(&'a self, guard: MutexGuard<'a, Inner>) -> MutexGuard<'a, Inner> {
        self.changed.wait(guard).unwrap_or_else(|p| p.into_inner())
    }

    fn wait_for<'a>(&'a self, guard: MutexGuard<'a, Inner>, timeout: Duration) -> MutexGuard<'a, Inner> {
        self.changed.wait_timeout(guard, timeout).unwrap_or_else(|p| p.into_inner()).0
    }

    fn build_snapshot(&self, inner: &Inner) -> Snapshot {
        let completed = completed_bytes(inner);
        let state = match inner.phase {
            Phase::Finished(state) => state,
            _ => TransferState::Active,
        };
        let active = state == TransferState::Active;
        let now = Instant::now();
        let _ = now;
        let segments = inner
            .segments
            .iter()
            .map(|s| {
                let end = if s.end == u64::MAX { s.pos.max(s.start) } else { s.end };
                let seg_state = if s.pos >= s.end {
                    SegmentState::Done
                } else if s.owner.is_some() {
                    SegmentState::Active
                } else if s.attempts > 0 {
                    SegmentState::Retrying
                } else {
                    SegmentState::Pending
                };
                SegmentInfo { start: s.start, end, completed: s.pos.min(end).saturating_sub(s.start), state: seg_state }
            })
            .collect();
        Snapshot {
            state,
            total_bytes: self.probe.total,
            completed_bytes: completed,
            speed_bps: if active { inner.progress.speed_bps() } else { 0 },
            eta: if active { self.probe.total.and_then(|t| inner.progress.eta(t.saturating_sub(completed))) } else { None },
            connections: inner.segments.iter().filter(|s| s.owner.is_some()).count() as u32,
            mode: self.mode,
            resume_safe: self.resume_safe,
            segments,
            retries: inner.retries,
            last_error: inner.last_error.clone(),
            events: inner.events.iter().cloned().collect(),
            final_url: self.probe.final_url.clone(),
            content_type: self.probe.content_type.clone(),
        }
    }

    fn sidecar_of(&self, inner: &Inner) -> Sidecar {
        Sidecar::from_probe(&self.probe, inner.segments.iter().map(|s| SidecarSegment { start: s.start, end: s.end, pos: s.pos.min(s.end) }).collect())
    }

    fn revoke_all(&self, inner: &mut Inner) {
        for segment in &mut inner.segments {
            segment.owner = None;
        }
    }

    fn has_claimable(&self, inner: &Inner, now: Instant) -> bool {
        let pending = inner.segments.iter().any(|s| s.owner.is_none() && s.pos < s.end && s.not_before <= now);
        pending || (self.segmented && inner.segments.iter().any(|s| s.owner.is_some() && split_point(s.pos, s.end, self.options.min_split_bytes).is_some()))
    }

    /// Spawns workers up to the connection limit while there is work a
    /// worker could claim.
    fn top_up(shared: &Arc<Shared>, inner: &mut Inner, now: Instant) {
        while inner.workers < shared.options.max_connections.max(1) && shared.has_claimable(inner, now) {
            inner.workers += 1;
            let for_worker = shared.clone();
            thread::spawn(move || worker(for_worker));
        }
    }

    /// Hands out work: a pending segment, else half of the running segment
    /// with the most bytes left.
    fn claim(&self) -> Option<Claim> {
        let mut inner = self.lock();
        if inner.phase != Phase::Running {
            return None;
        }
        let now = Instant::now();
        let owner = inner.next_owner;
        if let Some(index) = inner.segments.iter().position(|s| s.owner.is_none() && s.pos < s.end && s.not_before <= now) {
            inner.next_owner += 1;
            let segment = &mut inner.segments[index];
            segment.owner = Some(owner);
            segment.last_progress = now;
            return Some(Claim { index, owner, pos: segment.pos, end: segment.end });
        }
        if !self.segmented {
            return None;
        }
        let (victim, mid, _) = inner
            .segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.owner.is_some())
            .filter_map(|(i, s)| split_point(s.pos, s.end, self.options.min_split_bytes).map(|mid| (i, mid, s.end - s.pos)))
            .max_by_key(|&(_, _, remaining)| remaining)?;
        inner.next_owner += 1;
        let end = inner.segments[victim].end;
        inner.segments[victim].end = mid;
        let mut taken = fresh_segment(mid, end, mid, now);
        taken.owner = Some(owner);
        inner.segments.push(taken);
        push_event(&mut inner, format!("segment #{victim} split at byte {mid}: a new connection takes {mid}..{end}"));
        Some(Claim { index: inner.segments.len() - 1, owner, pos: mid, end })
    }

    /// Records a failed attempt on a segment: back off and retry, or --
    /// past the retry budget -- fail the whole transfer.
    fn retry_segment(&self, inner: &mut Inner, index: usize, error: DownloadError, now: Instant) {
        let max = self.options.max_retries;
        let segmented = self.segmented;
        let segment = &mut inner.segments[index];
        segment.owner = None;
        if !segmented {
            // A plain stream can't resume mid-way: start over.
            segment.pos = segment.start;
        }
        segment.attempts += 1;
        let attempts = segment.attempts;
        inner.retries += 1;
        if attempts > max {
            let message = format!("segment #{index} failed {attempts} times in a row; last error: {error}");
            inner.last_error = Some(message.clone());
            push_event(inner, message.clone());
            inner.phase = Phase::Failing(message);
        } else {
            let delay = self.options.retry_delay(attempts);
            inner.segments[index].not_before = now + delay;
            inner.last_error = Some(error.to_string());
            push_event(inner, format!("segment #{index} failed (attempt {attempts}/{max}), retrying in {} ms: {error}", delay.as_millis()));
        }
    }

    /// Revokes segments that have gone too long without a byte (`ureq` has
    /// no idle-read timeout, so this is the only stall detection there is).
    fn watchdog(&self, inner: &mut Inner, now: Instant) {
        let stall = self.options.stall_timeout;
        let stalled: Vec<(usize, Duration)> = inner
            .segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.owner.is_some() && now.saturating_duration_since(s.last_progress) > stall)
            .map(|(i, s)| (i, now.saturating_duration_since(s.last_progress)))
            .collect();
        for (index, idle) in stalled {
            if inner.phase != Phase::Running {
                break;
            }
            self.retry_segment(inner, index, DownloadError::Network(format!("the connection stalled: no data for {:.1} s", idle.as_secs_f64())), now);
        }
    }

    fn request(&self, claim: &Claim) -> Result<crate::download::backend::ByteStream, DownloadError> {
        let range = self.segmented.then(|| ByteRange::new(claim.pos, claim.end).expect("a claimed segment is non-empty"));
        self.backend.get(&self.probe, range)
    }

    /// Fetches one claimed segment, writing each buffer at its offset and
    /// committing progress only while the claim still stands.
    fn run_segment(&self, claim: &Claim) -> Outcome {
        let mut reader = match self.request(claim) {
            Ok(reader) => reader,
            Err(e) if e.is_retryable() => return Outcome::Retry(e),
            Err(e) => return Outcome::Fatal(e),
        };
        let mut buffer = vec![0u8; self.options.buffer_bytes.max(1)];
        let (mut pos, mut end) = (claim.pos, claim.end);
        loop {
            let n = match reader.read(&mut buffer) {
                Ok(n) => n,
                Err(e) => return Outcome::Retry(DownloadError::Network(e.to_string())),
            };
            if n == 0 {
                return self.at_end_of_stream(claim, pos, end);
            }
            // `end` may have shrunk since the last commit (another worker
            // split this segment); at most one buffer is written twice,
            // with identical bytes, and only the bytes below `end` count.
            let take = (end - pos).min(n as u64) as usize;
            if take > 0 {
                if let Err(e) = self.file.write_all_at(&buffer[..take], pos) {
                    return Outcome::Fatal(DownloadError::Io(e.to_string()));
                }
            }
            pos += take as u64;

            let mut inner = self.lock();
            let running = inner.phase == Phase::Running;
            let segment = &mut inner.segments[claim.index];
            if !running || segment.owner != Some(claim.owner) {
                return Outcome::Revoked;
            }
            segment.pos = pos;
            segment.last_progress = Instant::now();
            segment.attempts = 0;
            end = segment.end;
            let finished = pos >= end;
            if finished {
                segment.owner = None;
            }
            inner.dirty = true;
            if finished {
                return Outcome::Done;
            }
        }
    }

    fn at_end_of_stream(&self, claim: &Claim, pos: u64, end: u64) -> Outcome {
        if self.segmented || self.probe.total.is_some() {
            return Outcome::Retry(DownloadError::Truncated { got: pos - claim.pos, expected: end - claim.pos });
        }
        // A stream of unknown length simply ends here.
        let mut inner = self.lock();
        let running = inner.phase == Phase::Running;
        let segment = &mut inner.segments[claim.index];
        if !running || segment.owner != Some(claim.owner) {
            return Outcome::Revoked;
        }
        segment.end = pos;
        segment.owner = None;
        inner.dirty = true;
        Outcome::Done
    }

    fn finish_segment(&self, claim: &Claim, outcome: Outcome) {
        let mut inner = self.lock();
        let running = inner.phase == Phase::Running;
        let own = inner.segments[claim.index].owner == Some(claim.owner);
        match outcome {
            Outcome::Retry(error) if running && own => self.retry_segment(&mut inner, claim.index, error, Instant::now()),
            Outcome::Fatal(error) if running && own => {
                inner.segments[claim.index].owner = None;
                inner.last_error = Some(error.to_string());
                push_event(&mut inner, format!("segment #{} failed: {error}", claim.index));
                inner.phase = Phase::Failing(error.to_string());
            }
            _ => {}
        }
        drop(inner);
        self.changed.notify_all();
    }

    /// Makes the data durable and writes the sidecar; a failure is noted
    /// once in the event log, not repeated every tick.
    fn checkpoint(&self, sidecar: &Sidecar) {
        let result = self.file.sync_data().and_then(|()| sidecar.save(&self.dest));
        let mut inner = self.lock();
        match result {
            Ok(()) => inner.checkpoint_failed = false,
            Err(e) if !inner.checkpoint_failed => {
                inner.checkpoint_failed = true;
                push_event(&mut inner, format!("could not save progress: {e}"));
            }
            Err(_) => {}
        }
    }

    fn publish(&self, snapshot: &Snapshot) {
        if let Some(callback) = &self.on_update {
            callback(snapshot);
        }
    }

    /// Ends the transfer in `state`: the update callback sees the final
    /// snapshot first, so a caller woken by [`Transfer::wait`] can rely on
    /// having been told.
    fn conclude(&self, state: TransferState, event: String) {
        let snapshot = {
            let mut inner = self.lock();
            push_event(&mut inner, event);
            let mut snapshot = self.build_snapshot(&inner);
            snapshot.state = state;
            snapshot.speed_bps = 0;
            snapshot.eta = None;
            snapshot
        };
        self.publish(&snapshot);
        self.lock().phase = Phase::Finished(state);
        self.changed.notify_all();
    }

    fn settle_pause(&self) {
        let sidecar = {
            let mut inner = self.lock();
            self.revoke_all(&mut inner);
            self.resume_safe.then(|| self.sidecar_of(&inner))
        };
        match sidecar {
            Some(sidecar) => {
                self.checkpoint(&sidecar);
                let done = completed_bytes(&self.lock());
                self.conclude(TransferState::Paused, format!("paused at {done} of {} bytes", self.probe.total.unwrap_or(0)));
            }
            None => {
                remove_partials(&self.dest);
                {
                    let mut inner = self.lock();
                    for segment in &mut inner.segments {
                        segment.pos = segment.start;
                        segment.attempts = 0;
                    }
                }
                self.conclude(TransferState::Paused, "paused: the server gave nothing to resume from, so the progress is discarded and a resume starts from the beginning".to_string());
            }
        }
    }

    fn settle_cancel(&self) {
        self.revoke_all(&mut self.lock());
        remove_partials(&self.dest);
        self.conclude(TransferState::Cancelled, "cancelled".to_string());
    }

    fn settle_failure(&self, message: &str) {
        let sidecar = {
            let mut inner = self.lock();
            self.revoke_all(&mut inner);
            self.resume_safe.then(|| self.sidecar_of(&inner))
        };
        match sidecar {
            // Keep what was fetched: a later begin can resume it.
            Some(sidecar) => self.checkpoint(&sidecar),
            None => remove_partials(&self.dest),
        }
        self.conclude(TransferState::Failed, format!("failed: {message}"));
    }

    fn settle_finalize(&self) {
        self.revoke_all(&mut self.lock());
        match self.finalize_file() {
            Ok(total) => self.conclude(TransferState::Completed, format!("completed: {total} bytes")),
            Err(error) => {
                self.lock().last_error = Some(error.to_string());
                // Everything was fetched: leave the data and a finished sidecar
                // in place, so a retry only has to move the file.
                let sidecar = self.resume_safe.then(|| self.sidecar_of(&self.lock()));
                if let Some(sidecar) = sidecar {
                    self.checkpoint(&sidecar);
                }
                self.conclude(TransferState::Failed, format!("failed: {error}"));
            }
        }
    }

    /// `fsync`, check the length, and atomically move the data file over
    /// the destination -- which therefore never holds a partial file.
    fn finalize_file(&self) -> Result<u64, DownloadError> {
        self.file.sync_all()?;
        let length = self.file.metadata()?.len();
        if let Some(total) = self.probe.total {
            if length != total {
                return Err(DownloadError::Io(format!("the finished file is {length} bytes, expected {total}")));
            }
        }
        if !self.options.overwrite && self.dest.exists() {
            return Err(DownloadError::DestinationExists(self.dest.clone()));
        }
        std::fs::rename(part_path(&self.dest), &self.dest)?;
        let sidecar = sidecar_path(&self.dest);
        let _ = std::fs::remove_file(&sidecar);
        remove_partials(&self.dest);
        Ok(length)
    }
}

fn worker(shared: Arc<Shared>) {
    while let Some(claim) = shared.claim() {
        let outcome = shared.run_segment(&claim);
        shared.finish_segment(&claim, outcome);
    }
    shared.lock().workers -= 1;
    shared.changed.notify_all();
}

fn coordinate(shared: Arc<Shared>) {
    let mut last_checkpoint = Instant::now();
    let mut last_published: Option<Snapshot> = None;
    loop {
        let (phase, snapshot, checkpoint) = {
            let mut inner = shared.lock();
            if inner.phase == Phase::Running {
                inner = shared.wait_for(inner, shared.options.tick);
            }
            let now = Instant::now();
            if inner.phase == Phase::Running {
                if inner.segments.iter().all(|s| s.pos >= s.end) {
                    inner.phase = Phase::Finalizing;
                } else {
                    shared.watchdog(&mut inner, now);
                    Shared::top_up(&shared, &mut inner, now);
                }
            }
            let completed = completed_bytes(&inner);
            inner.progress.record(now, completed);
            let checkpoint = if inner.phase == Phase::Running && inner.dirty && shared.resume_safe && now.saturating_duration_since(last_checkpoint) >= shared.options.checkpoint_interval {
                inner.dirty = false;
                Some(shared.sidecar_of(&inner))
            } else {
                None
            };
            (inner.phase.clone(), shared.build_snapshot(&inner), checkpoint)
        };

        if last_published.as_ref() != Some(&snapshot) {
            shared.publish(&snapshot);
            last_published = Some(snapshot);
        }
        match phase {
            Phase::Running => {
                if let Some(sidecar) = checkpoint {
                    shared.checkpoint(&sidecar);
                    last_checkpoint = Instant::now();
                }
            }
            Phase::Pausing => return shared.settle_pause(),
            Phase::Cancelling => return shared.settle_cancel(),
            Phase::Failing(message) => return shared.settle_failure(&message),
            Phase::Finalizing => return shared.settle_finalize(),
            Phase::Finished(_) => return,
        }
    }
}
