// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// A small RAII gate for the one operation that must be globally
/// serialized within a broker: a v1 → v2 cutover. Releasing it in
/// [`Drop`] covers all fail-closed early returns in [`cutover`].
pub(super) struct CutoverGate {
    pub(super) in_progress: AtomicBool,
}

impl CutoverGate {
    pub(super) fn new() -> Self {
        Self {
            in_progress: AtomicBool::new(false),
        }
    }

    pub(super) fn try_acquire(&self) -> Option<CutoverGuard<'_>> {
        self.in_progress
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| CutoverGuard { gate: self })
    }
}

pub(super) struct CutoverGuard<'a> {
    pub(super) gate: &'a CutoverGate,
}

impl Drop for CutoverGuard<'_> {
    fn drop(&mut self) {
        self.gate.in_progress.store(false, Ordering::Release);
    }
}

/// A generous but bounded wait for [`capture_v1_tabs`]'s `Tabs` reply --
/// a few seconds, per `phase-8-live-core-hotswap/PLAN.md`'s "a
/// reasonable timeout... if it never arrives, that's a `CutoverFailed`."
pub(super) const TAB_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// A cheap, dependency-free, sufficiently-unique request id for the
/// launcher's own synthetic internal-client messages (`ListTabs` during
/// tab capture; `Navigate`/`OpenTab`/`ListTabs` during replay and the
/// post-replay health check) -- time-based rather than a small
/// sequential counter, so an accidental collision with another,
/// independently-numbered client's own (typically small, sequential)
/// request_id sharing the same broker is vanishingly unlikely. Mirrors
/// `blueice-mcp-server`'s own `fastrand_like_suffix`.
pub(super) fn synthetic_request_id() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Captures v1's currently-open tab list, as a synthetic internal
/// client of the broker -- `core` only ever has the one real connection
/// `core_writer` holds; every external client's traffic (and this
/// synthetic one) is proxied through it, so this reuses the exact same
/// [`register_client`]-style registration rather than a side channel.
/// Filters the broadcast stream for the specific reply carrying the
/// request ID this call generated itself, exactly the discipline
/// `blueice-mcp-server`'s `send_and_drain` already established for
/// picking one reply out of shared, multi-client traffic --
/// reimplemented locally per this crate's existing duplicate-small-
/// helpers convention.
///
/// The synthetic client is never explicitly removed from `clients`:
/// once this call returns, its local `receiver` is dropped, so the
/// very next broadcast attempt to its paired `Sender` (still sitting in
/// `clients`) fails and [`broadcast_core_to_clients`]'s existing
/// dead-channel pruning removes it -- the same self-cleanup mechanism
/// already relied on for any other client's writer thread exiting, so
/// no new client-identity-tracking machinery is needed just for this.
pub(super) fn capture_v1_tabs(
    core_writer: &Arc<Mutex<UnixStream>>,
    clients: &Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    timeout: Duration,
) -> Result<Vec<TabSummary>, String> {
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(sender);

    let request_id = synthetic_request_id();
    {
        let mut core = core_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        write_client_message_with_id(&mut *core, Some(request_id), &ClientMessage::ListTabs)
            .map_err(|e| format!("failed to send ListTabs to v1: {e}"))?;
    }

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for v1's ListTabs reply".to_string());
        }
        match receiver.recv_timeout(remaining) {
            Ok(TaggedServerMessage {
                request_id: Some(id),
                message: ServerMessage::Tabs(tabs),
                ..
            }) if id == request_id => return Ok(tabs),
            Ok(_) => continue, // some other client's concurrent broadcast traffic
            // A timeout inside `recv_timeout` itself (as opposed to the
            // deadline check above) is not yet the final "timed out"
            // error -- loop back so that check produces the accurate
            // message once `remaining` is truly exhausted, rather than
            // this arm misreporting it as a disconnect.
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(
                    "v1's broadcast connection ended while waiting for its ListTabs reply"
                        .to_string(),
                );
            }
        }
    }
}

