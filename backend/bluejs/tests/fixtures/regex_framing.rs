// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
        child: Command::new("true").spawn().unwrap(),
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
        child: Command::new("true").spawn().unwrap(),
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
