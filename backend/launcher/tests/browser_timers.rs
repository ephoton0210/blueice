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

#[test]
fn unhandled_promise_rejections_reach_the_real_page_before_a_later_timer() {
    let (fixture, url, gatekeeper) = fixture(
        "<p id=counter>waiting</p><script>
        var observed='unreported';
        window.onunhandledrejection=function(event){observed=event.reason;event.preventDefault();};
        Promise.reject('probe');
        setTimeout(function(){document.getElementById('counter').textContent='result:'+observed;},150);
        </script>".to_string(),
    );
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
    loop {
        let (_, id, message) = frames
            .pop_front()
            .unwrap_or_else(|| blueice_ipc::read_server_message_with_ids(&mut stream).unwrap());
        if id.is_none() && matches!(message, ServerMessage::FrameReady { .. }) {
            let ServerMessage::Representation(snapshot) = request_with_frames(
                &mut stream,
                2,
                ClientMessage::GetRepresentation,
                &mut frames,
            ) else {
                panic!("expected page representation");
            };
            if let Some(result) = snapshot
                .nodes
                .iter()
                .filter_map(|node| node.name.as_deref())
                .find(|text| text.starts_with("result:"))
            {
                assert_eq!(result, "result:probe");
                break;
            }
        }
    }
}

#[test]
fn reported_page_errors_reach_window_onerror_through_the_real_child() {
    let (fixture, url, gatekeeper) = fixture(
        "<p id=counter>unreported</p><script>
        window.onerror = function(message, filename, line, column, error) {
            document.getElementById('counter').textContent = 'reported:' + error.message;
            return true;
        };
        reportError(new Error('probe'));
        </script>"
            .to_string(),
    );
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
    let ServerMessage::Representation(snapshot) =
        request(&mut stream, 2, ClientMessage::GetRepresentation)
    else {
        panic!("expected page representation");
    };
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("reported:probe")),
        "{snapshot:?}"
    );
}

fn observed_error_source_location(script: &str) -> String {
    observed_script_error_source_location(script, "", false)
}