/// How many represented nodes v1 shows for each of `tabs`, in order, asked of
/// v1 exactly as [`capture_v1_tabs`] asks for the tab list. Best effort by
/// design: a tab whose representation does not arrive in time is `None` and is
/// simply skipped by the structural health check, since the health bar must not
/// fail a cutover merely because v1 was slow to describe one page.
pub(super) fn capture_v1_node_counts(
    core_writer: &Arc<Mutex<UnixStream>>,
    clients: &Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    tabs: &[TabSummary],
    timeout: Duration,
) -> Vec<Option<usize>> {
    let mut counts = vec![None; tabs.len()];
    if tabs.is_empty() {
        return counts;
    }
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(sender);

    // One request per tab, each with its own id, so replies map back by id.
    let base = synthetic_request_id();
    {
        let mut core = core_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (index, tab) in tabs.iter().enumerate() {
            let sent = blueice_ipc::write_client_message_with_ids(
                &mut *core,
                Some(tab.id),
                Some(base.wrapping_add(index as u64 + 1)),
                &ClientMessage::GetRepresentation,
            );
            if sent.is_err() {
                return counts;
            }
        }
    }

    let deadline = Instant::now() + timeout;
    let mut answered = 0;
    while answered < tabs.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(TaggedServerMessage {
                request_id: Some(id),
                message: ServerMessage::Representation(snapshot),
                ..
            }) => {
                let index = id.wrapping_sub(base).wrapping_sub(1) as usize;
                if index < counts.len() && counts[index].is_none() {
                    counts[index] = Some(snapshot.nodes.len());
                    answered += 1;
                }
            }
            Ok(_) => continue, // some other client's concurrent broadcast traffic
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    counts
}

/// Whether v2's rendering of a tab is structurally comparable to v1's. A
/// first-pass bar, as `phase-8-live-core-hotswap/PLAN.md` planned: v2 must not
/// be blank when v1 was not, and its node count must stay within half to double
/// of v1's. That catches a blank or crashed render without failing on a page
/// whose content legitimately changed between the two fetches. A tab v1 itself
/// showed nothing for (a blank tab) constrains nothing.
pub(super) fn structure_comparable(v1_nodes: usize, v2_nodes: usize) -> bool {
    if v1_nodes == 0 {
        return true;
    }
    v2_nodes > 0 && v2_nodes * 2 >= v1_nodes && v2_nodes <= v1_nodes * 2
}

/// Replays `captured_tabs` (v1's [`TabSummary`] list, in the order
/// [`capture_v1_tabs`] returned them) into `v2`, talking directly to
/// `v2_stream` -- v2 has no other client yet, so there's no broadcast/
/// synthetic-client dance needed here, unlike [`capture_v1_tabs`].
/// `Navigate{url}` for the first (v2's own already-existing default)
/// tab, skipped entirely if it had no url (leaves that slot blank, same
/// as a fresh tab); `OpenTab{url}` for every subsequent one (itself
/// `Option<String>`, so a blank later tab is replayed as a blank
/// `OpenTab` too, preserving v1's own tab count). The first failure (an
/// `Error`/`GatekeeperBlocked` reply, or the connection breaking)
/// aborts the whole replay -- `phase-8-live-core-hotswap/PLAN.md`'s
/// single-attempt, fail-closed contract.
pub(super) fn replay_tabs(
    v2_stream: &mut UnixStream,
    captured_tabs: &[TabSummary],
) -> Result<Vec<TaggedServerMessage>, String> {
    let mut replay_frames = Vec::new();
    for (i, tab) in captured_tabs.iter().enumerate() {
        if i == 0 {
            let Some(url) = &tab.url else { continue };
            let request_id = synthetic_request_id();
            write_client_message_with_id(
                v2_stream,
                Some(request_id),
                &ClientMessage::Navigate { url: url.clone() },
            )
            .map_err(|e| format!("failed to replay the default tab's Navigate into v2: {e}"))?;
            replay_frames.push(expect_navigate_success(v2_stream, request_id)?);
        } else {
            let request_id = synthetic_request_id();
            write_client_message_with_id(
                v2_stream,
                Some(request_id),
                &ClientMessage::OpenTab {
                    url: tab.url.clone(),
                },
            )
            .map_err(|e| format!("failed to replay tab {i}'s OpenTab into v2: {e}"))?;
            if let Some(frame) = expect_open_tab_success(v2_stream, request_id, tab.url.is_some())?
            {
                replay_frames.push(frame);
            }
        }
    }
    Ok(replay_frames)
}

