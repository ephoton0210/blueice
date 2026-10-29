// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real BlueTS watch cycles must match the frozen native and a fresh build.
use super::support::*;
use super::workflows;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

pub struct Watch {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    readers: Vec<thread::JoinHandle<()>>,
    seen: Vec<String>,
}
fn reader<R: Read + Send + 'static>(
    stream: R,
    sender: mpsc::Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else {
                break;
            };
            if sender.send(line).is_err() {
                break;
            }
        }
    })
}
impl Watch {
    pub fn spawn(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(root)
            .args([
                "--build",
                "tsconfig.json",
                "--watch",
                "--verbose",
                "--pretty",
                "false",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let (sender, lines) = mpsc::channel();
        let readers = vec![
            reader(child.stdout.take().unwrap(), sender.clone()),
            reader(child.stderr.take().unwrap(), sender),
        ];
        Self {
            child,
            input,
            lines,
            readers,
            seen: Vec::new(),
        }
    }
    pub fn cycle(&mut self, index: usize) -> Vec<String> {
        self.cycle_result(index, false)
    }
    pub fn cycle_result(&mut self, index: usize, error: bool) -> Vec<String> {
        let until = Instant::now() + Duration::from_secs(10);
        let mut result = Vec::new();
        loop {
            assert!(
                Instant::now() < until,
                "watch cycle {index} timeout: {:?}",
                self.seen
            );
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "watch ended: {:?}",
                self.seen
            );
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(50)) {
                self.seen.push(line.clone());
                if line == format!("Watch build {index} complete") {
                    assert!(!error, "unexpected green watch cycle");
                    return result;
                }
                if line == format!("Watch build {index} completed with errors") {
                    assert!(error, "watch error: {:?}", self.seen);
                    return result;
                }
                result.push(line);
            }
        }
    }
    pub fn quiet(&mut self) {
        let until = Instant::now() + Duration::from_millis(400);
        while Instant::now() < until {
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(25)) {
                assert!(
                    !line.starts_with("Watch build ") && !line.starts_with("File change detected"),
                    "extra cycle: {line}"
                );
                self.seen.push(line);
            }
        }
    }
    pub fn stop(mut self) {
        self.input.as_mut().unwrap().write_all(b"q\n").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < until, "watch shutdown timeout");
            thread::sleep(Duration::from_millis(25));
        }
        for reader in self.readers.drain(..) {
            reader.join().unwrap();
        }
    }
    pub fn failure(mut self) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(
                Instant::now() < until,
                "watch budget did not stop the process"
            );
            thread::sleep(Duration::from_millis(25));
        }
        for reader in self.readers.drain(..) {
            reader.join().unwrap();
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
pub fn observations() -> Vec<Value> {
    let input = input();
    let reference = reference();
    let mut failures = Vec::new();
    let mut observed = Vec::new();
    for workflow in input["watch"].as_array().unwrap() {
        let temp = Temporary::new("watch");
        let root = &temp.0;
        let mut inputs = workflow["files"].clone();
        write(root, &inputs);
        let mut process = Watch::spawn(root);
        for (index, step) in workflow["steps"].as_array().unwrap().iter().enumerate() {
            if index > 0 {
                apply(root, &mut inputs, &step["changes"]);
                for changes in step["burst"].as_array().into_iter().flatten() {
                    apply(root, &mut inputs, changes);
                }
            }
            let lines = process.cycle(index);
            assert!(
                !lines
                    .iter()
                    .any(|line| line.contains("error TS") || line.contains("bluetsc:")),
                "{lines:?}"
            );
            let actual = json!({"exit":0,"events":events(root,&lines.join("\n")),"diagnostics":[],
                "outputs":outputs(root,&inputs),"selectedInputs":workflows::selected(root)});
            compare(&actual, expected(&reference, &step["id"]), &mut failures);
            let clean = Temporary::new("clean");
            write(&clean.0, &inputs);
            let fresh = run(&clean.0, &["--build".into(), "tsconfig.json".into()]);
            assert!(
                fresh.status.success(),
                "{}{}",
                String::from_utf8_lossy(&fresh.stdout),
                String::from_utf8_lossy(&fresh.stderr)
            );
            assert_eq!(
                outputs(&clean.0, &inputs),
                actual["outputs"],
                "{}: actual watch/clean bytes",
                step["id"]
            );
            observed.push(attach(step, &inputs, &actual));
            process.quiet();
        }
        process.stop();
    }
    finish(&failures);
    observed
}
