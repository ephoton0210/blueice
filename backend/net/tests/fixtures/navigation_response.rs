// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One owned loopback response, optionally withholding its body until release.
//! Deadlines and Drop release/reap the fixture even when a regression fails.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct ObservedRequest {
    pub method: String,
    pub target: String,
    pub body: Vec<u8>,
}

pub struct ResponseServer {
    pub url: String,
    headers_sent: mpsc::Receiver<()>,
    release: Option<mpsc::Sender<()>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<ObservedRequest>>>,
}

impl ResponseServer {
    pub fn start(status: &str, headers: &[(&str, &str)], body: &[u8], held: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/response", listener.local_addr().unwrap());
        let mut head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (key, value) in headers {
            head.push_str(&format!("{key}: {value}\r\n"));
        }
        head.push_str("\r\n");
        let body = body.to_vec();
        let (headers_tx, headers_sent) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if stop.load(Ordering::Acquire) || Instant::now() >= until {
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "no fixture request",
                            ));
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => return Err(error),
                }
            };
            // Accepted sockets can inherit the listener's nonblocking mode.
            // A client may connect before it sends its first request bytes.
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_secs(3)))?;
            stream.set_write_timeout(Some(Duration::from_secs(3)))?;
            let request = read_request(&mut stream)?;
            stream.write_all(head.as_bytes())?;
            stream.flush()?;
            let _ = headers_tx.send(());
            if held {
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
            }
            stream.write_all(&body)?;
            stream.flush()?;
            Ok(request)
        });
        Self {
            url,
            headers_sent,
            release: Some(release),
            cancelled,
            worker: Some(worker),
        }
    }

    pub fn wait_for_headers(&self) -> Result<(), mpsc::RecvTimeoutError> {
        self.headers_sent.recv_timeout(Duration::from_secs(3))
    }

    pub fn release(&mut self) {
        if let Some(sender) = self.release.take() {
            let _ = sender.send(());
        }
    }

    pub fn finish(mut self) -> ObservedRequest {
        self.release();
        self.worker.take().unwrap().join().unwrap().unwrap()
    }
}

impl Drop for ResponseServer {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.release();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<ObservedRequest> {
    let mut raw = Vec::new();
    let end = loop {
        read_more(stream, &mut raw)?;
        if let Some(end) = raw.windows(4).position(|x| x == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = std::str::from_utf8(&raw[..end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid fixture headers"))?;
    let mut words = head.lines().next().unwrap_or_default().split_whitespace();
    let method = words.next().unwrap_or_default().to_owned();
    let target = words.next().unwrap_or_default().to_owned();
    let length = head
        .lines()
        .filter_map(|x| x.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid fixture length"))?
        .unwrap_or(0);
    if length > 16_384 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fixture body too large",
        ));
    }
    while raw.len() < end + length {
        read_more(stream, &mut raw)?;
    }
    Ok(ObservedRequest {
        method,
        target,
        body: raw[end..end + length].to_vec(),
    })
}

fn read_more(stream: &mut TcpStream, raw: &mut Vec<u8>) -> io::Result<()> {
    let mut buffer = [0; 1024];
    let count = stream.read(&mut buffer)?;
    if count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated fixture request",
        ));
    }
    raw.extend_from_slice(&buffer[..count]);
    if raw.len() > 32_768 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fixture request too large",
        ));
    }
    Ok(())
}