/// Reads from `v2_stream` until the reply to `request_id` -- `Navigated`
/// on success -- is seen, then keeps reading until the `FrameReady`
/// that always follows it (`session.rs`'s `reply_success` always sends
/// both, in that order) arrives too, so this call never leaves an
/// unread `FrameReady` on the wire for the next replay message to
/// misinterpret -- mirroring `blueice-mcp-server`'s own
/// `send_and_drain`/`open_tab` discipline. Any `Error`/
/// `GatekeeperBlocked` reply, or the read itself failing (v2's
/// connection broke, e.g. because writing the frame failed), is a
/// replay failure.
pub(super) fn expect_navigate_success(
    v2_stream: &mut UnixStream,
    request_id: u64,
) -> Result<TaggedServerMessage, String> {
    let mut navigated = false;
    loop {
        let (tab_id, reply_id, message) = read_server_message_with_ids(v2_stream)
            .map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if reply_id != Some(request_id) {
            continue;
        }
        match message {
            ServerMessage::Navigated { .. } => navigated = true,
            frame @ ServerMessage::FrameReady { .. } if navigated => {
                return Ok(TaggedServerMessage {
                    tab_id,
                    request_id: None, // a cutover handoff, not the synthetic replay request
                    message: frame,
                });
            }
            ServerMessage::FrameReady { .. } => continue, // shouldn't happen before Navigated, but don't misinterpret
            ServerMessage::Error { message } => {
                return Err(format!("v2 rejected the replayed Navigate: {message}"));
            }
            ServerMessage::GatekeeperBlocked { reason, .. } => {
                return Err(format!(
                    "v2's gatekeeper blocked the replayed Navigate: {reason}"
                ));
            }
            _ => continue,
        }
    }
}

/// Like [`expect_navigate_success`], for a replayed `OpenTab` --
/// `session.rs`'s `handle_open_tab` only sends a `FrameReady` after
/// `TabOpened` when the tab was actually navigated (`expects_frame`),
/// so a blank replayed tab is considered done as soon as `TabOpened`
/// itself arrives.
pub(super) fn expect_open_tab_success(
    v2_stream: &mut UnixStream,
    request_id: u64,
    expects_frame: bool,
) -> Result<Option<TaggedServerMessage>, String> {
    let mut opened = false;
    loop {
        let (tab_id, reply_id, message) = read_server_message_with_ids(v2_stream)
            .map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if reply_id != Some(request_id) {
            continue;
        }
        match message {
            ServerMessage::TabOpened { .. } => {
                opened = true;
                if !expects_frame {
                    return Ok(None);
                }
            }
            frame @ ServerMessage::FrameReady { .. } if opened => {
                return Ok(Some(TaggedServerMessage {
                    tab_id,
                    request_id: None,
                    message: frame,
                }));
            }
            ServerMessage::FrameReady { .. } => continue,
            ServerMessage::Error { message } => {
                return Err(format!("v2 rejected the replayed OpenTab: {message}"));
            }
            ServerMessage::GatekeeperBlocked { reason, .. } => {
                return Err(format!(
                    "v2's gatekeeper blocked the replayed OpenTab: {reason}"
                ));
            }
            _ => continue,
        }
    }
}

/// Confirms v2 actually reflects what was just replayed: sends
/// `ListTabs` directly on `v2_stream` and checks the tab count and URLs
/// match `captured_tabs` -- the "lighter" health bar this minimal
/// slice's plan settled on (structurally diffing each tab's DOM/
/// representation against v1's own captured state is explicitly
/// deferred).
pub(super) fn health_check(
    v2_stream: &mut UnixStream,
    captured_tabs: &[TabSummary],
) -> Result<Vec<u64>, String> {
    let request_id = synthetic_request_id();
    write_client_message_with_id(v2_stream, Some(request_id), &ClientMessage::ListTabs)
        .map_err(|e| format!("failed to send v2's health-check ListTabs: {e}"))?;
    loop {
        let (reply_id, message) = read_server_message_with_id(v2_stream)
            .map_err(|e| format!("failed reading v2's health-check reply: {e}"))?;
        if matches!(reply_id, Some(id) if id != request_id) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => {
                let expected: Vec<&Option<String>> = captured_tabs.iter().map(|t| &t.url).collect();
                let actual: Vec<&Option<String>> = tabs.iter().map(|t| &t.url).collect();
                return if actual == expected {
                    Ok(tabs.iter().map(|t| t.id).collect())
                } else {
                    Err(format!(
                        "v2's post-replay ListTabs didn't match what was captured from v1: expected {expected:?}, got {actual:?}"
                    ))
                };
            }
            _ => continue,
        }
    }
}

