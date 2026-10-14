// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Launcher shutdown owns both the serving core and a staged automatic update.
//! Fixture containment runs only after the public lifetime assertions.

mod common;

use blueice_ipc::{read_client_message, write_client_message, ClientMessage};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use common::{open_credits, wait_for, Rig};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const STARTING_UPDATE: &str = r#"if [ "$n" -ge 2 ]; then
    previous=""
    for argument in "$@"; do
        if [ "$previous" = "--socket" ]; then
            printf '%s\n' "$argument" > "$DIR/pending.socket"
            break
        fi
        previous="$argument"
    done
    echo "$$" > "$DIR/pending.pid.staged"
    mv "$DIR/pending.pid.staged" "$DIR/pending.pid"
    while [ ! -e "$DIR/allow-start" ]; do sleep 0.01; done
fi
exec "$REAL" "$@""#;

fn core_pid(rig: &Rig) -> libc::pid_t {
    let mut control = UnixStream::connect(&rig.control).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_control_request(&mut control, &ControlRequest::Status).unwrap();
    let ControlReply::Status(status) = read_control_reply(&mut control).unwrap() else {
        panic!("expected launcher status");
    };
    status.core_pid.unwrap() as libc::pid_t
}

fn trigger_update(rig: &Rig) {
    let current = std::fs::read_to_string(rig.dir.join("blueice-core")).unwrap();
    let staged = rig.dir.join("blueice-core.staged");
    std::fs::write(&staged, format!("{current}\n# stable replacement build\n")).unwrap();
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(staged, rig.dir.join("blueice-core")).unwrap();
}

fn pending_pid(rig: &Rig) -> libc::pid_t {
    let marker = rig.dir.join("pending.pid");
    assert!(
        wait_for(&marker),
        "update never reached its startup barrier"
    );
    let pending = std::fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse::<libc::pid_t>()
        .unwrap();
    assert_eq!(
        unsafe { libc::getpgid(pending) },
        rig.child.id() as libc::pid_t
    );
    pending
}

fn assert_shutdown(rig: &mut Rig, owned: &[libc::pid_t]) {
    let mut client = rig.client();
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    let status = loop {
        if let Some(status) = rig.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "launcher did not finish shutdown"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "launcher shutdown failed: {status}");
    for &pid in owned {
        let result = unsafe { libc::kill(pid, 0) };
        assert_eq!(result, -1, "owned child {pid} survived launcher shutdown");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

#[test]
fn shutdown_reaps_the_unchanged_serving_core() {
    let mut rig = Rig::start_isolated(
        "shutdown-steady",
        STARTING_UPDATE,
        &["--auto-update-secs", "1"],
    );
    let serving = core_pid(&rig);
    assert_shutdown(&mut rig, &[serving]);
}

#[test]
fn shutdown_during_automatic_update_reaps_the_starting_replacement() {
    let mut rig = Rig::start_isolated(
        "shutdown-starting",
        STARTING_UPDATE,
        &["--auto-update-secs", "1"],
    );
    let mut client = rig.client();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    open_credits(&mut client);
    let serving = core_pid(&rig);
    trigger_update(&rig);
    let pending = pending_pid(&rig);
    assert_ne!(serving, pending);
    assert_eq!(
        unsafe { libc::getpgid(pending) },
        rig.child.id() as libc::pid_t
    );
    assert_eq!(
        core_pid(&rig),
        serving,
        "startup must not replace the active core"
    );
    assert_shutdown(&mut rig, &[serving, pending]);
}

#[test]
fn shutdown_during_manual_cutover_reaps_the_starting_replacement() {
    let mut rig = Rig::start_isolated("shutdown-manual", STARTING_UPDATE, &[]);
    let serving = core_pid(&rig);
    let path = rig.control.clone();
    let request = thread::spawn(move || {
        let mut control = UnixStream::connect(path).unwrap();
        control
            .set_read_timeout(Some(Duration::from_secs(12)))
            .unwrap();
        write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
        read_control_reply(&mut control)
    });
    let pending = pending_pid(&rig);
    assert_shutdown(&mut rig, &[serving, pending]);
    assert!(
        !matches!(
            request.join().unwrap(),
            Ok(ControlReply::CutoverDone { .. })
        ),
        "a closing broker must never publish its staged replacement"
    );
}

#[test]
fn shutdown_interrupts_a_staged_core_that_never_answers_hello() {
    let mut rig = Rig::start_isolated(
        "shutdown-hello",
        STARTING_UPDATE,
        &["--auto-update-secs", "1"],
    );
    let serving = core_pid(&rig);
    trigger_update(&rig);
    let pending = pending_pid(&rig);
    let socket = std::fs::read_to_string(rig.dir.join("pending.socket")).unwrap();
    let listener = UnixListener::bind(socket.trim()).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (ready, waiting) = mpsc::channel();
    let peer = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "staged core never connected");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("staged accept failed: {error}"),
            }
        };
        // Darwin inherits the listener's O_NONBLOCK flag on accept.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert!(matches!(
            read_client_message(&mut stream).unwrap(),
            ClientMessage::Hello { .. }
        ));
        ready.send(()).unwrap();
        // Deliberately withhold the Hello reply. Shutdown must interrupt I/O.
        assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_shutdown(&mut rig, &[serving, pending]);
    peer.join().unwrap();
}

#[test]
fn shutdown_after_a_committed_update_reaps_both_core_generations() {
    let mut rig = Rig::start_isolated(
        "shutdown-committed",
        "exec \"$REAL\" \"$@\"",
        &["--auto-update-secs", "1"],
    );
    let serving = core_pid(&rig);
    trigger_update(&rig);
    let deadline = Instant::now() + Duration::from_secs(15);
    let replacement = loop {
        let current = core_pid(&rig);
        if current != serving {
            break current;
        }
        assert!(Instant::now() < deadline, "update did not commit");
        thread::sleep(Duration::from_millis(20));
    };
    assert_shutdown(&mut rig, &[serving, replacement]);
}

#[test]
fn shutdown_after_broken_update_attempts_reaps_the_serving_core() {
    let script = "echo \"$$\" >> \"$DIR/attempt.pids\"\nif [ \"$n\" -ge 2 ]; then exit 1; fi\nexec \"$REAL\" \"$@\"";
    let mut rig = Rig::start_isolated("shutdown-broken", script, &["--auto-update-secs", "1"]);
    let serving = core_pid(&rig);
    trigger_update(&rig);
    let deadline = Instant::now() + Duration::from_secs(15);
    let attempts = loop {
        let contents = std::fs::read_to_string(rig.dir.join("attempt.pids")).unwrap_or_default();
        let attempts = contents
            .lines()
            .map(str::parse::<libc::pid_t>)
            .collect::<Result<Vec<_>, _>>();
        if let Ok(attempts) = attempts {
            if contents.ends_with('\n') && attempts.len() == 4 {
                break attempts;
            }
        }
        assert!(
            Instant::now() < deadline,
            "broken update retries did not finish"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(core_pid(&rig), serving);
    assert_shutdown(&mut rig, &attempts);
}

#[test]
fn shutdown_wakes_a_watcher_with_a_long_polling_interval() {
    let mut rig = Rig::start_isolated(
        "shutdown-long-poll",
        STARTING_UPDATE,
        &["--auto-update-secs", "3600"],
    );
    let serving = core_pid(&rig);
    assert_shutdown(&mut rig, &[serving]);
}
