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

fn reply(
    stream: &mut UnixStream,
    tab: u64,
    message: &ClientMessage,
    request: u64,
) -> ServerMessage {
    blueice_ipc::write_client_message_with_ids(stream, Some(tab), Some(request), message).unwrap();
    loop {
        let (_, id, message) = blueice_ipc::read_server_message_with_ids(stream).unwrap();
        if id == Some(request) && !matches!(message, ServerMessage::FrameReady { .. }) {
            return message;
        }
    }
}

#[test]
fn page_blob_urls_render_without_network_and_retire_on_creator_navigation() {
    let root = std::path::Path::new("/tmp").join(format!(
        "bi-object-url-{}-{}",
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
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = fixture.stop.clone();
    fixture.workers.push(thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            match listener.accept() {
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
    let url = format!("http://{}/creator", server.local_addr().unwrap());
    let payload = format!("<p id=result>local object URL 中文</p><div id=script>pending</div><script>document.getElementById('script').textContent='blob script ran';</script><!--{}-->", "x".repeat(70_000));
    let payload = serde_json::to_string(&payload)
        .unwrap()
        .replace("</script>", r#"<\/script>"#);
    let body = format!("<div id=live>pending</div><div id=revoked>pending</div><script>const html=new Blob([{payload}],{{type:'text/html'}});document.getElementById('live').textContent='live:'+URL.createObjectURL(html);const revoked=URL.createObjectURL(html);URL.revokeObjectURL(revoked);document.getElementById('revoked').textContent='revoked:'+revoked;</script>");
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let worker_hits = hits.clone();
    let stop = fixture.stop.clone();
    fixture.workers.push(thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            match server.accept() {
                Ok((mut stream, _)) => { stream.set_nonblocking(false).unwrap(); stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap(); let mut request=[0;4096]; if stream.read(&mut request).unwrap_or(0)>0 { worker_hits.fetch_add(1,Ordering::Relaxed); let _=stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()); } }
                Err(error) if error.kind()==std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                Err(error)=>panic!("{error}"),
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
        reply(&mut stream, 1, &ClientMessage::Navigate { url }, 1),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::Dom(dom) = reply(&mut stream, 1, &ClientMessage::GetDom, 2) else {
        panic!("expected creator DOM")
    };
    let live = dom
        .split('"')
        .find_map(|text| text.strip_prefix("live:blob:"))
        .map(|suffix| format!("blob:{suffix}"))
        .expect("page must publish its actual Blob URL");
    let revoked = dom
        .split('"')
        .find_map(|text| text.strip_prefix("revoked:blob:"))
        .map(|suffix| format!("blob:{suffix}"))
        .expect("page must revoke its actual Blob URL");
    assert!(
        matches!(reply(&mut stream,1,&ClientMessage::Navigate{url:revoked},3),ServerMessage::Error{message} if message.contains("Blob URL unavailable"))
    );
    let ServerMessage::TabOpened { tab_id: peer, .. } = reply(
        &mut stream,
        1,
        &ClientMessage::OpenTab {
            url: Some(format!("{live}#section")),
        },
        4,
    ) else {
        panic!("live Blob URL must navigate locally")
    };
    let ServerMessage::Dom(dom) = reply(&mut stream, peer, &ClientMessage::GetDom, 5) else {
        panic!("expected Blob DOM")
    };
    assert!(dom.contains("\"local object URL 中文\""), "{dom}");
    assert!(dom.contains("\"blob script ran\""), "{dom}");
    assert!(matches!(
        reply(
            &mut stream,
            1,
            &ClientMessage::Navigate {
                url: "about:blank".into()
            },
            6
        ),
        ServerMessage::Navigated { .. }
    ));
    assert!(
        matches!(reply(&mut stream,peer,&ClientMessage::Navigate{url:live},7),ServerMessage::Error{message} if message.contains("Blob URL unavailable"))
    );
    assert_eq!(
        hits.load(Ordering::Relaxed),
        1,
        "Blob loads must not refetch HTTP content"
    );
    drop(stream);
    drop(core);
}
