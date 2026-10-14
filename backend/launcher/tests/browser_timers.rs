// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_ipc::{ClientMessage, ServerMessage};
use blueice_launcher::{CoreLaunchOptions, SpawnedCore};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            worker.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn request(stream: &mut UnixStream, id: u64, message: ClientMessage) -> ServerMessage {
    request_with_frames(stream, id, message, &mut VecDeque::new())
}

type FrameMessage = (Option<u64>, Option<u64>, ServerMessage);

fn request_with_frames(
    stream: &mut UnixStream,
    id: u64,
    message: ClientMessage,
    frames: &mut VecDeque<FrameMessage>,
) -> ServerMessage {
    request_for_tab_with_frames(stream, 1, id, message, frames)
}

fn request_for_tab_with_frames(
    stream: &mut UnixStream,
    tab: u64,
    id: u64,
    message: ClientMessage,
    frames: &mut VecDeque<FrameMessage>,
) -> ServerMessage {
    blueice_ipc::write_client_message_with_ids(stream, Some(tab), Some(id), &message).unwrap();
    loop {
        let (tab, reply_id, message) = blueice_ipc::read_server_message_with_ids(stream).unwrap();
        if reply_id == Some(id) && !matches!(message, ServerMessage::FrameReady { .. }) {
            return message;
        }
        if reply_id.is_none() && matches!(message, ServerMessage::FrameReady { .. }) {
            frames.push_back((tab, reply_id, message));
        }
    }
}

#[test]
fn scheduling_exhaustion_in_one_real_child_realm_keeps_another_tab_rendering() {
    scheduling_exhaustion_case("try { for (var i = 0; i < 1025; i += 1) { queueMicrotask(noop); } } catch (error) {} document.getElementById('counter').textContent = 'escaped';");
}

#[test]
fn cleanup_task_exhaustion_in_one_real_child_keeps_another_tab_rendering() {
    scheduling_exhaustion_case("for (var i = 0; i < 65; i += 1) { var registry = new FinalizationRegistry(noop); registries.push(registry); registry.register({}, 0); } for (var j = 0; j < 3; j += 1) { Promise.resolve('x'.repeat(400000)).then(noop); }");
}

fn scheduling_exhaustion_case(exhaustion: &str) {
    let (bad_fixture, bad_url, gatekeeper) = fixture(
        "<p id=counter>failed:0</p><script>
        var attempts = 0; var registries = [];
        function noop() {}
        function exhaust() {
            attempts += 1;
            document.getElementById('counter').textContent = 'failed:' + attempts;
            __EXHAUST__
        }
        setInterval(exhaust, 200);
    </script>"
            .replace("__EXHAUST__", exhaustion),
    );
    let (_good_fixture, good_url, _) = fixture(
        "<p id=counter>tick:0</p><script>
        var count = 0; var timer = 0;
        function tick() {
            count += 1; document.getElementById('counter').textContent = 'tick:' + count;
            if (count === 3) { clearInterval(timer); }
        }
        timer = setInterval(tick, 500);
    </script>"
            .into(),
    );
    let core = SpawnedCore::spawn_with_options(
        240.0,
        120.0,
        &bad_fixture.root.join("frames"),
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper)
            .supervise_out_of_process_bluejs_with_page_dom(),
    )
    .unwrap();
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut frames = VecDeque::new();
    assert!(matches!(
        request_with_frames(
            &mut stream,
            1,
            ClientMessage::Navigate {
                url: bad_url.clone()
            },
            &mut frames
        ),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::TabOpened {
        tab_id: good_tab, ..
    } = request_with_frames(
        &mut stream,
        2,
        ClientMessage::OpenTab {
            url: Some(good_url),
        },
        &mut frames,
    )
    else {
        panic!("healthy tab must open");
    };
    let mut previous_generation = 0;
    for count in 1..=3 {
        let generation = loop {
            let (tab, id, message) = frames
                .pop_front()
                .unwrap_or_else(|| blueice_ipc::read_server_message_with_ids(&mut stream).unwrap());
            if let ServerMessage::FrameReady { generation, .. } = message {
                if tab == Some(good_tab) && id.is_none() {
                    break generation;
                }
            }
        };
        assert!(generation > previous_generation);
        previous_generation = generation;
        let ServerMessage::Representation(snapshot) = request_for_tab_with_frames(
            &mut stream,
            good_tab,
            10 + count,
            ClientMessage::GetRepresentation,
            &mut frames,
        ) else {
            panic!("healthy tab must remain observable");
        };
        assert_eq!(snapshot.generation, generation);
        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some(&format!("tick:{count}"))));
    }
    let ServerMessage::Representation(failed) = request_with_frames(
        &mut stream,
        20,
        ClientMessage::GetRepresentation,
        &mut frames,
    ) else {
        panic!("failed page keeps its last DOM snapshot");
    };
    assert!(
        failed
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("failed:1")),
        "{failed:?}"
    );
    assert!(!failed
        .nodes
        .iter()
        .any(|node| node.name.as_deref() == Some("escaped")));
    assert!(matches!(
        request_with_frames(
            &mut stream,
            21,
            ClientMessage::Navigate {
                url: bad_url.replace("/counter", "/replacement")
            },
            &mut frames
        ),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::Representation(replacement) = request_with_frames(
        &mut stream,
        22,
        ClientMessage::GetRepresentation,
        &mut frames,
    ) else {
        panic!("failed tab must accept a successor document");
    };
    assert!(replacement
        .nodes
        .iter()
        .any(|node| node.name.as_deref() == Some("replacement")));
}

fn fixture(body: String) -> (Fixture, String, PathBuf) {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let root = PathBuf::from("/tmp").join(format!(
        "bi-timers-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let mut fixture = Fixture {
        root,
        stop: Arc::new(AtomicBool::new(false)),
        workers: vec![],
    };
    let gatekeeper_path = fixture.root.join("gatekeeper.sock");
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    gatekeeper.set_nonblocking(true).unwrap();
    let stop = fixture.stop.clone();
    fixture.workers.push(thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            match gatekeeper.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("{error}"),
            }
        }
    }));
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let url = format!("http://{}/counter", server.local_addr().unwrap());
    let stop = fixture.stop.clone();
    fixture.workers.push(thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            match server.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).unwrap_or(0);
                    let reply = if String::from_utf8_lossy(&request[..read]).contains("/replacement") {
                        "<p>replacement</p>"
                    } else { &body };
                    if read != 0 {
                        let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).as_bytes());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("{error}"),
            }
        }
    }));
    (fixture, url, gatekeeper_path)
}