fn observed_script_error_source_location(script: &str, script_type: &str, emitted: bool) -> String {
    let (fixture, url, gatekeeper) = fixture(format!(
        "<p id=counter>waiting</p><script>
        window.onerror=function(message,filename,line,column,error){{
            document.getElementById('counter').textContent='location:'+filename+':'+line+':'+column;
            return true;
        }};
        </script><script type='{script_type}'>{script}</script>"
    ));
    let options = CoreLaunchOptions::default()
        .with_gatekeeper_socket(gatekeeper)
        .supervise_out_of_process_bluejs_with_page_dom();
    let options = if emitted {
        options
            .supervise_out_of_process_bluejs_with_emitted_blue_ts_classic()
            .supervise_out_of_process_bluejs_with_emitted_blue_ts_module()
    } else {
        options
    };
    let core = SpawnedCore::spawn_with_options(240.0, 120.0, &fixture.root.join("frames"), options)
        .unwrap();
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    assert!(matches!(
        request(&mut stream, 1, ClientMessage::Navigate { url }),
        ServerMessage::Navigated { .. }
    ));
    for request_id in 2..102 {
        let ServerMessage::Representation(snapshot) =
            request(&mut stream, request_id, ClientMessage::GetRepresentation)
        else {
            panic!("expected page representation");
        };
        if let Some(location) = snapshot
            .nodes
            .iter()
            .filter_map(|node| node.name.as_deref())
            .find(|name| name.starts_with("location:"))
        {
            return location.to_owned();
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the global error handler must publish coordinates: type={script_type:?}, source={script:?}");
}

#[test]
fn real_pages_report_original_direct_bluets_statement_and_call_sources() {
    for (script_type, source, coordinates) in [
        (
            "application/x-blueice-typescript",
            "function fail(): void {\n throw 'direct';\n}fail();",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "class C {get value(): number {\n throw 'direct';\n}}new C().value;",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "window.reportError('direct');",
            "1:1",
        ),
        (
            "application/x-blueice-typescript-module",
            "export function fail(): void {\n throw 'direct-module';\n}fail();",
            "2:2",
        ),
        (
            "application/x-blueice-typescript-module",
            "await 0;\nexport function fail(): void {\n throw 'direct-await';\n}fail();",
            "3:2",
        ),
        (
            "application/x-blueice-typescript",
            "const absent: any=null;const value: any=1 + (\n absent.field);",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "function read(value: any): void {}const absent: any=null;read(\n absent.field);",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const absent: any=null;const value: any={[\n absent.field]:1};",
            "2:2",
        ),
        (
            "application/x-blueice-typescript-module",
            "await 0;const absent: any=null;export const value: any=0 + (\n absent.field);",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const absent: any=null;const value: any=`value:${\n absent.field}`;",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const C=class {get value(): number {\n throw 'getter';}};new C().value;",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const C=class {constructor(public value: number) {\n throw 'constructor';}};new C(0);",
            "2:2",
        ),
        (
            "application/x-blueice-typescript-module",
            "await 0;const absent: any=null;export const value: any=`value:${\n absent.field}`;",
            "2:2",
        ),
        (
            "application/x-blueice-typescript-module",
            "await 0;export const C=class {go(): void {\n throw 'module-class';}};new C().go();",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const absent: any=null;class C {[\n absent.field]: any=0;}",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const absent: any=null;class C {[\n absent.field](): void {}}",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const bad: any=0;\n@bad class C {}",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const Native: any=WeakMap;\n@Native class C {}",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "namespace N {export function fail(): void {\n throw 'namespace-page';}}N.fail();",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "const absent: any=null;enum E {A=(\n absent.field)}",
            "2:2",
        ),
        (
            "application/x-blueice-typescript-module",
            "await 0;export namespace N {export function fail(): void {\n throw 'namespace-module';}}N.fail();",
            "2:2",
        ),
        (
            "application/x-blueice-typescript",
            "class Handler {get apply(): any {try{throw 'caught';}catch(error){}return 0;}}const Factory: any=(globalThis as any).Proxy;const dec: any=new Factory(WeakMap,new Handler());\n@dec class C {}",
            "2:2",
        ),
    ] {
        let location = observed_script_error_source_location(source, script_type, false);
        assert!(
            location.starts_with("location:blueice://page/tab-1/document-")
                && location.ends_with(&format!("/inline-1.ts:{coordinates}")),
            "{source}: {location}"
        );
    }
}

#[test]
fn real_pages_report_original_emitted_bluets_and_javascript_module_positions() {
    for (script_type, source, coordinates, extension, emitted) in [
        (
            "application/x-blueice-typescript",
            "function fail(): void {\n throw 'typed';\n}fail();",
            "2:2",
            "ts",
            true,
        ),
        (
            "application/x-blueice-typescript",
            "class C {get value(): number {\n throw 'typed';\n}}new C().value;",
            "2:2",
            "ts",
            true,
        ),
        (
            "application/x-blueice-typescript",
            "const face: string='😀';\r\nfunction fail(): void {\r\n throw 'typed';\r\n}fail();",
            "3:2",
            "ts",
            true,
        ),
        (
            "application/x-blueice-typescript-module",
            "export function fail(): void {\n throw 'typed-module';\n}fail();",
            "2:2",
            "ts",
            true,
        ),
        (
            "application/x-blueice-typescript-module",
            "await 0;\nexport function fail(): void {\n throw 'awaited-module';\n}fail();",
            "3:2",
            "ts",
            true,
        ),
        (
            "module",
            "export function fail(){\n throw 'js-module';\n}fail();",
            "2:2",
            "js",
            false,
        ),
    ] {
        let location = observed_script_error_source_location(source, script_type, emitted);
        assert!(
            location.starts_with("location:blueice://page/tab-1/document-")
                && location.ends_with(&format!("/inline-1.{extension}:{coordinates}")),
            "{script_type}: {location}"
        );
    }
}

#[test]
fn copied_console_sources_survive_the_real_core_child_boundary() {
    let (fixture, url, gatekeeper) = fixture(
        "<p>console positions</p><script>\nreportError('copied-source');\nFunction(\"throw 'unknown-source';\")();\n</script>".into(),
    );
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
    let ServerMessage::ConsoleMessages(batch) =
        request(&mut stream, 2, ClientMessage::GetConsoleMessages)
    else {
        panic!("expected copied console records");
    };
    assert_eq!(batch.records.len(), 2, "{batch:?}");
    let batch = serde_json::to_value(batch).unwrap();
    assert_eq!(batch["records"][0]["source"]["line"], 2);
    assert_eq!(batch["records"][0]["source"]["column"], 1);
    assert!(
        batch["records"][0]["source"]["filename"]
            .as_str()
            .is_some_and(|name| name.starts_with("blueice://page/tab-1/document-")
                && name.ends_with("/inline-0.js")),
        "{batch}"
    );
    assert!(batch["records"][1].get("source").is_none());
}

#[test]
fn uncaught_page_errors_keep_the_throwing_script_source_coordinates() {
    let mut failures = Vec::new();
    for (name, source, coordinates) in [
        ("root", "\nthrow 'position-probe';\n", "2:1"),
        (
            "block",
            "\nif (true) {\n  throw 'position-probe';\n}\n",
            "3:3",
        ),
        (
            "function",
            "\nfunction fail() {\n  throw 'position-probe';\n}\nfail();\n",
            "3:3",
        ),
        (
            "finally",
            "\ntry {\n  throw 'position-probe';\n} finally {\n  try { throw 'caught'; } catch {}\n}\n",
            "3:3",
        ),
        (
            "iterator cleanup",
            "\nvar iterator={ [Symbol.iterator](){return this;}, next(){return {value:1,done:false};}, return(){throw 'close';} };\nfor(var value of iterator){\n  throw 'position-probe';\n}\n",
            "4:3",
        ),
        (
            "suspended finally",
            "\nfunction* fail(){\n  try {\n    throw 'position-probe';\n  } finally { yield 1; try { throw 'caught'; } catch {} }\n}\nvar suspended=fail(); suspended.next();\n</script><script>\ntry { throw 'ambient'; } catch {} suspended.next();\n",
            "4:5",
        ),
        ("expression operand", "var value=(\n  1 +\n  missing);", "3:3"),
        ("default parameter", "function fail(value=\n  missing){}fail();", "2:3"),
        ("destructuring parameter", "function fail(\n  {value}\n){}fail(null);", "2:3"),
        ("named arrow", "var fail=()=> (\n  missing);fail();", "2:3"),
        ("template placeholder", "var value=`${ /* } */\n  missing}`;", "2:3"),
        ("optional callee reference", "(\n null.field\n)?.();", "2:2"),
        ("prefix reference", "++\n missing;", "2:2"),
        ("nested assignment array", "var value;[\n [value]\n]=[null];", "2:2"),
        ("assignment shorthand", "'use strict';({\n missing\n}={missing:1});", "2:2"),
        ("prepared rest target", "var object=null;[...\n object.field\n]=[];", "2:2"),
        ("for-of target", "'use strict';for(\n missing\n of [1]){}", "2:2"),
        ("class constructor", "class C {constructor(){\n  missing;}}new C();", "2:3"),
        ("class getter", "class C {get value(){\n  return missing;}}new C().value;", "2:10"),
        ("class field", "class C {value=\n  missing;}new C();", "2:3"),
        ("class static block", "class C {static {\n  missing;}}", "2:3"),
        ("decorator member", "var decorators=null;\n@decorators.decorate\nclass C {}", "2:2"),
        ("decorated field", "function dec(){}\nclass C {@dec value=\n  missing;}new C();", "3:3"),
    ] {
        let location = observed_error_source_location(source);
        if !location.starts_with("location:blueice://page/tab-1/document-")
            || !location.ends_with(&format!("/inline-1.js:{coordinates}"))
        {
            failures.push((name, location, coordinates));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn checked_host_member_chains_reject_before_any_page_mutation() {
    for call in [
        "window.console.missing.deep();",
        "window['absent'].deep();",
        "window['fetch']('https://example.test');",
    ] {
        let (fixture, url, gatekeeper) = fixture(format!(
            "<p id=counter>untouched</p><script type='application/x-blueice-typescript'>document.getElementById('counter')!.textContent = 'escaped'; {call}</script>"
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
        assert!(matches!(
            request(&mut stream, 1, ClientMessage::Navigate { url }),
            ServerMessage::Navigated { .. }
        ));
        let ServerMessage::BlueTsScriptReports(reports) =
            request(&mut stream, 2, ClientMessage::GetBlueTsScriptReports)
        else {
            panic!("expected BlueTS reports");
        };
        assert!(
            matches!(reports.as_slice(), [blueice_ipc::BlueTsScriptExecutionReport { outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category }, .. }] if category == "BlueTS compilation rejected the page script"),
            "{call}: {reports:?}"
        );
        let ServerMessage::Representation(snapshot) =
            request(&mut stream, 3, ClientMessage::GetRepresentation)
        else {
            panic!("expected page representation");
        };
        assert!(
            snapshot
                .nodes
                .iter()
                .any(|node| node.name.as_deref() == Some("untouched")),
            "{call}: {snapshot:?}"
        );
        assert!(
            !snapshot
                .nodes
                .iter()
                .any(|node| node.name.as_deref() == Some("escaped")),
            "{call}: {snapshot:?}"
        );
    }
}

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
fn javascript_and_bluets_console_logging_keeps_page_tasks_rendering() {
    counter_frames(
        "let count: number = 0;
        console.log('initial');
        function tick(): void {
            count += 1; console.warn('tick:%d', count);
            window.document.getElementById('counter')!.textContent = 'tick:' + count;
            if (count < 3) { window.setTimeout(tick, 500); }
            else { console.error('finished', count); }
        }
        window.setTimeout(tick, 500);",
        true,
    );
}

#[test]
fn javascript_and_bluets_pin_the_browser_integration_global_inventory() {
    // Every global named in section C of BROWSER_INTEGRATION_TODO.md, plus
    // the installed scheduling and DOM entries. These expectations are an
    // independent public observation of the real owner-selected profile.
    let inventory = [
        ("window", "object"),
        ("self", "object"),
        ("top", "object"),
        ("parent", "object"),
        ("console", "object"),
        ("location", "undefined"),
        ("history", "undefined"),
        ("navigator", "undefined"),
        ("URL", "function"),
        ("URLSearchParams", "undefined"),
        ("TextEncoder", "undefined"),
        ("TextDecoder", "undefined"),
        ("atob", "undefined"),
        ("btoa", "undefined"),
        ("structuredClone", "undefined"),
        ("performance", "undefined"),
        ("AbortController", "function"),
        ("AbortSignal", "function"),
        ("localStorage", "undefined"),
        ("sessionStorage", "undefined"),
        ("EventTarget", "function"),
        ("Event", "function"),
        ("CustomEvent", "function"),
        ("Blob", "function"),
        ("File", "function"),
        ("FileReader", "function"),
        ("ReadableStream", "function"),
        ("DOMException", "function"),
        ("WritableStream", "undefined"),
        ("TransformStream", "undefined"),
        ("CompressionStream", "undefined"),
        ("crypto", "undefined"),
        ("reportError", "function"),
        ("PromiseRejectionEvent", "function"),
        ("onunhandledrejection", "object"),
        ("onrejectionhandled", "object"),
        ("ErrorEvent", "function"),
        ("onerror", "object"),
        ("addEventListener", "function"),
        ("removeEventListener", "function"),
        ("dispatchEvent", "function"),
        ("document", "object"),
        ("setTimeout", "function"),
        ("setInterval", "function"),
        ("clearTimeout", "function"),
        ("clearInterval", "function"),
        ("queueMicrotask", "function"),
        ("requestAnimationFrame", "function"),
        ("cancelAnimationFrame", "function"),
        ("Window", "undefined"),
        ("postMessage", "undefined"),
        ("MessageChannel", "undefined"),
    ];
    let mut source = String::from("function verifyInventory(): void {\n");
    for (name, expected) in inventory {
        source.push_str(&format!(
            "if (typeof globalThis.{name} !== '{expected}') {{ throw 'inventory:{name}'; }}\n"
        ));
    }
    source.push_str(
        "}\nverifyInventory();\nlet count: number = 0;
        function tick(): void {
            count += 1; window.document.getElementById('counter')!.textContent = 'tick:' + count;
            if (count < 3) { window.setTimeout(tick, 500); }
        }
        window.setTimeout(tick, 500);",
    );
    counter_frames(&source, true);
}

#[test]
fn javascript_and_bluets_window_aliases_share_dom_and_scheduled_work() {
    counter_frames("let count: number = 0; let timer: number = 0;
        function verify(): void {
            if (window !== globalThis || self !== window || top !== window || parent !== window) {
                throw 'global aliases must share the page realm';
            }
        }
        verify();
        function publish(): void { parent.document.getElementById('counter')!.textContent = 'tick:' + count; }
        function tick(): void {
            count += 1; self.queueMicrotask(publish);
            if (count === 3) { top.clearInterval(timer); }
        }
        timer = window.setInterval(tick, 500);", true);
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
        let mut frame_generations = Vec::new();
        let mut representations = Vec::new();
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
            frame_generations.push(generation);
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
                // The next timer may publish while this request is in flight.
                // Match a live snapshot to its frame after all frames arrive.
                assert!(snapshot.generation >= generation, "{snapshot:?}");
                let snapshot_count = snapshot
                    .nodes
                    .iter()
                    .find_map(|node| {
                        node.name
                            .as_deref()?
                            .strip_prefix("tick:")?
                            .parse::<u64>()
                            .ok()
                    })
                    .expect("the Representation must include the counter");
                assert!((count..=3).contains(&snapshot_count), "{snapshot:?}");
                representations.push((snapshot_count, snapshot.generation));
            }
            previous_generation = generation;
            previous_pixels = pixels;
        }
        for (count, generation) in representations {
            assert_eq!(
                frame_generations[(count - 1) as usize],
                generation,
                "the Representation must match the frame for tick:{count}"
            );
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
