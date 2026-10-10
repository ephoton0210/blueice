// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Process-isolated regular expressions. The parent never executes regress.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::RuntimeError;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
// Process creation and cold DLL loading are host work, not regex execution.
// Keep this separate from the short per-operation deadline so a loaded or
// resource-constrained Windows VM cannot turn a valid regex into a harness
// error before its worker can announce readiness.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FRAME: usize = 16 * 1024 * 1024;
const READY: &[u8] = b"bluejs-regexp-worker/1";

/// Helper processes this process has started so far.
static STARTED: AtomicU64 = AtomicU64::new(0);

/// How many helper processes this process has started. A diagnostic for tests:
/// a healthy process keeps one worker per concurrently matching thread rather
/// than starting one per operation.
#[doc(hidden)]
pub fn workers_started() -> u64 {
    STARTED.load(Ordering::Relaxed)
}

/// Requests sent to helper processes so far (every operation that crossed the
/// pipe, whichever process served it).
static ROUND_TRIPS: AtomicU64 = AtomicU64::new(0);

/// How many requests this process has sent to helper processes. A diagnostic
/// for tests: repeating an identical match must be answered from the parent's
/// memo rather than by another round trip.
#[doc(hidden)]
pub fn round_trips() -> u64 {
    ROUND_TRIPS.load(Ordering::Relaxed)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Request {
    Compile {
        source: Vec<u16>,
        flags: String,
    },
    Find {
        // Omitted fields reuse the worker's most recently supplied pattern
        // or subject. The parent only omits them after this same worker has
        // acknowledged the corresponding full request.
        source: Option<Vec<u16>>,
        flags: Option<String>,
        input: Option<Vec<u16>>,
        start: usize,
    },
    Validate {
        patterns: Vec<(Vec<u16>, String)>,
    },
    Shutdown,
}

// Keep accepting the original untagged wire format for direct version-one
// clients. `deny_unknown_fields` is essential here: without it, a cached
// `Find` whose subject is omitted also satisfies the old `Compile` shape and
// the worker replies `Compiled` to a match request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRequest {
    source: Vec<u16>,
    flags: String,
    input: Option<Vec<u16>>,
    start: usize,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IncomingRequest {
    Current(Request),
    Legacy(LegacyRequest),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Reply {
    Compiled,
    Found(Option<Match>),
    Validated(Vec<bool>),
    SyntaxError(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Match {
    captures: Vec<Option<Range<usize>>>,
    names: Vec<(String, Option<Range<usize>>)>,
}

impl Match {
    pub(crate) fn whole(range: Range<usize>) -> Self {
        Self {
            captures: vec![Some(range)],
            names: Vec::new(),
        }
    }

    pub fn start(&self) -> usize {
        self.captures[0].as_ref().unwrap().start
    }
    pub fn end(&self) -> usize {
        self.captures[0].as_ref().unwrap().end
    }
    pub fn group(&self, index: usize) -> Option<Range<usize>> {
        self.captures.get(index).cloned().flatten()
    }
    pub fn groups(&self) -> impl Iterator<Item = Option<Range<usize>>> + '_ {
        self.captures.iter().cloned()
    }
    pub fn named_groups(&self) -> impl Iterator<Item = (&str, Option<Range<usize>>)> + '_ {
        self.names
            .iter()
            .map(|(name, range)| (name.as_str(), range.clone()))
    }
    fn valid(&self, length: usize) -> bool {
        self.captures.first().is_some_and(Option::is_some)
            && self
                .captures
                .iter()
                .chain(self.names.iter().map(|(_, range)| range))
                .flatten()
                .all(|r| r.start <= r.end && r.end <= length)
    }
}

fn frame_write(writer: &mut dyn Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "regex frame exceeds limit",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

fn frame_read(reader: &mut dyn Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "regex frame exceeds limit",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

struct Worker {
    child: Child,
    requests: Option<Sender<Vec<u8>>>,
    replies: Receiver<io::Result<Vec<u8>>>,
    io_thread: Option<JoinHandle<()>>,
    failed: bool,
    pattern: Option<(Vec<u16>, String)>,
    input: Option<Vec<u16>>,
}

/// The matcher executable's file name on this platform.
#[cfg(test)]
fn bare_worker_path() -> PathBuf {
    PathBuf::from(format!(
        "bluejs-regexp-worker{}",
        std::env::consts::EXE_SUFFIX
    ))
}

/// The matcher executable next to `executable` (or one directory up from a
/// Cargo `deps`/`examples` directory), given the running executable's path.
#[cfg(test)]
fn sibling_worker_path(executable: io::Result<PathBuf>) -> io::Result<PathBuf> {
    let executable = executable?;
    let mut directory = executable.parent().unwrap();
    if directory
        .file_name()
        .is_some_and(|name| name == "deps" || name == "examples")
    {
        directory = directory.parent().unwrap();
    }
    Ok(directory.join(bare_worker_path()))
}

/// A reply frame's payload: replies are plain data and always serialize.
fn encode_reply(reply: &Reply) -> Vec<u8> {
    serde_json::to_vec(reply).expect("a regex reply always serializes")
}

impl Worker {
    fn start() -> io::Result<Self> {
        let path = match std::env::var_os("BLUEJS_REGEXP_WORKER") {
            Some(path) => Ok(PathBuf::from(path)),
            None => Self::sibling_worker_path(std::env::current_exe()),
        };
        Self::start_from_path(path)
    }

    fn sibling_worker_path(executable: io::Result<PathBuf>) -> io::Result<PathBuf> {
        let executable = executable?;
        let mut directory = executable.parent().unwrap();
        if directory
            .file_name()
            .is_some_and(|name| name == "deps" || name == "examples")
        {
            directory = directory.parent().unwrap();
        }
        Ok(directory.join(format!(
            "bluejs-regexp-worker{}",
            std::env::consts::EXE_SUFFIX
        )))
    }

    fn start_from_path(path: io::Result<PathBuf>) -> io::Result<Self> {
        let path = path?;
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        STARTED.fetch_add(1, Ordering::Relaxed);
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let (requests, incoming) = mpsc::channel::<Vec<u8>>();
        let (outgoing, replies) = mpsc::channel();
        let io_thread = std::thread::spawn(move || {
            let ready = frame_read(&mut output);
            let failed = ready.is_err();
            if outgoing.send(ready).is_err() || failed {
                return;
            }
            for bytes in incoming {
                let result = frame_write(&mut input, &bytes).and_then(|()| frame_read(&mut output));
                let failed = result.is_err();
                if outgoing.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            requests: Some(requests),
            replies,
            io_thread: Some(io_thread),
            failed: false,
            pattern: None,
            input: None,
        };
        // Process startup is separately bounded; cold executable loading must
        // not consume a short budget intended for a regex operation.
        let ready = worker.replies.recv_timeout(STARTUP_TIMEOUT);
        match ready {
            Ok(Ok(ready)) if ready == READY => Ok(worker),
            _ => {
                worker.failed = true;
                Err(io::Error::other(
                    "regex worker startup failed or exceeded fifteen seconds",
                ))
            }
        }
    }

    fn transact(&mut self, bytes: Vec<u8>, timeout: Duration) -> Result<Vec<u8>, RuntimeError> {
        ROUND_TRIPS.fetch_add(1, Ordering::Relaxed);
        self.requests
            .as_ref()
            .unwrap()
            .send(bytes)
            .map_err(|error| worker_error(error.to_string()))?;
        let reply = self.replies.recv_timeout(timeout);
        reply
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => RuntimeError::RegexTimeout,
                error => worker_error(error.to_string()),
            })?
            .map_err(|error| worker_error(error.to_string()))
    }

    fn request(
        &mut self,
        request: Request,
        timeout: Duration,
        input_length: Option<usize>,
    ) -> Result<Reply, RuntimeError> {
        let bytes = serde_json::to_vec(&request).expect("a regex request always serializes");
        if bytes.len() > MAX_FRAME {
            return Err(worker_error("regex request exceeds frame limit".into()));
        }
        let bytes = self.transact(bytes, timeout)?;
        let reply: Reply =
            serde_json::from_slice(&bytes).map_err(|error| worker_error(error.to_string()))?;
        if input_length.is_some_and(
            |length| matches!(&reply, Reply::Found(Some(matched)) if !matched.valid(length)),
        ) {
            return Err(worker_error("invalid regex capture range".into()));
        }
        Ok(reply)
    }

    fn compile(
        &mut self,
        source: Vec<u16>,
        flags: String,
        timeout: Duration,
    ) -> Result<Reply, RuntimeError> {
        let reply = self.request(
            Request::Compile {
                source: source.clone(),
                flags: flags.clone(),
            },
            timeout,
            None,
        )?;
        if matches!(reply, Reply::Compiled) {
            self.pattern = Some((source, flags));
        }
        Ok(reply)
    }

    fn find(
        &mut self,
        source: Vec<u16>,
        flags: String,
        input: Vec<u16>,
        start: usize,
        timeout: Duration,
    ) -> Result<Option<Match>, RuntimeError> {
        let include_pattern = !self
            .pattern
            .as_ref()
            .is_some_and(|(old_source, old_flags)| *old_source == source && *old_flags == flags);
        let include_input = self.input.as_ref() != Some(&input);
        let reply = self.request(
            Request::Find {
                source: include_pattern.then(|| source.clone()),
                flags: include_pattern.then(|| flags.clone()),
                input: include_input.then(|| input.clone()),
                start,
            },
            timeout,
            Some(input.len()),
        )?;
        let Reply::Found(matched) = reply else {
            return Err(RuntimeError::RegexWorker("unexpected match reply".into()));
        };
        self.pattern = Some((source, flags));
        self.input = Some(input);
        Ok(matched)
    }

    fn validate(
        &mut self,
        patterns: Vec<(Vec<u16>, String)>,
        timeout: Duration,
    ) -> Result<Vec<bool>, RuntimeError> {
        let reply = self.request(Request::Validate { patterns }, timeout, None)?;
        let Reply::Validated(valid) = reply else {
            return Err(RuntimeError::RegexWorker(
                "unexpected validation reply".into(),
            ));
        };
        self.pattern = None;
        self.input = None;
        Ok(valid)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // A failed transaction can leave the I/O thread in its pipe read. On
        // Windows, joining it from the error path can block forever even
        // after terminating the private child. Close the sender so the I/O
        // thread exits once that read wakes, then detach only this failed
        // worker.
        #[cfg(windows)]
        if self.failed {
            let _ = self.child.kill();
            drop(self.requests.take());
            self.io_thread.take();
            return;
        }

        // Unix needs an explicit termination after an error to unblock a
        // pending pipe read before the I/O thread can be joined.
        #[cfg(not(windows))]
        if self.failed {
            let _ = self.child.kill();
        }

        // Closing stdin alone does not reliably wake a Windows pipe reader.
        // Ask a healthy worker to exit explicitly, then close the channel and
        // join its I/O thread before reaping the private child on every
        // platform. Workers are only dropped from ordinary code (never from a
        // thread's exit destructor; see `IDLE`), where that join is safe.
        if let Some(requests) = self.requests.as_ref() {
            let shutdown =
                serde_json::to_vec(&Request::Shutdown).expect("a shutdown request serializes");
            let _ = requests.send(shutdown);
        }
        drop(self.requests.take());
        if let Some(thread) = self.io_thread.take() {
            let _ = thread.join();
        }
        let _ = self.child.wait();
    }
}

fn worker_error(message: String) -> RuntimeError {
    RuntimeError::RegexWorker(message)
}

/// The worker's compiled pattern.
struct Compiled {
    source: Vec<u16>,
    flags: String,
    regex: regress::Regex,
    /// A case-insensitive pattern without the `u` and `v` flags is compiled
    /// case-sensitively from its canonical form and matched against the
    /// canonicalized subject (see `regex_canonicalize`).
    canonical: bool,
}

fn cache_pattern(
    cached: &mut Option<Compiled>,
    source: Vec<u16>,
    flags: String,
) -> Result<(), String> {
    let same = cached
        .as_ref()
        .is_some_and(|old| old.source == source && old.flags == flags);
    if same {
        return Ok(());
    }
    let pattern = crate::regex_group_names::regress_points(&source, &flags);
    let mut regress_flags = if pattern.canonical {
        flags.replace('i', "")
    } else {
        flags.clone()
    };
    // `v` includes everything `u` means, but `regress` only treats a pattern as
    // Unicode (case folding, for one) when it is also given `u`.
    if regress_flags.contains('v') && !regress_flags.contains('u') {
        regress_flags.push('u');
    }
    let regex = regress::Regex::from_unicode(
        pattern.points.into_iter(),
        regress::Flags::from(regress_flags.as_str()),
    )
    .map_err(|error| error.to_string())?;
    *cached = Some(Compiled {
        source,
        flags,
        regex,
        canonical: pattern.canonical,
    });
    Ok(())
}

/// Most idle helper processes kept for reuse. A helper is started only when no
/// idle one exists, so this bounds the processes left behind by a burst of
/// concurrent matching threads rather than limiting concurrency itself.
const MAX_IDLE_WORKERS: usize = 8;

/// Idle helpers, shared by every thread. They are deliberately not
/// thread-local: a thread-local `Worker` would be torn down by the thread's
/// exit destructor, and on Windows waiting for an I/O thread there can hang or
/// abort. Retiring the worker after every operation avoided that, at the price
/// of one process start per RegExp operation (milliseconds each, and a
/// Test262 case makes thousands). A static is never destructed, and every
/// worker is dropped from ordinary code outside any lock, so both the process
/// reuse and a deterministic shutdown are kept.
static IDLE: Mutex<Vec<Worker>> = Mutex::new(Vec::new());

fn with_worker<T>(
    operation: impl FnOnce(&mut Worker) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let idle = IDLE.lock().unwrap_or_else(PoisonError::into_inner).pop();
    let mut worker = match idle {
        Some(worker) => worker,
        None => Worker::start().map_err(|error| worker_error(error.to_string()))?,
    };
    let result = operation(&mut worker);
    // A rejected pattern is an expected reply from a healthy worker. Keep it;
    // transport and timeout failures require a fresh process for the next
    // operation.
    if result
        .as_ref()
        .is_err_and(|error| !matches!(error, RuntimeError::SyntaxError(_)))
    {
        worker.failed = true;
    } else {
        recycle_worker(worker, &IDLE);
        return result;
    }
    drop(worker);
    result
}

/// Retire an excess healthy helper outside the pool lock: its shutdown joins
/// the transport thread and waits for the owned child process.
fn recycle_worker(worker: Worker, pool: &Mutex<Vec<Worker>>) {
    let mut idle = pool.lock().unwrap_or_else(PoisonError::into_inner);
    if idle.len() < MAX_IDLE_WORKERS {
        idle.push(worker);
        return;
    }
    drop(idle);
    drop(worker);
}

/// Patterns the helper has already accepted. A regular expression literal in a
/// loop body is compiled on every iteration, and acceptance depends only on the
/// pattern and its flags. Rejected patterns are not kept: they are rare, and
/// their message must come from the helper. Cleared wholesale when full.
type Pattern = (Vec<u16>, String);
static ACCEPTED: Mutex<Option<HashSet<Pattern>>> = Mutex::new(None);
const ACCEPTED_ENTRIES: usize = 1024;
const ACCEPTED_KEY_UNITS: usize = 16 * 1024;

pub(crate) fn compile(
    source: Vec<u16>,
    flags: String,
    timeout: Duration,
) -> Result<Reply, RuntimeError> {
    let key = (source, flags);
    if ACCEPTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|accepted| accepted.contains(&key))
    {
        return Ok(Reply::Compiled);
    }
    let (source, flags) = key.clone();
    let reply = with_worker(|worker| worker.compile(source, flags, timeout))?;
    if matches!(reply, Reply::Compiled) && key.0.len() <= ACCEPTED_KEY_UNITS {
        let mut accepted = ACCEPTED.lock().unwrap_or_else(PoisonError::into_inner);
        let accepted = accepted.get_or_insert_with(Default::default);
        if accepted.len() >= ACCEPTED_ENTRIES {
            accepted.clear();
        }
        accepted.insert(key);
    }
    Ok(reply)
}

/// A match request, as the memo keys it.
#[derive(Clone, Hash, PartialEq, Eq)]
struct FindKey {
    source: Vec<u16>,
    flags: String,
    input: Vec<u16>,
    start: usize,
}

/// Results of recent `find` requests. Matching is a pure function of the
/// pattern, its flags, the subject and the start index, and scripts (Test262's
/// harness matrices in particular) ask the same question many times: one case
/// sent 167,000 requests of which 263 were distinct. Each request that reaches
/// the helper costs a pipe round trip (tens of microseconds on Unix, about half
/// a millisecond on Windows), so answering repeats here is what keeps such a
/// case inside its deadline on every platform.
///
/// Only answers the helper actually gave are kept: a timeout or transport
/// failure is never recorded, so a pattern that is slow only under load is not
/// remembered as slow.
#[derive(Default)]
struct FindMemo {
    results: HashMap<FindKey, Option<Match>>,
    order: VecDeque<FindKey>,
    /// UTF-16 code units held by the keys, to bound memory.
    units: usize,
}

/// Most results remembered.
const MEMO_ENTRIES: usize = 1024;
/// Most UTF-16 code units of pattern and subject text remembered across all
/// entries (4 MiB), and the most one request may contribute.
const MEMO_UNITS: usize = 2 * 1024 * 1024;
const MEMO_KEY_UNITS: usize = 16 * 1024;

impl FindMemo {
    fn remember(&mut self, key: FindKey, result: Option<Match>) {
        let size = key.source.len() + key.input.len();
        if size > MEMO_KEY_UNITS || self.results.contains_key(&key) {
            return;
        }
        while self.results.len() >= MEMO_ENTRIES || self.units + size > MEMO_UNITS {
            // Only remember mutates this private queue/map pair. Each admitted
            // key occurs once in both, and units is the sum of their sizes.
            // An empty memo plus an admitted key cannot exceed MEMO_UNITS.
            let oldest = self
                .order
                .pop_front()
                .expect("a full memo has an oldest key");
            let _ = self
                .results
                .remove(&oldest)
                .expect("the oldest memo key retains its cached result, including a miss");
            self.units -= oldest.source.len() + oldest.input.len();
        }
        self.units += size;
        self.order.push_back(key.clone());
        self.results.insert(key, result);
    }
}

static MEMO: Mutex<Option<FindMemo>> = Mutex::new(None);

pub(crate) fn find(
    source: Vec<u16>,
    flags: String,
    input: Vec<u16>,
    start: usize,
    timeout: Duration,
) -> Result<Option<Match>, RuntimeError> {
    let key = FindKey {
        source,
        flags,
        input,
        start,
    };
    if let Some(known) = MEMO
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|memo| memo.results.get(&key))
    {
        return Ok(known.clone());
    }
    let FindKey {
        source,
        flags,
        input,
        start,
    } = key.clone();
    let result = with_worker(|worker| worker.find(source, flags, input, start, timeout))?;
    MEMO.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(FindMemo::default)
        .remember(key, result.clone());
    Ok(result)
}

pub(crate) fn validate(
    patterns: Vec<(Vec<u16>, String)>,
    timeout: Duration,
) -> Result<Vec<bool>, RuntimeError> {
    with_worker(|worker| worker.validate(patterns, timeout))
}

/// Entry point for the separately installed matcher executable.
#[doc(hidden)]
pub fn serve() -> io::Result<()> {
    serve_on(&mut io::stdin().lock(), &mut io::stdout().lock())
}

fn serve_on(stream_in: &mut dyn Read, stream_out: &mut dyn Write) -> io::Result<()> {
    frame_write(stream_out, READY)?;
    let mut cached: Option<Compiled> = None;
    let mut cached_input: Option<Vec<u16>> = None;
    let mut canonical_input: Option<Vec<u16>> = None;
    loop {
        let bytes = match frame_read(stream_in) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        };
        let request: IncomingRequest = serde_json::from_slice(&bytes)?;
        let request = match request {
            IncomingRequest::Current(request) => request,
            IncomingRequest::Legacy(LegacyRequest {
                source,
                flags,
                input: Some(input),
                start,
            }) => Request::Find {
                source: Some(source),
                flags: Some(flags),
                input: Some(input),
                start,
            },
            IncomingRequest::Legacy(LegacyRequest {
                source,
                flags,
                input: None,
                ..
            }) => Request::Compile { source, flags },
        };
        let reply = match request {
            Request::Shutdown => return Ok(()),
            Request::Compile { source, flags } => match cache_pattern(&mut cached, source, flags) {
                Ok(()) => Reply::Compiled,
                Err(message) => Reply::SyntaxError(message),
            },
            Request::Find {
                source,
                flags,
                input,
                start,
            } => {
                if let Some(source) = source {
                    let flags = flags.ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "missing regex flags")
                    })?;
                    if let Err(message) = cache_pattern(&mut cached, source, flags) {
                        frame_write(stream_out, &encode_reply(&Reply::SyntaxError(message)))?;
                        continue;
                    }
                } else if flags.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "regex flags without a pattern",
                    ));
                }
                if let Some(input) = input {
                    cached_input = Some(input);
                    canonical_input = None;
                }
                let input = cached_input.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing regex subject")
                })?;
                let compiled = cached.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing regex pattern")
                })?;
                let subject: &[u16] = if compiled.canonical {
                    canonical_input.get_or_insert_with(|| {
                        crate::regex_canonicalize::canonicalize_subject(input)
                    })
                } else {
                    input
                };
                let found = if start > input.len() {
                    None
                } else if compiled.flags.contains(['u', 'v']) {
                    compiled.regex.find_from_utf16(subject, start).next()
                } else {
                    compiled.regex.find_from_ucs2(subject, start).next()
                };
                Reply::Found(found.map(|m| {
                    Match {
                        captures: m.groups().collect(),
                        names: m
                            .named_groups()
                            .map(|(name, range)| (name.to_string(), range))
                            .collect(),
                    }
                }))
            }
            Request::Validate { patterns } => Reply::Validated(
                patterns
                    .into_iter()
                    .map(|(source, flags)| cache_pattern(&mut cached, source, flags).is_ok())
                    .collect(),
            ),
        };
        frame_write(stream_out, &encode_reply(&reply))?;
    }
}

