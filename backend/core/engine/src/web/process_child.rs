// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The process that runs a document's scripts.
//!
//! `blueice-core --page-scripts-child <socket>` connects back to the core that
//! started it and serves one document: it builds the realm from the document it
//! is sent, runs the turns it is told to, and answers with what they did. It
//! owns the live document between turns; the core asks for a copy when it needs
//! one. It has no network and opens no files: a request a script makes goes back
//! to the core as data, and what the scripts want to know about layout they ask
//! the core for, mid-turn, one question at a time.

use super::dom_bridge::{DomState, RemoteQueries};
use super::turn::run_turn;
use super::wire::{read_frame, write_frame, Answer, ChildDone, FromChild, Query, ToChild};
use super::WebHost;
use blueice_dom::{Document, NodeId};
use blueice_image::RasterImage;
use std::cell::RefCell;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;

/// The scripts' process, from `main`: returns the exit status.
pub fn run_script_child(socket: &Path) -> i32 {
    let stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("page-scripts child: cannot connect to the core: {error}");
            return 2;
        }
    };
    // Page scripts nest deeply (React's render is hundreds of frames): give the
    // realm the same stack the core's own main thread has, and more.
    let served = std::thread::Builder::new()
        .name("page-scripts".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || serve(stream))
        .map_err(io::Error::other)
        .and_then(|thread| thread.join().map_err(|_| io::Error::other("the realm panicked")));
    match served {
        Ok(Ok(())) => 0,
        Ok(Err(error)) | Err(error) => {
            eprintln!("page-scripts child: {error}");
            1
        }
    }
}

/// Asks the core about layout while a turn runs.
struct ChildQueries {
    stream: RefCell<UnixStream>,
}

impl ChildQueries {
    fn ask(&self, query: Query) -> Option<Answer> {
        let mut stream = self.stream.borrow_mut();
        write_frame(&mut *stream, &FromChild::Query(query).encode()).ok()?;
        let frame = read_frame(&mut *stream).ok()??;
        match ToChild::decode(&frame)? {
            ToChild::Answer(answer) => Some(answer.0),
            _ => None,
        }
    }
}

impl RemoteQueries for ChildQueries {
    fn computed(&self, node: NodeId, property: &str) -> Option<String> {
        match self.ask(Query::Computed {
            node: node.as_u64(),
            property: property.to_string(),
        })? {
            Answer::Text(text) => text,
            _ => None,
        }
    }

    fn rect(&self, node: NodeId) -> Option<(f64, f64, f64, f64)> {
        match self.ask(Query::Rect(node.as_u64()))? {
            Answer::Rect(rect) => rect,
            _ => None,
        }
    }

    fn image(&self, node: NodeId) -> Option<Arc<RasterImage>> {
        match self.ask(Query::Image(node.as_u64()))? {
            Answer::Image(image) => image,
            _ => None,
        }
    }
}

fn refused(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

fn serve(stream: UnixStream) -> io::Result<()> {
    let mut input = stream.try_clone()?;
    let mut output = stream.try_clone()?;
    let Some(first) = read_frame(&mut input)? else {
        return Ok(());
    };
    let Some(ToChild::Init { doc, url, viewport }) = ToChild::decode(&first) else {
        return Err(refused("the first message was not Init"));
    };
    let Some(doc) = Document::decode(&doc) else {
        write_frame(
            &mut output,
            &FromChild::InitFailed("the document did not decode".into()).encode(),
        )?;
        return Err(refused("the document did not decode"));
    };
    let mut state = DomState::new(doc, &url, viewport);
    state.remote = Some(Box::new(ChildQueries {
        stream: RefCell::new(stream.try_clone()?),
    }));
    let mut host = match WebHost::new(state) {
        Ok(host) => host,
        Err(error) => {
            write_frame(&mut output, &FromChild::InitFailed(error).encode())?;
            return Ok(());
        }
    };
    write_frame(&mut output, &FromChild::Ready.encode())?;
    let mut console_sent = 0;
    loop {
        let Some(frame) = read_frame(&mut input)? else {
            return Ok(());
        };
        match ToChild::decode(&frame).ok_or_else(|| refused("a message did not decode"))? {
            ToChild::Turn {
                viewport,
                url,
                turn,
            } => {
                let revision = host.with_state(|s| {
                    s.viewport = viewport;
                    if let Some(url) = url {
                        s.url = url;
                    }
                    s.dirty = false;
                    s.doc.revision()
                });
                run_turn(&mut host, turn);
                let console = host.console()[console_sent..].to_vec();
                console_sent += console.len();
                let done = host.with_state(|s| ChildDone {
                    dirty: s.dirty || s.doc.revision() != revision,
                    navigations: std::mem::take(&mut s.navigations),
                    script_requests: std::mem::take(&mut s.script_requests)
                        .into_iter()
                        .map(|(node, url)| (node.as_u64(), url))
                        .collect(),
                    url: s.url.clone(),
                    net_jobs: std::mem::take(&mut s.net_jobs),
                    canvases: s
                        .take_canvas_pictures()
                        .into_iter()
                        .map(|(node, image)| (node.as_u64(), image))
                        .collect(),
                    loaded_images: std::mem::take(&mut s.loaded_images)
                        .into_iter()
                        .map(|(src, loaded)| {
                            // The layout size travels in the picture's own natural size.
                            let image = RasterImage::new(
                                loaded.image.width,
                                loaded.image.height,
                                loaded.image.rgba.clone(),
                            )
                            .with_natural(loaded.natural);
                            (src, Arc::new(image))
                        })
                        .collect(),
                    next_timer_due: None,
                    clock_ms: 0,
                    console,
                });
                let done = ChildDone {
                    next_timer_due: host.next_timer_due(),
                    clock_ms: host.now_ms(),
                    ..done
                };
                write_frame(&mut output, &FromChild::Done(done).encode())?;
            }
            ToChild::Doc(bytes) => {
                // The page changed the document itself (typing, say): this is it now.
                if let Some(doc) = Document::decode(&bytes) {
                    host.with_state(|s| s.doc = doc);
                }
            }
            ToChild::FetchDoc => {
                // The journal travels with the document and starts over, so the page
                // sees each change once.
                let (doc, changes) =
                    host.with_state(|s| (s.doc.encode(), s.doc.take_changes().encode()));
                write_frame(&mut output, &FromChild::Doc { doc, changes }.encode())?;
            }
            ToChild::Init { .. } | ToChild::Answer(_) => {
                return Err(refused("a message arrived out of turn"));
            }
        }
    }
}
