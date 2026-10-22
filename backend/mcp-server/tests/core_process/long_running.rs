// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real Core/child long-running and two-tab acceptance. Opt in with
//! `cargo test -p blueice-mcp-server --test core_process long_running -- --ignored --nocapture`;
//! `BLUEJS_ACCEPT_SECS` sets the steady-state phase (default 90).
//!
//! One launcher-managed Core with its supervised BlueJS child serves two tabs
//! over a real MCP client: a counter page (`setInterval`) and a Promise/timer
//! churn page. The test samples the page's own representation, the console and
//! the resident memory of every descendant process, then makes the second tab
//! exhaust its document budget, navigate away and close, while the first tab
//! must keep running throughout.

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const COUNTER: &str = "<p id=n>0</p><script>var n=0;setInterval(function(){n++;document.getElementById('n').textContent='count '+n;},50);</script>";
const CHURN: &str = "<p>churn</p><script>var ticks=0;function tick(){var s='x'.repeat(50000);Promise.resolve(s).then(function(v){ticks++;if(ticks%100===0){console.log('tick '+ticks);}setTimeout(tick,10);});}tick();</script>";
const EXHAUST: &str = "<p>exhaust</p><script>var i=0;function burn(){for(var k=0;k<20;k++){Symbol.for('y'.repeat(400000)+(i++));}setTimeout(burn,5);}burn();</script>";
const AFTER: &str = "<p>recovered</p><script>console.log('recovered');</script>";

struct Site {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Site {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let read = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]);
                let body = if request.contains("GET /counter") {
                    COUNTER
                } else if request.contains("GET /churn") {
                    CHURN
                } else if request.contains("GET /exhaust") {
                    EXHAUST
                } else {
                    AFTER
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Resident KiB of every process descended from this test process (Core and
/// its BlueJS child), by name.
fn descendant_rss() -> Vec<(String, u64)> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,rss=,comm="])
        .output()
        .expect("ps must run");
    let rows: Vec<(u32, u32, u64, String)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((
                parts.next()?.parse().ok()?,
                parts.next()?.parse().ok()?,
                parts.next()?.parse().ok()?,
                parts.next()?.rsplit('/').next()?.to_string(),
            ))
        })
        .collect();
    let mut family = vec![std::process::id()];
    let mut found = Vec::new();
    loop {
        let before = family.len();
        for (pid, ppid, rss, name) in &rows {
            if family.contains(ppid) && !family.contains(pid) {
                family.push(*pid);
                found.push((name.clone(), *rss));
            }
        }
        if family.len() == before {
            return found;
        }
    }
}
fn total_rss() -> u64 {
    descendant_rss().iter().map(|(_, rss)| rss).sum()
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, CompilerMcpClient>,
    tool: &'static str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let result = client
        .call_tool(
            CallToolRequestParams::new(tool).with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false), "{tool}: {result:?}");
    let text = &result.content[0].as_text().unwrap().text;
    let marker = format!("{}\n", blueice_mcp_server::UNTRUSTED_CONTENT_MARKER);
    let payload = text.split_once(&marker).map_or(text.as_str(), |(_, p)| p);
    serde_json::from_str(payload).unwrap_or_else(|_| serde_json::Value::String(text.clone()))
}

/// (generation, counter value) from a representation, if the counter is shown.
fn counter(snapshot: &serde_json::Value) -> (u64, Option<u64>) {
    let generation = snapshot["generation"].as_u64().unwrap();
    let value = snapshot["nodes"].as_array().and_then(|nodes| {
        nodes.iter().find_map(|node| {
            node["name"]
                .as_str()?
                .strip_prefix("count ")?
                .parse::<u64>()
                .ok()
        })
    });
    (generation, value)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "minutes-long real-process acceptance; run explicitly"]