/// The structural half of the health bar: each replayed tab of v2 (`v2_tab_ids`,
/// in replay order) must render something [`structure_comparable`] to what v1
/// showed for the same tab. Tabs with no v1 measurement are skipped.
pub(super) fn structural_health_check(
    v2_stream: &mut UnixStream,
    v2_tab_ids: &[u64],
    v1_node_counts: &[Option<usize>],
) -> Result<(), String> {
    for (index, tab_id) in v2_tab_ids.iter().enumerate() {
        let Some(v1_nodes) = v1_node_counts.get(index).copied().flatten() else {
            continue;
        };
        let request_id = synthetic_request_id();
        blueice_ipc::write_client_message_with_ids(
            v2_stream,
            Some(*tab_id),
            Some(request_id),
            &ClientMessage::GetRepresentation,
        )
        .map_err(|e| format!("failed to ask v2 to describe replayed tab {index}: {e}"))?;
        let v2_nodes = loop {
            let (_, reply_id, message) = read_server_message_with_ids(v2_stream).map_err(|e| {
                format!("failed reading v2's description of replayed tab {index}: {e}")
            })?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::Representation(snapshot) => break snapshot.nodes.len(),
                ServerMessage::Error { message } => {
                    return Err(format!(
                        "v2 could not describe replayed tab {index}: {message}"
                    ));
                }
                _ => continue,
            }
        };
        if !structure_comparable(v1_nodes, v2_nodes) {
            return Err(format!(
                "v2's post-replay representation of tab {index} is structurally different from v1's: v1 showed {v1_nodes} node(s), v2 shows {v2_nodes}"
            ));
        }
    }
    Ok(())
}

/// A fresh, unique frame directory for a newly-[`SpawnedCore::spawn`]ed
/// v2, derived from v1's own -- see [`Broker::frame_dir`]'s docs for
/// why sharing one directory between two simultaneously-live `core`
/// instances isn't safe. `target_generation` (the generation this
/// cutover attempt would bump to if it succeeds) makes this unique
/// across repeated cutover attempts too, not just between v1 and v2.
pub(super) fn v2_frame_dir(v1_frame_dir: &Path, target_generation: u64) -> PathBuf {
    let stem = v1_frame_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frames".to_string());
    v1_frame_dir.with_file_name(format!("{stem}-cutover-{target_generation}"))
}

/// The actual swap, once replay + health check have both fully
/// succeeded. Retarget the writer under its lock, suppress late v1
/// broadcasts, hand the already-rendered v2 replay frames to existing
/// clients, then start v2's live broadcast. This ensures the frontend
/// sees the new core without needing another user action.
pub(super) fn perform_swap(
    broker: &Arc<Broker>,
    v2: SpawnedCore,
    target_generation: u64,
    replay_frames: Vec<TaggedServerMessage>,
) {
    // Every relay accept snapshots its target while holding this gate.
    // Hold it across the browser-writer swap and both relay activations,
    // so no newly accepted compiler or debugger connection can observe a
    // different core generation from browser traffic.
    let _route_handoff = broker
        .route_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let v2_broadcast_stream = v2
        .stream
        .try_clone()
        .expect("try_clone on a fresh stream should not fail");
    let v2_writer_stream = v2
        .stream
        .try_clone()
        .expect("try_clone on a fresh stream should not fail");
    let mut writer = broker
        .core_writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut active = broker
        .active_core
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Publish the new writer, process, and generation while holding both
    // locks. A concurrent permission inspection cannot pair v1's private
    // pipe with v2's generation (or vice versa) during this transition.
    broker.generation.store(target_generation, Ordering::SeqCst);
    *writer = v2_writer_stream;
    // The replacement private listeners are ready before replay/health
    // checking. Under the shared handoff gate, make them targets only for
    // future accepts after browser traffic has moved to the same core.
    v2.activate_relays_after_handoff();
    let v1 = active.replace(v2);
    drop(active);
    if let Some(v1) = v1.as_ref() {
        // Unblocks v1's old broadcast thread's blocked read (shutdown
        // affects every fd sharing this socket's underlying open file
        // description, including that thread's own clone) so it exits
        // through the ordinary disconnect path -- which it now
        // recognizes as an expected supersession, not a fault, since
        // `generation` was already bumped above.
        let _ = v1.stream.shutdown(Shutdown::Both);
    }

    {
        let mut clients = broker
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for frame in replay_frames {
            clients.retain(|client| client.send(frame.clone()).is_ok());
        }
    }
    drop(writer);

    spawn_generation_tagged_broadcast(
        v2_broadcast_stream,
        Arc::clone(&broker.clients),
        Arc::clone(&broker.generation),
        target_generation,
        broker.done.clone(),
    );
    drop(v1); // reap v1 after clients have already received v2's frames
}

