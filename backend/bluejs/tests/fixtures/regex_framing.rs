// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Starts a process which exits immediately.  These transport-failure tests
/// only need an owned [`Child`], not a shell or a POSIX utility.
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
fn oversized_outgoing_protocol_frames_are_rejected_before_writing() {
    let mut output = Vec::new();
    assert_eq!(
        frame_write(&mut output, &vec![0; MAX_FRAME + 1])
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert!(output.is_empty());
}

#[test]
fn disconnected_transport_is_a_worker_failure_not_a_timeout() {
    let (sender, _receiver) = mpsc::channel();
    let (reply, replies) = mpsc::channel();
    drop(reply);
    let mut worker = Worker {
        child: exited_child(),
        requests: Some(sender),
        replies,
        io_thread: None,
        failed: true,
        pattern: None,
        input: None,
    };
    assert!(matches!(
        worker.transact(vec![], Duration::from_millis(100)),
        Err(RuntimeError::RegexWorker(_))
    ));
}

#[test]
fn closed_request_channel_fails_before_waiting() {
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
    assert!(matches!(
        worker.transact(vec![], Duration::from_millis(100)),
        Err(RuntimeError::RegexWorker(_))
    ));
}

#[test]
fn timed_out_transport_releases_the_cached_process() {
    assert!(matches!(
        super::find(
            "(a+)+$".encode_utf16().collect(),
            String::new(),
            format!("{}!", "a".repeat(40)).encode_utf16().collect(),
            0,
            Duration::from_millis(40),
        ),
        Err(RuntimeError::RegexTimeout)
    ));
    assert!(matches!(
        super::find(vec![97], String::new(), vec![97], 0, DEFAULT_TIMEOUT),
        Ok(Some(_))
    ));
}

#[test]
fn cached_find_with_a_replaced_pattern_is_not_decoded_as_compile() {
    let subject: Vec<u16> = "abc".encode_utf16().collect();
    let first: Vec<u16> = "a".encode_utf16().collect();
    let second: Vec<u16> = "b".encode_utf16().collect();

    assert!(matches!(
        super::compile(first.clone(), String::new(), DEFAULT_TIMEOUT),
        Ok(Reply::Compiled)
    ));
    assert!(matches!(
        super::find(
            first.clone(),
            String::new(),
            subject.clone(),
            0,
            DEFAULT_TIMEOUT
        ),
        Ok(Some(_))
    ));
    assert!(matches!(
        super::compile(second, String::new(), DEFAULT_TIMEOUT),
        Ok(Reply::Compiled)
    ));

    // This reuses the parent-side cached subject while replacing the pattern.
    // The request must still be interpreted as Find by the worker.
    assert!(matches!(
        super::find(first, String::new(), subject, 0, DEFAULT_TIMEOUT),
        Ok(Some(_))
    ));
}

/// A worker whose transport is two in-memory channels, so a test controls
/// exactly what "the worker" replies. `_requests` keeps the request side open.
fn scripted_worker(replies: Vec<Reply>) -> (Worker, Receiver<Vec<u8>>) {
    let (sender, requests) = mpsc::channel();
    let (reply, incoming) = mpsc::channel();
    for scripted in replies {
        reply
            .send(Ok(serde_json::to_vec(&scripted).unwrap()))
            .unwrap();
    }
    let worker = Worker {
        child: exited_child(),
        requests: Some(sender),
        replies: incoming,
        io_thread: None,
        failed: false,
        pattern: None,
        input: None,
    };
    (worker, requests)
}

#[test]
fn a_reply_with_a_capture_outside_the_subject_is_a_worker_failure() {
    let out_of_range = Reply::Found(Some(Match::whole(0..9)));
    let (mut worker, _requests) = scripted_worker(vec![out_of_range]);
    let find = Request::Find {
        source: None,
        flags: None,
        input: None,
        start: 0,
    };
    assert!(matches!(
        worker.request(find, Duration::from_millis(100), Some(3)),
        Err(RuntimeError::RegexWorker(message)) if message == "invalid regex capture range"
    ));
}

#[test]
fn validation_expects_a_validated_reply() {
    let (mut worker, _requests) = scripted_worker(vec![Reply::Compiled]);
    assert!(matches!(
        worker.validate(vec![(vec![97], String::new())], Duration::from_millis(100)),
        Err(RuntimeError::RegexWorker(message)) if message == "unexpected validation reply"
    ));
}

#[test]
fn a_worker_whose_request_channel_is_already_gone_is_still_reaped_on_drop() {
    let (_reply, incoming) = mpsc::channel();
    let worker = Worker {
        child: exited_child(),
        requests: None,
        replies: incoming,
        io_thread: None,
        failed: false,
        pattern: None,
        input: None,
    };
    drop(worker);
}

/// Runs the worker loop over `requests` (each a JSON value) and returns how it
/// ended plus every reply frame it wrote after its ready frame.
fn serve_requests(requests: &[serde_json::Value]) -> (io::Result<()>, Vec<serde_json::Value>) {
    let mut input = Vec::new();
    for request in requests {
        frame_write(&mut input, &serde_json::to_vec(request).unwrap()).unwrap();
    }
    let mut output = Vec::new();
    let ended = serve_on(&mut io::Cursor::new(input), &mut output);
    let mut frames = io::Cursor::new(output);
    assert_eq!(frame_read(&mut frames).unwrap(), READY);
    let mut replies = Vec::new();
    while let Ok(bytes) = frame_read(&mut frames) {
        replies.push(serde_json::from_slice(&bytes).unwrap());
    }
    (ended, replies)
}

fn invalid_data(ended: io::Result<()>) -> String {
    let error = ended.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    error.to_string()
}

#[test]
fn the_worker_answers_the_legacy_request_shape() {
    let (ended, replies) = serve_requests(&[
        // No subject: compile only.
        serde_json::json!({"source": [97], "flags": "", "input": null, "start": 0}),
        // With a subject: find.
        serde_json::json!({"source": [97], "flags": "", "input": [98, 97], "start": 0}),
    ]);
    assert!(ended.is_ok(), "end of input ends the loop cleanly");
    assert_eq!(replies[0], serde_json::json!("Compiled"));
    assert_eq!(
        replies[1]["Found"]["captures"],
        serde_json::json!([{"start": 1, "end": 2}])
    );
}

#[test]
fn a_find_whose_pattern_does_not_compile_is_answered_and_the_loop_continues() {
    let (ended, replies) = serve_requests(&[
        serde_json::json!({"operation": "find", "source": [40], "flags": "", "input": [97], "start": 0}),
        serde_json::json!({"operation": "shutdown"}),
    ]);
    assert!(ended.is_ok());
    assert_eq!(replies.len(), 1);
    assert!(replies[0]["SyntaxError"].is_string(), "{replies:?}");
}

#[test]
fn a_malformed_find_ends_the_worker_with_the_reason() {
    let missing_flags = serve_requests(&[
        serde_json::json!({"operation": "find", "source": [97], "flags": null, "input": [97], "start": 0}),
    ]);
    assert_eq!(invalid_data(missing_flags.0), "missing regex flags");

    let flags_without_pattern = serve_requests(&[
        serde_json::json!({"operation": "find", "source": null, "flags": "i", "input": [97], "start": 0}),
    ]);
    assert_eq!(
        invalid_data(flags_without_pattern.0),
        "regex flags without a pattern"
    );

    let no_subject_yet = serve_requests(&[
        serde_json::json!({"operation": "compile", "source": [97], "flags": ""}),
        serde_json::json!({"operation": "find", "source": null, "flags": null, "input": null, "start": 0}),
    ]);
    assert_eq!(invalid_data(no_subject_yet.0), "missing regex subject");

    let no_pattern_yet = serve_requests(&[
        serde_json::json!({"operation": "find", "source": null, "flags": null, "input": [97], "start": 0}),
    ]);
    assert_eq!(invalid_data(no_pattern_yet.0), "missing regex pattern");
}

#[test]
fn a_request_larger_than_a_frame_is_refused_before_it_is_sent() {
    let (mut worker, _requests) = scripted_worker(Vec::new());
    // Each `0` serializes to two bytes of JSON, so this cannot fit in one frame.
    let find = Request::Find {
        source: None,
        flags: None,
        input: Some(vec![0; MAX_FRAME / 2 + 1]),
        start: 0,
    };
    assert!(matches!(
        worker.request(find, Duration::from_millis(100), None),
        Err(RuntimeError::RegexWorker(message)) if message == "regex request exceeds frame limit"
    ));
}

#[test]
fn a_validated_reply_forgets_the_workers_cached_pattern_and_subject() {
    let (mut worker, _requests) = scripted_worker(vec![Reply::Validated(vec![true, false])]);
    worker.pattern = Some((vec![97], String::new()));
    worker.input = Some(vec![97]);
    let checked = worker.validate(
        vec![(vec![97], String::new()), (vec![40], String::new())],
        Duration::from_millis(100),
    );
    assert_eq!(checked.unwrap(), vec![true, false]);
    assert!(worker.pattern.is_none() && worker.input.is_none());
}

/// A sink that accepts `budget` bytes and then fails every write.
struct FailingWriter {
    budget: usize,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.budget == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let taken = bytes.len().min(self.budget);
        self.budget -= taken;
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_worker_ends_when_a_frame_cannot_be_read_or_its_answer_cannot_be_written() {
    // A length prefix beyond the frame limit is a read error, not end of input.
    let oversized = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
    let ended = serve_on(&mut io::Cursor::new(oversized), &mut Vec::new());
    assert_eq!(ended.unwrap_err().kind(), io::ErrorKind::InvalidData);

    // The ready frame goes out, then the answer to a bad pattern cannot.
    let mut input = Vec::new();
    let find = serde_json::json!({
        "operation": "find", "source": [40], "flags": "", "input": [97], "start": 0
    });
    frame_write(&mut input, &serde_json::to_vec(&find).unwrap()).unwrap();
    let mut sink = FailingWriter {
        budget: 4 + READY.len(),
    };
    let ended = serve_on(&mut io::Cursor::new(input), &mut sink);
    assert_eq!(ended.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn the_worker_answers_compile_find_and_validate_requests() {
    let (ended, replies) = serve_requests(&[
        serde_json::json!({"operation": "compile", "source": [40], "flags": ""}),
        // A case-insensitive pattern without `u` is matched in canonical form.
        serde_json::json!({"operation": "find", "source": [97], "flags": "i", "input": [65], "start": 0}),
        // A start past the end of the subject finds nothing.
        serde_json::json!({"operation": "find", "source": [97], "flags": "", "input": [97], "start": 5}),
        // With `u` the subject is read as UTF-16 code points.
        serde_json::json!({"operation": "find", "source": [97], "flags": "u", "input": [97], "start": 0}),
        serde_json::json!({"operation": "validate", "patterns": [[[97], ""], [[40], ""]]}),
    ]);
    assert!(ended.is_ok());
    assert!(replies[0]["SyntaxError"].is_string(), "{replies:?}");
    let whole = serde_json::json!([{"start": 0, "end": 1}]);
    assert_eq!(replies[1]["Found"]["captures"], whole);
    assert_eq!(replies[2], serde_json::json!({"Found": null}));
    assert_eq!(replies[3]["Found"]["captures"], whole);
    assert_eq!(replies[4], serde_json::json!({"Validated": [true, false]}));
}