async fn two_tab_long_running_acceptance() {
    let seconds: u64 = std::env::var("BLUEJS_ACCEPT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(90);
    let site = Site::start();
    let mut launcher = LauncherManagedCore::spawn_with_profile(None, true);
    let server = BlueIceMcpServer::connect_with_core_and_compiler_sockets(
        &launcher.rendezvous_socket,
        &launcher.compiler_socket,
    )
    .unwrap();
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let client = CompilerMcpClient.serve(client_transport).await.unwrap();

    let a = 1;
    call(
        &client,
        "navigate",
        serde_json::json!({"url": format!("{}/counter", site.url), "tab_id": a}),
    )
    .await;
    let opened = call(
        &client,
        "open_tab",
        serde_json::json!({"url": format!("{}/churn", site.url)}),
    )
    .await;
    eprintln!("open_tab -> {opened}");
    let b = opened["tab_id"].as_u64().or_else(|| {
        opened.as_str().and_then(|text| {
            text.split(|c: char| !c.is_ascii_digit())
                .find_map(|part| part.parse::<u64>().ok())
        })
    });
    let b = b.expect("open_tab must report the new tab id");
    assert_ne!(a, b);

    let start = Instant::now();
    let warmup = Duration::from_secs((seconds / 4).max(5));
    let deadline = start + Duration::from_secs(seconds);
    let mut samples: Vec<(u64, u64, u64, u64)> = Vec::new(); // secs, rss KiB, generation, count
    let mut last_generation = 0;
    let mut last_count = 0;
    let mut churn_ticks = 0;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let snapshot = call(
            &client,
            "get_page_representation",
            serde_json::json!({"tab_id": a}),
        )
        .await;
        let (generation, count) = counter(&snapshot);
        let count = count.expect("tab A must show its counter");
        assert!(
            generation > last_generation,
            "A's generation must advance: {generation} after {last_generation}"
        );
        assert!(
            count > last_count,
            "A's counter must advance: {count} after {last_count}"
        );
        last_generation = generation;
        last_count = count;
        let console = call(
            &client,
            "get_console_messages",
            serde_json::json!({"tab_id": b}),
        )
        .await;
        for record in console["records"].as_array().into_iter().flatten() {
            if let Some(n) = record["message"]
                .as_str()
                .and_then(|m| m.strip_prefix("tick "))
                .and_then(|n| n.parse::<u64>().ok())
            {
                assert!(
                    n > churn_ticks,
                    "B's churn must advance: {n} after {churn_ticks}"
                );
                churn_ticks = n;
            }
        }
        let rss = total_rss();
        samples.push((start.elapsed().as_secs(), rss, generation, count));
        eprintln!(
            "t={:>4}s rss={:>7} KiB gen={generation} count={count} churn={churn_ticks}",
            start.elapsed().as_secs(),
            rss
        );
    }
    assert!(churn_ticks > 0, "tab B never reported its churn");

    // Resident memory must be bounded once warm: allow allocator noise only.
    let warm: Vec<_> = samples.iter().filter(|s| s.0 >= warmup.as_secs()).collect();
    let base = warm.first().expect("a warm sample").1;
    let peak = warm.iter().map(|s| s.1).max().unwrap();
    eprintln!("warm baseline {base} KiB, steady-state peak {peak} KiB");
    assert!(
        peak <= base + 32 * 1024,
        "resident memory grew {} KiB after warm-up ({base} -> {peak})",
        peak - base
    );

    // B exhausts its own document budget; A must be unaffected.
    call(
        &client,
        "navigate",
        serde_json::json!({"url": format!("{}/exhaust", site.url), "tab_id": b}),
    )
    .await;
    let before = last_count;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let (_, count) = counter(
        &call(
            &client,
            "get_page_representation",
            serde_json::json!({"tab_id": a}),
        )
        .await,
    );
    assert!(
        count.unwrap() > before,
        "A stalled while B exhausted its budget"
    );

    // B recovers on navigation; its old document's work is gone.
    call(
        &client,
        "navigate",
        serde_json::json!({"url": format!("{}/after", site.url), "tab_id": b}),
    )
    .await;
    let mut recovered = false;
    for _ in 0..50 {
        let console = call(
            &client,
            "get_console_messages",
            serde_json::json!({"tab_id": b}),
        )
        .await;
        if console["records"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r["message"] == "recovered")
        {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        recovered,
        "B must run scripts again after navigating away from exhaustion"
    );

    // Closing B releases its document; memory returns near the warm baseline.
    call(&client, "close_tab", serde_json::json!({"tab_id": b})).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let settled = total_rss();
    eprintln!("after close: {settled} KiB (warm baseline {base} KiB)");
    assert!(
        settled <= base + 32 * 1024,
        "memory did not settle after closing B: {settled} vs {base}"
    );
    let (_, count) = counter(
        &call(
            &client,
            "get_page_representation",
            serde_json::json!({"tab_id": a}),
        )
        .await,
    );
    assert!(count.unwrap() > before, "A must still run after B closed");

    drop(client);
    server_task.abort();
    launcher.shutdown();
}