/// Performs one cutover attempt: captures v1's tab list, spawns v2,
/// replays the tabs into it, health-checks it, and -- only if all of
/// that succeeds -- performs the actual swap. A single attempt, fail
/// closed to v1 on any error -- see `phase-8-live-core-hotswap/PLAN.md`'s
/// "Wiring design" for why no retry policy exists yet. v1 is never
/// touched until every step through the health check has succeeded.
/// Why one cutover attempt did not succeed, and whether trying again could
/// plausibly help. Every failure leaves v1 untouched and serving.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AttemptFailure {
    /// Environmental and plausibly transient (v2 failed to start, a reply timed
    /// out): retry.
    Retry(String),
    /// v2 ran and failed its health check. One more attempt is reasonable (it
    /// may have been a startup race); a second identical failure is
    /// deterministic.
    RetryOnce(String),
    /// Deterministic: v2 rejected the replay, or its gatekeeper blocked a
    /// replayed URL. Trying again would only repeat it.
    Final(String),
}

/// A cutover makes at most this many attempts.
pub(super) const MAX_CUTOVER_ATTEMPTS: u32 = 3;
/// The pause before attempt `n + 1` is this times `2^(n - 1)`.
pub(super) const CUTOVER_RETRY_BASE_PAUSE: Duration = Duration::from_millis(250);
/// How long v2 may take to answer any single replay or health-check message
/// before the attempt is abandoned rather than left hanging.
pub(super) const V2_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Classifies a replay or health-check error message by what produced it. The
/// messages are all created in this file, and a test pins every one, so a
/// reworded message cannot silently change how it is retried.
pub(super) fn classify_attempt_error(reason: String) -> AttemptFailure {
    if reason.contains("gatekeeper blocked") || reason.contains("rejected the replayed") {
        AttemptFailure::Final(reason)
    } else if reason.contains("post-replay ListTabs")
        || reason.contains("post-replay representation")
    {
        AttemptFailure::RetryOnce(reason)
    } else {
        AttemptFailure::Retry(reason)
    }
}

/// Runs `attempt` (given its 1-based number) until it succeeds or the policy
/// gives up: never more than [`MAX_CUTOVER_ATTEMPTS`], never again after a
/// [`AttemptFailure::Final`], and at most once again after a
/// [`AttemptFailure::RetryOnce`] has already happened. `pause` is called with
/// the delay before each retry (a parameter so the policy is a plain unit test).
pub(super) fn run_with_retries<T>(
    mut attempt: impl FnMut(u32) -> Result<T, AttemptFailure>,
    mut pause: impl FnMut(Duration),
) -> Result<T, String> {
    let mut health_failures = 0u32;
    let mut last_reason = String::new();
    for number in 1..=MAX_CUTOVER_ATTEMPTS {
        match attempt(number) {
            Ok(done) => return Ok(done),
            Err(AttemptFailure::Final(reason)) => {
                return Err(format!("{reason} (attempt {number}; not retryable)"));
            }
            Err(AttemptFailure::RetryOnce(reason)) => {
                health_failures += 1;
                if health_failures >= 2 {
                    return Err(format!(
                        "{reason} (attempt {number}; failed the health check twice)"
                    ));
                }
                last_reason = reason;
            }
            Err(AttemptFailure::Retry(reason)) => last_reason = reason,
        }
        if number < MAX_CUTOVER_ATTEMPTS {
            pause(CUTOVER_RETRY_BASE_PAUSE * 2u32.pow(number - 1));
        }
    }
    Err(format!(
        "{last_reason} (gave up after {MAX_CUTOVER_ATTEMPTS} attempts)"
    ))
}