#[cfg(test)]
#[path = "../tests/fixtures/regex_framing.rs"]
mod tests;

#[cfg(any(test, coverage))]
#[path = "../tests/fixtures/regex_pool_boundaries.rs"]
mod pool_boundary_contracts;

#[cfg(coverage)]
#[doc(hidden)]
pub use pool_boundary_contracts::verify_regex_pool_boundary_contracts;

#[cfg(test)]
mod transport_failure_tests {
    use super::*;

    /// A writer that accepts `budget` bytes and then fails.
    struct Budget(usize);

    impl Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0 < bytes.len() {
                return Err(io::Error::other("out of budget"));
            }
            self.0 -= bytes.len();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut framed = Vec::new();
        frame_write(&mut framed, payload).unwrap();
        framed
    }

    #[test]
    fn a_frame_that_cannot_be_written_or_read_is_an_error() {
        assert!(frame_write(&mut Budget(0), b"x").is_err());
        assert!(frame_write(&mut Budget(4), b"x").is_err());
        assert_eq!(
            frame_read(&mut io::empty()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        // A length prefix promising more payload than the stream holds.
        let truncated = [5u8, 0, 0, 0, 1, 2];
        assert_eq!(
            frame_read(&mut truncated.as_slice()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    /// A process that exits immediately: these tests need an owned child, not
    /// a working matcher.
    fn exited_child() -> Child {
        #[cfg(windows)]
        {
            Command::new("cmd.exe")
                .args(["/C", "exit", "0"])
                .spawn()
                .unwrap()
        }

        #[cfg(not(windows))]
        {
            Command::new("true").spawn().unwrap()
        }
    }

    #[test]
    fn a_validation_over_a_closed_transport_is_a_worker_failure() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let (_reply, replies) = mpsc::channel();
        let mut worker = Worker {
            child: exited_child(),
            requests: Some(sender),
            replies,
            io_thread: None,
            failed: true,
            pattern: None,
            input: None,
        };
        assert!(worker
            .validate(Vec::new(), Duration::from_millis(50))
            .is_err());
    }

    #[test]
    fn the_worker_path_is_next_to_the_executable_or_above_its_deps_directory() {
        let suffix = format!("bluejs-regexp-worker{}", std::env::consts::EXE_SUFFIX);
        assert_eq!(
            sibling_worker_path(Ok(PathBuf::from("/build/debug/deps/test"))).unwrap(),
            PathBuf::from("/build/debug").join(&suffix)
        );
        assert_eq!(
            sibling_worker_path(Ok(PathBuf::from("/build/debug/examples/demo"))).unwrap(),
            PathBuf::from("/build/debug").join(&suffix)
        );
        assert_eq!(
            sibling_worker_path(Ok(PathBuf::from("/build/debug/app"))).unwrap(),
            PathBuf::from("/build/debug").join(&suffix)
        );
        assert_eq!(
            sibling_worker_path(Err(io::Error::other("no executable")))
                .unwrap_err()
                .to_string(),
            "no executable"
        );
    }

    #[test]
    fn the_worker_loop_reports_transport_and_request_errors() {
        // The ready frame cannot be written.
        assert!(serve_on(&mut io::empty(), &mut Budget(0)).is_err());
        // A request that is not JSON of a known shape.
        let garbage = frame(b"not json");
        assert!(serve_on(&mut garbage.as_slice(), &mut Vec::new()).is_err());
        // A find that supplies a pattern without its flags.
        let missing_flags = frame(
            &serde_json::to_vec(&Request::Find {
                source: Some(vec![97]),
                flags: None,
                input: Some(vec![97]),
                start: 0,
            })
            .unwrap(),
        );
        assert_eq!(
            serve_on(&mut missing_flags.as_slice(), &mut Vec::new())
                .unwrap_err()
                .to_string(),
            "missing regex flags"
        );
        // A syntax-error reply that cannot be written after the ready frame,
        // and an ordinary reply that cannot.
        let ready = 4 + READY.len();
        for request in [
            Request::Compile {
                source: vec![40],
                flags: String::new(),
            },
            Request::Find {
                source: Some(vec![40]),
                flags: Some(String::new()),
                input: Some(vec![97]),
                start: 0,
            },
            Request::Validate {
                patterns: vec![(vec![97], String::new())],
            },
        ] {
            let framed = frame(&serde_json::to_vec(&request).unwrap());
            assert!(serve_on(&mut framed.as_slice(), &mut Budget(ready)).is_err());
        }
    }

    /// A worker whose only reply is the given frame.
    fn worker_replying(reply: Vec<u8>) -> (Worker, Receiver<Vec<u8>>) {
        let (sender, requests) = mpsc::channel();
        let (replier, replies) = mpsc::channel();
        replier.send(Ok(reply)).unwrap();
        let worker = Worker {
            child: exited_child(),
            requests: Some(sender),
            replies,
            io_thread: None,
            failed: true,
            pattern: None,
            input: None,
        };
        (worker, requests)
    }

    #[test]
    fn zero_deadline_refuses_a_queued_reply_without_sending_work() {
        let (mut worker, requests) = worker_replying(encode_reply(&Reply::Validated(Vec::new())));
        assert_eq!(
            worker.validate(Vec::new(), Duration::ZERO).unwrap_err(),
            RuntimeError::RegexTimeout
        );
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn a_reply_is_decoded_and_its_captures_checked_after_the_transaction() {
        let ask = |reply: Vec<u8>, request: Request, input_length: Option<usize>| {
            let (mut worker, _requests) = worker_replying(reply);
            worker.request(request, Duration::from_millis(50), input_length)
        };
        let validate = || Request::Validate {
            patterns: Vec::new(),
        };
        let found = |captures: Vec<Option<Range<usize>>>| {
            encode_reply(&Reply::Found(Some(Match {
                captures,
                names: Vec::new(),
            })))
        };

        // A reply that is not a reply.
        let garbage = ask(b"not json".to_vec(), validate(), None).unwrap_err();
        assert!(garbage.to_string().contains("expected"), "{garbage}");
        // A capture range beyond the subject is refused, a valid one passed on.
        assert_eq!(
            ask(found(vec![Some(0..9)]), validate(), Some(3)).unwrap_err(),
            worker_error("invalid regex capture range".to_string())
        );
        let valid = ask(found(vec![Some(0..2)]), validate(), Some(3)).unwrap();
        assert_eq!(
            format!("{valid:?}"),
            format!("{:?}", Reply::Found(Some(Match::whole(0..2))))
        );
        // Without a subject length there is nothing to check a match against.
        let unchecked = ask(found(vec![Some(0..9)]), validate(), None).unwrap();
        assert_eq!(
            format!("{unchecked:?}"),
            format!("{:?}", Reply::Found(Some(Match::whole(0..9))))
        );
    }
}