#[test]
fn javascript_and_bluets_ready_timers_offer_a_frame_between_tasks() {
    counter_frames("let count: number = 0;
        function publish(): void { document.getElementById('counter')!.textContent = 'tick:' + count; }
        function tick(): void { count += 1; queueMicrotask(publish); }
        function arm(): void { setTimeout(tick, 0); setTimeout(tick, 0); setTimeout(tick, 0); }
        setTimeout(arm, 500);", true);
}

#[test]
fn javascript_and_bluets_abort_timeouts_share_timer_source_order_and_frames() {
    counter_frames("let count: number = 0;
        function publish(): void { document.getElementById('counter')!.textContent = 'tick:' + count; }
        function tick(expected: number): void {
            if (count !== expected) { throw 'timer source reordered'; }
            count += 1; queueMicrotask(publish);
        }
        function first(): void { tick(0); }
        function middle(): void { tick(1); }
        function last(): void { tick(2); }
        function arm(): void {
            setTimeout(first, 0);
            const signal = AbortSignal.timeout(0);
            signal.onabort = middle;
            setTimeout(last, 0);
        }
        setTimeout(arm, 500);", true);
}

#[test]
fn javascript_and_bluets_intervals_publish_matching_frames_and_representations() {
    counter_frames("let count: number = 0; let timer: number = 0;
        function publish(): void { document.getElementById('counter')!.textContent = 'tick:' + count; }
        function tick(): void { count += 1; queueMicrotask(publish); if (count === 3) { clearInterval(timer); } }
        timer = setInterval(tick, 500);", true);
}

#[test]
fn javascript_and_bluets_cleanup_tasks_publish_post_microtask_frames() {
    counter_frames("let count: number = 0; let timer: number = 0;
        function publish(): void { document.getElementById('counter')!.textContent = 'tick:' + count; }
        async function pressure() { return 'x'.repeat(400000); }
        function cleanup(): void {
            count += 1; queueMicrotask(publish);
            if (count === 3) { clearInterval(timer); }
        }
        const registry = new FinalizationRegistry(cleanup);
        function poll(): void {
            registry.register({}, 0);
            pressure(); pressure(); pressure();
        }
        timer = setInterval(poll, 500);", true);
}

#[test]
fn javascript_and_bluets_animation_frames_publish_post_microtask_representations() {
    counter_frames("let count: number = 0; let last: number = 0;
        function publish(): void { document.getElementById('counter')!.textContent = 'tick:' + count; }
        function refused(timestamp: number): void { throw 'cancelled frame callback'; }
        function arm(): void { requestAnimationFrame(tick); }
        function tick(timestamp: number): void {
            if (!(timestamp > last)) { throw 'frame timestamp must advance'; }
            last = timestamp; count += 1; queueMicrotask(publish);
            if (count < 3) { setTimeout(arm, 500); }
        }
        const cancelled: number = requestAnimationFrame(refused);
        cancelAnimationFrame(cancelled); setTimeout(arm, 100);", true);
}

#[test]
fn a_frame_callback_can_keep_rendering_without_further_timer_or_frontend_work() {
    counter_frames("let count: number = 0; let last: number = 0;
        function publish(): void { document.getElementById('counter')!.textContent = 'tick:' + count; }
        function arm(): void { requestAnimationFrame(tick); }
        function tick(timestamp: number): void {
            if (!(timestamp > last)) { throw 'frame timestamp must advance'; }
            last = timestamp; count += 1; queueMicrotask(publish);
            if (count < 3) { requestAnimationFrame(tick); }
        }
        setTimeout(arm, 100);", false);
}

fn counter_frames(source: &str, check_every_representation: bool) {
    for typed in [false, true] {
        let source = if typed {
            source.to_string()
        } else {
            source
                .replace(": number", "")
                .replace(": void", "")
                .replace("('counter')!", "('counter')")
        };
        let kind = if typed {
            " type='application/x-blueice-typescript'"
        } else {
            ""
        };
        let (fixture, url, gatekeeper) = fixture(format!(
            "<p id=counter>tick:0</p><script{kind}>{source}</script>"
        ));
        let core = SpawnedCore::spawn_with_options(
            240.0,
            120.0,
            &fixture.root.join("frames"),
            CoreLaunchOptions::default()
                .with_gatekeeper_socket(gatekeeper)
                .supervise_out_of_process_bluejs_with_page_dom(),
        )
        .unwrap();
        let mut stream = core.stream.try_clone().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut frames = VecDeque::new();
        assert!(matches!(
            request_with_frames(&mut stream, 1, ClientMessage::Navigate { url }, &mut frames),
            ServerMessage::Navigated { .. }
        ));
        let reports = request_with_frames(
            &mut stream,
            2,
            if typed {
                ClientMessage::GetBlueTsScriptReports
            } else {
                ClientMessage::GetBlueJsScriptReports
            },
            &mut frames,
        );
        match reports {
            ServerMessage::BlueJsScriptReports(reports) => assert!(
                reports.len() == 1
                    && reports.iter().all(|report| matches!(
                        report.outcome,
                        blueice_ipc::BlueJsScriptExecutionOutcome::Executed
                    )),
                "{reports:?}"
            ),
            ServerMessage::BlueTsScriptReports(reports) => assert!(
                reports.len() == 1
                    && reports.iter().all(|report| matches!(
                        report.outcome,
                        blueice_ipc::BlueTsScriptExecutionOutcome::Executed
                    )),
                "{reports:?}"
            ),
            other => panic!("expected script reports: {other:?}"),
        }
        let mut previous_generation = 0;
        let mut previous_pixels = Vec::new();
        for count in 1..=3 {
            let (tab, request_id, frame) = frames
                .pop_front()
                .unwrap_or_else(|| blueice_ipc::read_server_message_with_ids(&mut stream).unwrap());
            let ServerMessage::FrameReady {
                generation,
                shm_path,
                ..
            } = frame
            else {
                panic!("expected scheduled frame: {frame:?}");
            };
            assert_eq!(tab, Some(1));
            assert_eq!(
                request_id, None,
                "scheduled frames arrive without a frontend request"
            );
            assert!(generation > previous_generation);
            let pixels = std::fs::read(shm_path).unwrap();
            assert!(!pixels.is_empty());
            assert_ne!(
                pixels, previous_pixels,
                "a timer must paint its new counter"
            );
            if check_every_representation || count == 3 {
                let ServerMessage::Representation(snapshot) = request_with_frames(
                    &mut stream,
                    10 + count,
                    ClientMessage::GetRepresentation,
                    &mut frames,
                ) else {
                    panic!("expected Representation");
                };
                assert_eq!(snapshot.generation, generation);
                assert!(
                    snapshot
                        .nodes
                        .iter()
                        .any(|node| node.name.as_deref() == Some(&format!("tick:{count}"))),
                    "{snapshot:?}"
                );
            }
            previous_generation = generation;
            previous_pixels = pixels;
        }
        stream
            .set_read_timeout(Some(Duration::from_millis(650)))
            .unwrap();
        let error = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "the completed counter must stop publishing: {error}"
        );
        drop(stream);
        drop(core);
    }
}

#[test]
fn navigating_away_from_a_running_interval_keeps_the_successor_quiet() {
    let (fixture, url, gatekeeper) = fixture(
        "<p id=counter>tick:0</p><script>
        var count = 0; function tick() {
            count += 1; document.getElementById('counter').textContent = 'tick:' + count;
        } setInterval(tick, 500);
    </script>"
            .into(),
    );
    let replacement = url.replace("/counter", "/replacement");
    let core = SpawnedCore::spawn_with_options(
        240.0,
        120.0,
        &fixture.root.join("frames"),
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper)
            .supervise_out_of_process_bluejs_with_page_dom(),
    )
    .unwrap();
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    assert!(matches!(
        request(&mut stream, 1, ClientMessage::Navigate { url }),
        ServerMessage::Navigated { .. }
    ));
    loop {
        let (_, id, frame) = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));
        if id.is_none() {
            break;
        }
        assert_eq!(
            id,
            Some(1),
            "only the initial navigation frame may precede the timer"
        );
    }
    assert!(matches!(
        request(&mut stream, 2, ClientMessage::Navigate { url: replacement }),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::Representation(snapshot) =
        request(&mut stream, 3, ClientMessage::GetRepresentation)
    else {
        panic!("expected successor Representation");
    };
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("replacement")),
        "{snapshot:?}"
    );
    stream
        .set_read_timeout(Some(Duration::from_millis(650)))
        .unwrap();
    let error = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap_err();
    assert!(
        matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
        "successor must not publish predecessor timer work: {error}"
    );
    drop(stream);
    drop(core);
}