/// One try: spawn v2, replay v1's tabs into it, and health-check it. On any
/// failure v2 is dropped (killed and cleaned up) and v1 has not been touched.
pub(super) fn attempt_cutover(
    broker: &Arc<Broker>,
    captured_tabs: &[TabSummary],
    v1_node_counts: &[Option<usize>],
    target_generation: u64,
    attempt: u32,
) -> Result<(SpawnedCore, Vec<TaggedServerMessage>), AttemptFailure> {
    let mut frame_dir = v2_frame_dir(&broker.frame_dir, target_generation);
    if attempt > 1 {
        // A failed earlier attempt may not have finished removing its own.
        let name = frame_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        frame_dir.set_file_name(format!("{name}-retry{attempt}"));
    }
    // Reproduce the launcher-owned startup policy (including the compiler
    // and debugger relays), always pointing at this launcher's own
    // gatekeeper, installed package, and assistant.
    let mut options = broker.core_options.clone();
    options.gatekeeper_socket = Some(broker.gatekeeper_socket.clone());
    options.extension_manifest = broker.extension_manifest.clone();
    options.assistant = broker.assistant.clone();
    let mut v2 = SpawnedCore::spawn_with_options_and_relays(
        broker.width,
        broker.height,
        &frame_dir,
        options,
        RelaySet {
            route_gate: Arc::clone(&broker.route_gate),
            compiler_mcp_relay: broker.compiler_mcp_relay.clone(),
            debugger_relay: broker.debugger_relay.clone(),
        },
    )
    .map_err(|e| AttemptFailure::Retry(format!("failed to spawn v2: {e}")))?;

    // A v2 that stops answering must fail this attempt, not hang the cutover.
    let _ = v2.stream.set_read_timeout(Some(V2_REPLY_TIMEOUT));
    let replay_frames =
        replay_tabs(&mut v2.stream, captured_tabs).map_err(classify_attempt_error)?;
    let v2_tab_ids = health_check(&mut v2.stream, captured_tabs).map_err(classify_attempt_error)?;
    structural_health_check(&mut v2.stream, &v2_tab_ids, v1_node_counts)
        .map_err(classify_attempt_error)?;
    // v2's stream is about to become the live connection, read by a broadcast
    // thread that blocks indefinitely by design.
    let _ = v2.stream.set_read_timeout(None);
    Ok((v2, replay_frames))
}

pub(crate) fn cutover(broker: &Arc<Broker>) -> control::ControlReply {
    let Some(_guard) = broker.cutover_gate.try_acquire() else {
        crate::trace::event("cutover.busy", "another cutover is in flight");
        return control::ControlReply::CutoverBusy;
    };
    crate::trace::event("cutover.start", "");

    let captured_tabs =
        match capture_v1_tabs(&broker.core_writer, &broker.clients, TAB_CAPTURE_TIMEOUT) {
            Ok(tabs) => tabs,
            Err(reason) => return control::ControlReply::CutoverFailed { reason },
        };

    // What v1 shows for each tab, measured before v2 exists, so v2's rendering
    // has something structural to be compared against.
    let v1_node_counts = capture_v1_node_counts(
        &broker.core_writer,
        &broker.clients,
        &captured_tabs,
        TAB_CAPTURE_TIMEOUT,
    );

    let target_generation = broker.generation.load(Ordering::SeqCst) + 1;
    match run_with_retries(
        |attempt| {
            attempt_cutover(
                broker,
                &captured_tabs,
                &v1_node_counts,
                target_generation,
                attempt,
            )
        },
        thread::sleep,
    ) {
        Ok((v2, replay_frames)) => {
            let tabs_migrated = captured_tabs.len();
            perform_swap(broker, v2, target_generation, replay_frames);
            crate::trace::event(
                "cutover.done",
                &format!("generation {target_generation}, {tabs_migrated} tab(s)"),
            );
            control::ControlReply::CutoverDone { tabs_migrated }
        }
        Err(reason) => {
            crate::trace::event("cutover.failed", &reason);
            control::ControlReply::CutoverFailed { reason }
        }
    }
}

