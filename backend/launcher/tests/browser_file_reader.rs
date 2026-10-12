// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_ipc::{ClientMessage, ServerMessage};
use blueice_launcher::{CoreLaunchOptions, SpawnedCore};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Fixture {
    root: std::path::PathBuf,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn reply(stream: &mut UnixStream, message: &ClientMessage, request: u64) -> ServerMessage {
    blueice_ipc::write_client_message_with_ids(stream, Some(1), Some(request), message).unwrap();
    loop {
        let (_, id, message) = blueice_ipc::read_server_message_with_ids(stream).unwrap();
        if id == Some(request) && !matches!(message, ServerMessage::FrameReady { .. }) {
            return message;
        }
    }
}

#[test]
fn ordinary_file_reader_tasks_mutate_the_live_core_after_scripts_and_selection_events() {
    // Darwin's per-user TMPDIR may already consume most of SUN_LEN.
    let root = std::path::Path::new("/tmp").join(format!(
        "bi-file-reader-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
    let url = format!("http://{}/reader", server.local_addr().unwrap());
    let stop = fixture.stop.clone();
    fixture.workers.push(thread::spawn(move || {
        let body = concat!(
            "<div id=initial>before</div><div id=selected>none</div><div id=streamed>pending</div>",
            "<input id=upload type=file aria-label=Upload>",
            "<script>",
            "let lateReady=false, lateObserved=false; const initial=new FileReader();",
            "initial.onload=()=>{if(!lateReady) throw 'task ran before later scripts';};",
            "initial.onloadend=()=>{document.getElementById('initial').textContent=initial.result+':'+lateObserved;};",
            "initial.readAsText(new Blob(['initial 中文']));",
            "const byteReader=new Blob([new Uint8Array([7,0,255])]).stream().getReader();",
            "byteReader.read().then(result=>{document.getElementById('streamed').textContent='stream:'+Array.from(result.value).join(',');});",
            "const upload=document.getElementById('upload'), events=[]; let selected=null;",
            "upload.addEventListener('input', ()=>{events.push('input');const reader=new FileReader();",
            "for(const type of ['loadstart','progress','load','loadend']) reader.addEventListener(type, ()=>events.push(type));",
            "reader.onload=()=>{selected=Array.from(new Uint8Array(reader.result));};",
            "reader.onloadend=()=>{document.getElementById('selected').textContent=events.join(',')+':'+selected.join(',');};",
            "reader.readAsArrayBuffer(upload.files[0]);});",
            "upload.addEventListener('change', ()=>events.push('change'));</script>",
            "<script>lateReady=true;initial.addEventListener('load',()=>{lateObserved=true;});</script>"
        );
        while !stop.load(Ordering::Acquire) {
            match server.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut request = [0; 4096];
                    if stream.read(&mut request).unwrap_or(0) != 0 {
                        let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("{error}"),
            }
        }
    }));
    let core = SpawnedCore::spawn_with_options(
        400.0,
        300.0,
        &fixture.root.join("frames"),
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path)
            .supervise_out_of_process_bluejs_with_page_dom(),
    )
    .unwrap();
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    assert!(matches!(
        reply(&mut stream, &ClientMessage::Navigate { url }, 1),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::Dom(dom) = reply(&mut stream, &ClientMessage::GetDom, 2) else {
        panic!("expected DOM")
    };
    assert!(dom.contains("\"initial 中文:true\""), "{dom}");
    assert!(dom.contains("\"stream:7,0,255\""), "{dom}");
    let ServerMessage::Representation(snapshot) =
        reply(&mut stream, &ClientMessage::GetRepresentation, 3)
    else {
        panic!("expected representation")
    };
    let upload = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Upload"))
        .unwrap();
    let ServerMessage::TextInputState(state) =
        reply(&mut stream, &ClientMessage::GetTextInputState, 4)
    else {
        panic!("expected context")
    };
    let ServerMessage::FileInputState(files) = reply(
        &mut stream,
        &ClientMessage::FileInput(blueice_ipc::file_input::FileInputAction::Prepare {
            frame_source: state.frame_source,
            document_generation: state.document_generation,
            node_id: upload.id,
        }),
        5,
    ) else {
        panic!("expected file state")
    };
    assert!(matches!(
        reply(
            &mut stream,
            &ClientMessage::FileInput(blueice_ipc::file_input::FileInputAction::Set {
                context: files.context,
                files: vec![blueice_ipc::file_input::FileData {
                    name: "selected-中文.bin".into(),
                    media_type: "application/octet-stream".into(),
                    last_modified: 123,
                    bytes: vec![0, 255, 65],
                }],
            }),
            6
        ),
        ServerMessage::FileInputState(_)
    ));
    let ServerMessage::Dom(dom) = reply(&mut stream, &ClientMessage::GetDom, 7) else {
        panic!("expected selected DOM")
    };
    assert!(
        dom.contains("\"input,change,loadstart,progress,load,loadend:0,255,65\""),
        "{dom}"
    );
    drop(stream);
    drop(core);
}
