// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Process-isolated regular expressions. The parent never executes regress.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::RuntimeError;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_FRAME: usize = 16 * 1024 * 1024;
const READY: &[u8] = b"bluejs-regexp-worker/1";

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Request {
    // Kept for direct worker clients from protocol version 1. New parent
    // requests use the cache-aware variants below.
    Legacy {
        source: Vec<u16>,
        flags: String,
        input: Option<Vec<u16>>,
        start: usize,
    },
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
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Reply {
    Compiled,
    Found(Option<Match>),
    SyntaxError(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Match {
    captures: Vec<Option<Range<usize>>>,
    names: Vec<(String, Option<Range<usize>>)>,
}

impl Match {
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

impl Worker {
    fn start() -> io::Result<Self> {
        let path = match std::env::var_os("BLUEJS_REGEXP_WORKER") {
            Some(path) => PathBuf::from(path),
            None => {
                let executable = std::env::current_exe()?;
                let mut directory = executable.parent().unwrap();
                if directory
                    .file_name()
                    .is_some_and(|name| name == "deps" || name == "examples")
                {
                    directory = directory.parent().unwrap();
                }
                directory.join(format!(
                    "bluejs-regexp-worker{}",
                    std::env::consts::EXE_SUFFIX
                ))
            }
        };
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
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
        match worker.replies.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(ready)) if ready == READY => Ok(worker),
            _ => {
                worker.failed = true;
                Err(io::Error::other(
                    "regex worker startup failed or exceeded two seconds",
                ))
            }
        }
    }

    fn transact(&mut self, bytes: Vec<u8>, timeout: Duration) -> Result<Vec<u8>, RuntimeError> {
        self.requests
            .as_ref()
            .unwrap()
            .send(bytes)
            .map_err(worker_error)?;
        self.replies
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => RuntimeError::RegexTimeout,
                error => worker_error(error),
            })?
            .map_err(worker_error)
    }

    fn request(
        &mut self,
        request: Request,
        timeout: Duration,
        input_length: Option<usize>,
    ) -> Result<Reply, RuntimeError> {
        let bytes = serde_json::to_vec(&request).map_err(worker_error)?;
        if bytes.len() > MAX_FRAME {
            return Err(worker_error("regex request exceeds frame limit"));
        }
        let bytes = self.transact(bytes, timeout)?;
        let reply: Reply = serde_json::from_slice(&bytes).map_err(worker_error)?;
        if input_length.is_some_and(
            |length| matches!(&reply, Reply::Found(Some(matched)) if !matched.valid(length)),
        ) {
            return Err(worker_error("invalid regex capture range"));
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
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Kill before joining on a failed transaction: the IO thread only
        // accesses pipes owned by this child, so termination unblocks it.
        if self.failed {
            let _ = self.child.kill();
        }
        self.requests.take();
        if let Some(thread) = self.io_thread.take() {
            let _ = thread.join();
        }
        let _ = self.child.wait();
    }
}

fn worker_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::RegexWorker(error.to_string())
}

fn cache_pattern(
    cached: &mut Option<(Vec<u16>, String, regress::Regex)>,
    source: Vec<u16>,
    flags: String,
) -> Result<(), String> {
    let same = cached
        .as_ref()
        .is_some_and(|(old_source, old_flags, _)| *old_source == source && *old_flags == flags);
    if same {
        return Ok(());
    }
    let unicode = flags.contains(['u', 'v']);
    let points: Vec<u32> = if unicode {
        char::decode_utf16(source.iter().copied())
            .map(|c| c.map_or_else(|e| u32::from(e.unpaired_surrogate()), |c| c as u32))
            .collect()
    } else {
        source.iter().map(|&c| u32::from(c)).collect()
    };
    let regex =
        regress::Regex::from_unicode(points.into_iter(), regress::Flags::from(flags.as_str()))
            .map_err(|error| error.to_string())?;
    *cached = Some((source, flags, regex));
    Ok(())
}

thread_local! { static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) }; }

fn with_worker<T>(
    operation: impl FnOnce(&mut Worker) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Worker::start().map_err(worker_error)?);
        }
        let result = operation(slot.as_mut().unwrap());
        if result.is_err() {
            let mut worker = slot.take().unwrap();
            worker.failed = true;
        }
        result
    })
}

pub(crate) fn compile(
    source: Vec<u16>,
    flags: String,
    timeout: Duration,
) -> Result<Reply, RuntimeError> {
    with_worker(|worker| worker.compile(source, flags, timeout))
}

pub(crate) fn find(
    source: Vec<u16>,
    flags: String,
    input: Vec<u16>,
    start: usize,
    timeout: Duration,
) -> Result<Option<Match>, RuntimeError> {
    with_worker(|worker| worker.find(source, flags, input, start, timeout))
}

/// Entry point for the separately installed matcher executable.
#[doc(hidden)]
pub fn serve() -> io::Result<()> {
    let (mut input, mut output) = (io::stdin().lock(), io::stdout().lock());
    frame_write(&mut output, READY)?;
    let mut cached: Option<(Vec<u16>, String, regress::Regex)> = None;
    let mut cached_input = None;
    loop {
        let bytes = match frame_read(&mut input) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        };
        let request: Request = serde_json::from_slice(&bytes)?;
        let request = match request {
            Request::Legacy {
                source,
                flags,
                input: Some(input),
                start,
            } => Request::Find {
                source: Some(source),
                flags: Some(flags),
                input: Some(input),
                start,
            },
            Request::Legacy {
                source,
                flags,
                input: None,
                ..
            } => Request::Compile { source, flags },
            request => request,
        };
        let reply = match request {
            Request::Legacy { .. } => unreachable!("legacy requests are normalized above"),
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
                        frame_write(
                            &mut output,
                            &serde_json::to_vec(&Reply::SyntaxError(message))?,
                        )?;
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
                }
                let input = cached_input.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing regex subject")
                })?;
                let (_, flags, regex) = cached.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing regex pattern")
                })?;
                let found = if start > input.len() {
                    None
                } else if flags.contains(['u', 'v']) {
                    regex.find_from_utf16(input, start).next()
                } else {
                    regex.find_from_ucs2(input, start).next()
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
        };
        frame_write(&mut output, &serde_json::to_vec(&reply)?)?;
    }
}

#[cfg(test)]
#[path = "../tests/fixtures/regex_framing.rs"]
mod tests;