/// Handles one control-socket connection: routes cutover through the
/// single-flight gate, or performs a bounded, read-only inspection of the
/// active core's private permission pipe. This socket cannot grant or
/// revoke an extension capability.
pub(super) fn handle_control_connection(
    mut conn: UnixStream,
    broker: &Arc<Broker>,
) -> io::Result<()> {
    let request = control::read_control_request(&mut conn)?;
    let reply = match request {
        control::ControlRequest::Cutover => cutover(broker),
        control::ControlRequest::InspectExtensionPermissions => {
            let active = broker
                .active_core
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let core_generation = broker.generation.load(Ordering::SeqCst);
            match active
                .as_ref()
                .map(SpawnedCore::inspect_installed_extension)
            {
                Some(Ok(installed)) => control::ControlReply::ExtensionPermissions {
                    core_generation,
                    installed,
                },
                Some(Err(error)) => control::ControlReply::ExtensionPermissionsUnavailable {
                    reason: error.to_string(),
                },
                None => control::ControlReply::ExtensionPermissionsUnavailable {
                    reason: "the active core is unavailable".into(),
                },
            }
        }
        control::ControlRequest::Status => {
            let core_pid = broker
                .active_core
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .map(|core| core.child.id());
            control::ControlReply::Status(Box::new(control::LauncherStatus {
                launcher_pid: std::process::id(),
                core_generation: broker.generation.load(Ordering::SeqCst),
                core_pid,
                assistant: broker
                    .assistant_settings
                    .as_ref()
                    .map(|s| s.assistant_status()),
                pending_proposal: broker
                    .assistant_settings
                    .as_ref()
                    .and_then(|s| s.pending())
                    .map(|view| control::PendingProposalStatus {
                        id: view.id,
                        seconds_left: view.expires_in.as_secs(),
                    }),
            }))
        }
        control::ControlRequest::ProposeAssistantSettings { settings } => {
            match broker.assistant_settings.as_ref() {
                None => control::ControlReply::AssistantProposalRefused {
                    reason: "this launcher supervises no assistant".to_string(),
                },
                Some(service) => match service.propose(settings) {
                    assistant_proposals::ProposeOutcome::Accepted { id, digest, diff } => {
                        control::ControlReply::AssistantProposalAccepted { id, digest, diff }
                    }
                    assistant_proposals::ProposeOutcome::Blocked(violations) => {
                        control::ControlReply::AssistantProposalBlocked { violations }
                    }
                    assistant_proposals::ProposeOutcome::PendingExists => {
                        control::ControlReply::AssistantProposalRefused {
                            reason: "a proposal is already waiting for the person's decision"
                                .to_string(),
                        }
                    }
                    assistant_proposals::ProposeOutcome::RateLimited => {
                        control::ControlReply::AssistantProposalRefused {
                            reason: "too many proposals this hour".to_string(),
                        }
                    }
                },
            }
        }
        control::ControlRequest::InspectAssistantSettings => {
            match broker.assistant_settings.as_ref() {
                None => control::ControlReply::AssistantProposalRefused {
                    reason: "this launcher supervises no assistant".to_string(),
                },
                Some(service) => control::ControlReply::AssistantSettingsInForce {
                    settings: Box::new(service.current()),
                },
            }
        }
        control::ControlRequest::AssistantProposalStatus { id } => {
            match broker.assistant_settings.as_ref() {
                None => control::ControlReply::AssistantProposalRefused {
                    reason: "this launcher supervises no assistant".to_string(),
                },
                Some(service) => control::ControlReply::AssistantProposalStatus {
                    status: service.proposal_status(id).as_str().to_string(),
                },
            }
        }
    };
    control::write_control_reply(&mut conn, &reply)
}
