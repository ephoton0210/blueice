// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The conversation between the page and the process that runs its scripts.
//!
//! Plain length-prefixed binary, written by hand: the two ends are one program
//! at one version, there is nothing to negotiate, and a message that does not
//! decode whole is refused, never trusted. Everything the scripts hand back
//! (navigations, network requests, pictures) is data the page validates as it
//! would from a thread; the process has no network and no files of its own.

use super::dom_bridge::NavigationRequest;
use super::net::NetJob;
use super::turn::{ScriptPlan, Startup, Turn};
use blueice_image::RasterImage;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Arc;

/// The largest single message either side accepts.
const MAX_FRAME: usize = 512 * 1024 * 1024;

// ---------------------------------------------------------------- framing

/// Sends one message.
pub(crate) fn write_frame(out: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "message too large"));
    }
    out.write_all(&(payload.len() as u32).to_le_bytes())?;
    out.write_all(payload)?;
    out.flush()
}

/// Reads one message; `Ok(None)` at a clean end of the stream.
pub(crate) fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload)?;
    Ok(Some(payload))
}

// ---------------------------------------------------------------- primitives

#[derive(Default)]
pub(crate) struct Enc(Vec<u8>);

impl Enc {
    pub fn finish(self) -> Vec<u8> {
        self.0
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u64(v.len() as u64);
        self.0.extend_from_slice(v);
    }
    fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    fn opt_str(&mut self, v: &Option<String>) {
        match v {
            Some(s) => {
                self.u8(1);
                self.str(s);
            }
            None => self.u8(0),
        }
    }
    fn opt_u64(&mut self, v: Option<u64>) {
        match v {
            Some(n) => {
                self.u8(1);
                self.u64(n);
            }
            None => self.u8(0),
        }
    }
    fn strs(&mut self, v: &[String]) {
        self.u64(v.len() as u64);
        for s in v {
            self.str(s);
        }
    }
    fn pairs(&mut self, v: &[(String, String)]) {
        self.u64(v.len() as u64);
        for (a, b) in v {
            self.str(a);
            self.str(b);
        }
    }
    fn map(&mut self, v: &HashMap<String, String>) {
        let mut entries: Vec<_> = v.iter().collect();
        entries.sort();
        self.u64(entries.len() as u64);
        for (k, val) in entries {
            self.str(k);
            self.str(val);
        }
    }
    fn image(&mut self, v: &RasterImage) {
        self.u32(v.width);
        self.u32(v.height);
        self.f64(v.natural().0);
        self.f64(v.natural().1);
        self.bytes(&v.rgba);
    }
}

pub(crate) struct Dec<'a>(&'a [u8]);

impl<'a> Dec<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Dec(bytes)
    }
    pub fn done(&self) -> bool {
        self.0.is_empty()
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().expect("four")))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().expect("eight")))
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|b| i32::from_le_bytes(b.try_into().expect("four")))
    }
    fn f64(&mut self) -> Option<f64> {
        self.take(8).map(|b| f64::from_le_bytes(b.try_into().expect("eight")))
    }
    /// A count that cannot exceed what is left, so corrupt input never reserves
    /// more than it holds.
    fn count(&mut self) -> Option<usize> {
        let n = usize::try_from(self.u64()?).ok()?;
        (n <= self.0.len()).then_some(n)
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.count()?;
        Some(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    fn opt_str(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.str()?)),
            _ => None,
        }
    }
    fn opt_u64(&mut self) -> Option<Option<u64>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.u64()?)),
            _ => None,
        }
    }
    fn strs(&mut self) -> Option<Vec<String>> {
        let n = self.count()?;
        (0..n).map(|_| self.str()).collect()
    }
    fn pairs(&mut self) -> Option<Vec<(String, String)>> {
        let n = self.count()?;
        (0..n).map(|_| Some((self.str()?, self.str()?))).collect()
    }
    fn map(&mut self) -> Option<HashMap<String, String>> {
        Some(self.pairs()?.into_iter().collect())
    }
    fn image(&mut self) -> Option<RasterImage> {
        let (width, height) = (self.u32()?, self.u32()?);
        let natural = (self.f64()?, self.f64()?);
        let rgba = self.bytes()?;
        // The pixel buffer must be exactly the picture it claims to be.
        let wanted = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        (rgba.len() == wanted).then(|| RasterImage::new(width, height, rgba).with_natural(natural))
    }
}

// ---------------------------------------------------------------- turns

impl Turn {
    fn encode(&self, e: &mut Enc) {
        match self {
            Turn::Startup(s) => {
                e.u8(0);
                e.u64(s.scripts.len() as u64);
                for script in &s.scripts {
                    e.u64(script.node);
                    e.bool(script.module);
                    e.opt_str(&script.src);
                    e.str(&script.inline);
                    e.bool(script.deferred);
                }
                e.map(&s.linked);
                e.map(&s.modules);
                e.opt_str(&s.document);
            }
            Turn::ScriptLoaded { node, src, source } => {
                e.u8(1);
                e.u64(*node);
                e.str(src);
                e.opt_str(source);
            }
            Turn::AfterLayout => e.u8(2),
            Turn::Scrolled(y) => {
                e.u8(3);
                e.f64(*y);
            }
            Turn::NetCalls(calls) => {
                e.u8(4);
                e.strs(calls);
            }
            Turn::Tick => e.u8(5),
            Turn::Probe(source) => {
                e.u8(6);
                e.str(source);
            }
        }
    }

    fn decode(d: &mut Dec) -> Option<Turn> {
        Some(match d.u8()? {
            0 => {
                let n = d.count()?;
                let mut scripts = Vec::with_capacity(n);
                for _ in 0..n {
                    scripts.push(ScriptPlan {
                        node: d.u64()?,
                        module: d.bool()?,
                        src: d.opt_str()?,
                        inline: d.str()?,
                        deferred: d.bool()?,
                    });
                }
                Turn::Startup(Startup {
                    scripts,
                    linked: d.map()?,
                    modules: d.map()?,
                    document: d.opt_str()?,
                })
            }
            1 => Turn::ScriptLoaded {
                node: d.u64()?,
                src: d.str()?,
                source: d.opt_str()?,
            },
            2 => Turn::AfterLayout,
            3 => Turn::Scrolled(d.f64()?),
            4 => Turn::NetCalls(d.strs()?),
            5 => Turn::Tick,
            6 => Turn::Probe(d.str()?),
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------- messages

/// What the scripts may ask the page about its layout while a turn runs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Query {
    /// A node's computed value of a CSS property.
    Computed { node: u64, property: String },
    /// A node's decoded picture.
    Image(u64),
}

/// The page's answer to a [`Query`].
#[derive(Debug, Clone)]
pub(crate) enum Answer {
    Text(Option<String>),
    Image(Option<Arc<RasterImage>>),
}

/// Page to scripts' process.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ToChild {
    /// Build the realm for this document.
    Init {
        doc: Vec<u8>,
        url: String,
        viewport: (f64, f64),
    },
    /// The page changed the document itself; this is it now.
    Doc(Vec<u8>),
    /// Run a turn. `viewport` and `url` are the page's now. `rects` are every
    /// laid-out box by node in document coordinates, sent when the page has laid out
    /// again since the last turn that carried them.
    Turn {
        viewport: (f64, f64),
        url: Option<String>,
        rects: Option<Vec<(u64, [f64; 4])>>,
        turn: Turn,
    },
    /// Send the document as it is now.
    FetchDoc,
    Answer(AnswerWire),
    /// The faces the page has now (all of them), for canvas text to name.
    Fonts(Vec<crate::fonts::ScriptFont>),
}

/// [`Answer`] as it travels (a picture is compared by its pixels in tests).
#[derive(Debug, Clone)]
pub(crate) struct AnswerWire(pub Answer);

impl PartialEq for AnswerWire {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Answer::Text(a), Answer::Text(b)) => a == b,
            (Answer::Image(a), Answer::Image(b)) => match (a, b) {
                (Some(a), Some(b)) => {
                    (a.width, a.height, &a.rgba, a.natural())
                        == (b.width, b.height, &b.rgba, b.natural())
                }
                (None, None) => true,
                _ => false,
            },
            _ => false,
        }
    }
}

fn put_font(e: &mut Enc, font: &crate::fonts::ScriptFont) {
    e.str(&font.family);
    e.u32(u32::from(font.weight_min));
    e.u32(u32::from(font.weight_max));
    e.bool(font.italic);
    e.bool(font.slant.is_some());
    let (lo, hi) = font.slant.unwrap_or((0, 0));
    e.u32(u32::from(lo as u16));
    e.u32(u32::from(hi as u16));
    e.u32(u32::from(font.stretch_min));
    e.u32(u32::from(font.stretch_max));
    e.u64(font.ranges.len() as u64);
    for (a, b) in &font.ranges {
        e.u32(*a);
        e.u32(*b);
    }
    e.bytes(&font.bytes);
}

fn get_font(d: &mut Dec) -> Option<crate::fonts::ScriptFont> {
    let family = d.str()?;
    let weight_min = d.u32()? as u16;
    let weight_max = d.u32()? as u16;
    let italic = d.bool()?;
    let has_slant = d.bool()?;
    let (lo, hi) = (d.u32()? as u16 as i16, d.u32()? as u16 as i16);
    let slant = has_slant.then_some((lo, hi));
    let stretch_min = d.u32()? as u16;
    let stretch_max = d.u32()? as u16;
    let m = d.count()?;
    let mut ranges = Vec::with_capacity(m);
    for _ in 0..m {
        ranges.push((d.u32()?, d.u32()?));
    }
    Some(crate::fonts::ScriptFont { family, weight_min, weight_max, italic, slant, stretch_min, stretch_max, ranges, bytes: d.bytes()? })
}

impl ToChild {
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Enc::default();
        match self {
            ToChild::Init { doc, url, viewport } => {
                e.u8(0);
                e.bytes(doc);
                e.str(url);
                e.f64(viewport.0);
                e.f64(viewport.1);
            }
            ToChild::Doc(doc) => {
                e.u8(1);
                e.bytes(doc);
            }
            ToChild::Turn {
                viewport,
                url,
                rects,
                turn,
            } => {
                e.u8(2);
                e.f64(viewport.0);
                e.f64(viewport.1);
                e.opt_str(url);
                match rects {
                    Some(rects) => {
                        e.u8(1);
                        e.u64(rects.len() as u64);
                        for (node, rect) in rects {
                            e.u64(*node);
                            for v in rect {
                                e.f64(*v);
                            }
                        }
                    }
                    None => e.u8(0),
                }
                turn.encode(&mut e);
            }
            ToChild::FetchDoc => e.u8(3),
            ToChild::Fonts(fonts) => {
                e.u8(5);
                e.u64(fonts.len() as u64);
                for font in fonts {
                    put_font(&mut e, font);
                }
            }
            ToChild::Answer(AnswerWire(answer)) => {
                e.u8(4);
                match answer {
                    Answer::Text(t) => {
                        e.u8(0);
                        e.opt_str(t);
                    }
                    Answer::Image(i) => {
                        e.u8(2);
                        match i {
                            Some(image) => {
                                e.u8(1);
                                e.image(image);
                            }
                            None => e.u8(0),
                        }
                    }
                }
            }
        }
        e.finish()
    }

    pub fn decode(bytes: &[u8]) -> Option<ToChild> {
        let mut d = Dec::new(bytes);
        let message = match d.u8()? {
            0 => ToChild::Init {
                doc: d.bytes()?,
                url: d.str()?,
                viewport: (d.f64()?, d.f64()?),
            },
            1 => ToChild::Doc(d.bytes()?),
            2 => ToChild::Turn {
                viewport: (d.f64()?, d.f64()?),
                url: d.opt_str()?,
                rects: match d.u8()? {
                    0 => None,
                    1 => {
                        let n = d.count()?;
                        let mut rects = Vec::with_capacity(n);
                        for _ in 0..n {
                            rects.push((d.u64()?, [d.f64()?, d.f64()?, d.f64()?, d.f64()?]));
                        }
                        Some(rects)
                    }
                    _ => return None,
                },
                turn: Turn::decode(&mut d)?,
            },
            3 => ToChild::FetchDoc,
            5 => {
                let n = d.count()?;
                let mut fonts = Vec::with_capacity(n);
                for _ in 0..n {
                    fonts.push(get_font(&mut d)?);
                }
                ToChild::Fonts(fonts)
            }
            4 => ToChild::Answer(AnswerWire(match d.u8()? {
                0 => Answer::Text(d.opt_str()?),
                2 => Answer::Image(match d.u8()? {
                    0 => None,
                    1 => Some(Arc::new(d.image()?)),
                    _ => return None,
                }),
                _ => return None,
            })),
            _ => return None,
        };
        d.done().then_some(message)
    }
}

/// What a turn leaves for the page, apart from the document.
#[derive(Debug, Clone)]
pub(crate) struct ChildDone {
    pub dirty: bool,
    pub navigations: Vec<NavigationRequest>,
    pub script_requests: Vec<(u64, String)>,
    pub url: String,
    pub net_jobs: Vec<NetJob>,
    pub canvases: Vec<(u64, Arc<RasterImage>)>,
    pub loaded_images: Vec<(String, Arc<RasterImage>)>,
    pub loaded_fonts: Vec<crate::fonts::ScriptFont>,
    /// Where a script asked the window to scroll to (document pixels from the top).
    pub scroll_to: Option<f64>,
    pub next_timer_due: Option<u64>,
    pub clock_ms: u64,
    pub console: Vec<String>,
}

/// Scripts' process to the page.
#[derive(Debug, Clone)]
pub(crate) enum FromChild {
    Ready,
    InitFailed(String),
    Done(Box<ChildDone>),
    Query(Query),
    /// The document as it is now, and what changed in it since the page last
    /// had it (a change journal), so the page can restyle incrementally.
    Doc { doc: Vec<u8>, changes: Vec<u8> },
}

impl FromChild {
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Enc::default();
        match self {
            FromChild::Ready => e.u8(0),
            FromChild::InitFailed(why) => {
                e.u8(1);
                e.str(why);
            }
            FromChild::Done(done) => {
                e.u8(2);
                e.bool(done.dirty);
                e.u64(done.navigations.len() as u64);
                for navigation in &done.navigations {
                    match navigation {
                        NavigationRequest::Load { url, replace } => {
                            e.u8(0);
                            e.str(url);
                            e.bool(*replace);
                        }
                        NavigationRequest::PushState { url, replace } => {
                            e.u8(1);
                            e.str(url);
                            e.bool(*replace);
                        }
                        NavigationRequest::Reload => e.u8(2),
                        NavigationRequest::Go(n) => {
                            e.u8(3);
                            e.i32(*n);
                        }
                    }
                }
                e.u64(done.script_requests.len() as u64);
                for (node, url) in &done.script_requests {
                    e.u64(*node);
                    e.str(url);
                }
                e.str(&done.url);
                e.u64(done.net_jobs.len() as u64);
                for job in &done.net_jobs {
                    e.u64(job.id);
                    e.str(&job.method);
                    e.str(&job.url);
                    e.pairs(&job.headers);
                    e.bytes(&job.body);
                    e.str(&job.origin);
                    e.bool(job.no_cors);
                }
                e.u64(done.canvases.len() as u64);
                for (node, image) in &done.canvases {
                    e.u64(*node);
                    e.image(image);
                }
                e.u64(done.loaded_images.len() as u64);
                for (src, image) in &done.loaded_images {
                    e.str(src);
                    e.image(image);
                }
                e.u64(done.loaded_fonts.len() as u64);
                for font in &done.loaded_fonts {
                    put_font(&mut e, font);
                }
                e.bool(done.scroll_to.is_some());
                e.f64(done.scroll_to.unwrap_or(0.0));
                e.opt_u64(done.next_timer_due);
                e.u64(done.clock_ms);
                e.strs(&done.console);
            }
            FromChild::Query(query) => {
                e.u8(3);
                match query {
                    Query::Computed { node, property } => {
                        e.u8(0);
                        e.u64(*node);
                        e.str(property);
                    }
                    Query::Image(node) => {
                        e.u8(2);
                        e.u64(*node);
                    }
                }
            }
            FromChild::Doc { doc, changes } => {
                e.u8(4);
                e.bytes(doc);
                e.bytes(changes);
            }
        }
        e.finish()
    }

    pub fn decode(bytes: &[u8]) -> Option<FromChild> {
        let mut d = Dec::new(bytes);
        let message = match d.u8()? {
            0 => FromChild::Ready,
            1 => FromChild::InitFailed(d.str()?),
            2 => {
                let dirty = d.bool()?;
                let n = d.count()?;
                let mut navigations = Vec::with_capacity(n);
                for _ in 0..n {
                    navigations.push(match d.u8()? {
                        0 => NavigationRequest::Load {
                            url: d.str()?,
                            replace: d.bool()?,
                        },
                        1 => NavigationRequest::PushState {
                            url: d.str()?,
                            replace: d.bool()?,
                        },
                        2 => NavigationRequest::Reload,
                        3 => NavigationRequest::Go(d.i32()?),
                        _ => return None,
                    });
                }
                let n = d.count()?;
                let mut script_requests = Vec::with_capacity(n);
                for _ in 0..n {
                    script_requests.push((d.u64()?, d.str()?));
                }
                let url = d.str()?;
                let n = d.count()?;
                let mut net_jobs = Vec::with_capacity(n);
                for _ in 0..n {
                    net_jobs.push(NetJob {
                        id: d.u64()?,
                        method: d.str()?,
                        url: d.str()?,
                        headers: d.pairs()?,
                        body: d.bytes()?,
                        origin: d.str()?,
                        no_cors: d.bool()?,
                    });
                }
                let n = d.count()?;
                let mut canvases = Vec::with_capacity(n);
                for _ in 0..n {
                    canvases.push((d.u64()?, Arc::new(d.image()?)));
                }
                let n = d.count()?;
                let mut loaded_images = Vec::with_capacity(n);
                for _ in 0..n {
                    loaded_images.push((d.str()?, Arc::new(d.image()?)));
                }
                let n = d.count()?;
                let mut loaded_fonts = Vec::with_capacity(n);
                for _ in 0..n {
                    loaded_fonts.push(get_font(&mut d)?);
                }
                FromChild::Done(Box::new(ChildDone {
                    dirty,
                    navigations,
                    script_requests,
                    url,
                    net_jobs,
                    canvases,
                    loaded_images,
                    loaded_fonts,
                    scroll_to: {
                        let asked = d.bool()?;
                        let y = d.f64()?;
                        asked.then_some(y)
                    },
                    next_timer_due: d.opt_u64()?,
                    clock_ms: d.u64()?,
                    console: d.strs()?,
                }))
            }
            3 => FromChild::Query(match d.u8()? {
                0 => Query::Computed {
                    node: d.u64()?,
                    property: d.str()?,
                },
                2 => Query::Image(d.u64()?),
                _ => return None,
            }),
            4 => FromChild::Doc {
                doc: d.bytes()?,
                changes: d.bytes()?,
            },
            _ => return None,
        };
        d.done().then_some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, seed: u8) -> Arc<RasterImage> {
        let rgba = (0..w * h * 4).map(|i| (i as u8).wrapping_add(seed)).collect();
        Arc::new(RasterImage::new(w, h, rgba).with_natural((f64::from(w) / 2.0, f64::from(h) / 2.0)))
    }

    fn same_picture(a: &RasterImage, b: &RasterImage) -> bool {
        (a.width, a.height, &a.rgba, a.natural()) == (b.width, b.height, &b.rgba, b.natural())
    }

    #[test]
    fn frames_carry_messages_whole_and_a_closed_stream_ends_cleanly() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"first").unwrap();
        write_frame(&mut wire, b"").unwrap();
        write_frame(&mut wire, &vec![7u8; 100_000]).unwrap();
        let mut input = wire.as_slice();
        assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"first");
        assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"");
        assert_eq!(read_frame(&mut input).unwrap().unwrap().len(), 100_000);
        assert!(read_frame(&mut input).unwrap().is_none());
        // A frame cut short is an error, not a short message.
        let mut cut = &wire[..8];
        assert!(read_frame(&mut cut).is_err());
        // An absurd length is refused before anything is allocated for it.
        let mut huge = &[0xff, 0xff, 0xff, 0xff, 1, 2][..];
        assert!(read_frame(&mut huge).is_err());
    }

    fn every_turn() -> Vec<Turn> {
        vec![
            Turn::Startup(Startup {
                scripts: vec![
                    ScriptPlan {
                        node: 12,
                        module: false,
                        src: Some("https://e.test/a.js".into()),
                        inline: String::new(),
                        deferred: true,
                    },
                    ScriptPlan {
                        node: 13,
                        module: true,
                        src: None,
                        inline: "export const x = '你好';".into(),
                        deferred: false,
                    },
                ],
                linked: HashMap::from([("a.js".to_string(), "var a = 1;".to_string())]),
                modules: HashMap::from([("k".to_string(), "export {}".to_string())]),
                document: Some("https://e.test/p".into()),
            }),
            Turn::Startup(Startup::default()),
            Turn::ScriptLoaded {
                node: 9,
                src: "s".into(),
                source: Some("code".into()),
            },
            Turn::ScriptLoaded {
                node: 9,
                src: "s".into(),
                source: None,
            },
            Turn::AfterLayout,
            Turn::Scrolled(-12.5),
            Turn::NetCalls(vec!["a();".into(), "b(\"\\n\");".into()]),
            Turn::NetCalls(vec![]),
            Turn::Tick,
            Turn::Probe("1 + 1".into()),
        ]
    }

    #[test]
    fn every_message_to_the_scripts_round_trips() {
        let mut messages = vec![
            ToChild::Init {
                doc: vec![1, 2, 3],
                url: "https://e.test/".into(),
                viewport: (800.0, 600.5),
            },
            ToChild::Doc(vec![]),
            ToChild::FetchDoc,
            ToChild::Fonts(vec![crate::fonts::ScriptFont {
                family: "x".into(), weight_min: 400, weight_max: 700, italic: false, slant: None, stretch_min: 750, stretch_max: 1250,
                ranges: vec![(1, 2)], bytes: vec![9, 8, 7],
            }]),
            ToChild::Answer(AnswerWire(Answer::Text(Some("block".into())))),
            ToChild::Answer(AnswerWire(Answer::Text(None))),
            ToChild::Answer(AnswerWire(Answer::Image(Some(picture(3, 2, 9))))),
            ToChild::Answer(AnswerWire(Answer::Image(None))),
        ];
        for turn in every_turn() {
            messages.push(ToChild::Turn {
                viewport: (1280.0, 720.0),
                url: Some("https://e.test/x".into()),
                rects: Some(vec![(3, [1.0, 2.0, 3.5, 4.25]), (9, [0.0, 0.0, 0.0, 0.0])]),
                turn: turn.clone(),
            });
            messages.push(ToChild::Turn {
                viewport: (1.0, 2.0),
                url: None,
                rects: None,
                turn,
            });
        }
        for message in messages {
            let bytes = message.encode();
            assert_eq!(ToChild::decode(&bytes), Some(message.clone()), "{message:?}");
            for cut in 0..bytes.len() {
                assert!(ToChild::decode(&bytes[..cut]).is_none(), "{message:?} cut at {cut}");
            }
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(ToChild::decode(&longer).is_none(), "trailing bytes");
        }
    }

    #[test]
    fn every_message_from_the_scripts_round_trips() {
        let done = ChildDone {
            dirty: true,
            navigations: vec![
                NavigationRequest::Load {
                    url: "https://e.test/n".into(),
                    replace: true,
                },
                NavigationRequest::PushState {
                    url: "/p".into(),
                    replace: false,
                },
                NavigationRequest::Reload,
                NavigationRequest::Go(-2),
            ],
            script_requests: vec![(4, "https://e.test/s.js".into())],
            url: "https://e.test/p".into(),
            net_jobs: vec![NetJob {
                id: 77,
                method: "POST".into(),
                url: "https://e.test/api".into(),
                headers: vec![("content-type".into(), "application/json".into())],
                body: vec![0, 255, 7],
                origin: "https://e.test".into(),
                no_cors: false,
            }],
            canvases: vec![(5, picture(4, 4, 1))],
            loaded_images: vec![("img.png".into(), picture(2, 3, 5))],
            loaded_fonts: vec![crate::fonts::ScriptFont {
                family: "Dyn".into(),
                weight_min: 300,
                weight_max: 700,
                italic: true,
                slant: Some((-50, 150)),
                stretch_min: 500,
                stretch_max: 2000,
                ranges: vec![(0x41, 0x5a), (0x61, 0x61)],
                bytes: vec![1, 2, 3, 4, 5],
            }],
            scroll_to: Some(1234.5),
            next_timer_due: Some(1234),
            clock_ms: 99,
            console: vec!["[log] hi".into(), "[error] 失敗".into()],
        };
        let back = match FromChild::decode(&FromChild::Done(Box::new(done.clone())).encode()).unwrap() {
            FromChild::Done(back) => back,
            other => panic!("{other:?}"),
        };
        assert_eq!(back.dirty, done.dirty);
        assert_eq!(back.navigations, done.navigations);
        assert_eq!(back.script_requests, done.script_requests);
        assert_eq!(back.url, done.url);
        assert_eq!(back.net_jobs, done.net_jobs);
        assert_eq!(back.scroll_to, Some(1234.5));
        let none = ChildDone { scroll_to: None, ..done.clone() };
        match FromChild::decode(&FromChild::Done(Box::new(none)).encode()).unwrap() {
            FromChild::Done(back) => assert_eq!(back.scroll_to, None),
            other => panic!("{other:?}"),
        }
        assert_eq!(back.next_timer_due, done.next_timer_due);
        assert_eq!(back.clock_ms, done.clock_ms);
        assert_eq!(back.console, done.console);
        assert_eq!(back.canvases[0].0, 5);
        assert!(same_picture(&back.canvases[0].1, &done.canvases[0].1));
        assert_eq!(back.loaded_images[0].0, "img.png");
        let font = &back.loaded_fonts[0];
        assert_eq!((font.family.as_str(), font.weight_min, font.weight_max, font.italic), ("Dyn", 300, 700, true));
        assert_eq!((font.ranges.clone(), font.bytes.clone()), (vec![(0x41, 0x5a), (0x61, 0x61)], vec![1, 2, 3, 4, 5]));
        assert!(same_picture(&back.loaded_images[0].1, &done.loaded_images[0].1));

        for message in [
            FromChild::Ready,
            FromChild::InitFailed("no realm".into()),
            FromChild::Query(Query::Computed {
                node: 3,
                property: "color".into(),
            }),
            FromChild::Query(Query::Image(9)),
            FromChild::Doc {
                doc: vec![5; 40],
                changes: vec![1, 2],
            },
            FromChild::Done(Box::new(done)),
        ] {
            let bytes = message.encode();
            assert!(FromChild::decode(&bytes).is_some(), "{message:?}");
            for cut in 0..bytes.len() {
                assert!(FromChild::decode(&bytes[..cut]).is_none(), "{message:?} cut at {cut}");
            }
        }
    }

    #[test]
    fn a_picture_whose_pixels_are_not_its_size_is_refused() {
        let image = picture(2, 2, 0);
        let mut e = Enc::default();
        e.u8(4);
        e.u8(2);
        e.u8(1);
        e.u32(image.width + 1); // claims a bigger picture than the pixels it carries
        e.u32(image.height);
        e.f64(1.0);
        e.f64(1.0);
        e.bytes(&image.rgba);
        assert!(ToChild::decode(&e.finish()).is_none());
    }

    #[test]
    fn unknown_message_kinds_and_garbage_are_refused_without_panicking() {
        assert!(ToChild::decode(&[]).is_none());
        assert!(ToChild::decode(&[200]).is_none());
        assert!(FromChild::decode(&[200]).is_none());
        for seed in 0u8..=255 {
            let junk: Vec<u8> = (0..64u8)
                .map(|i| seed.wrapping_mul(31).wrapping_add(i.wrapping_mul(7)))
                .collect();
            let _ = ToChild::decode(&junk);
            let _ = FromChild::decode(&junk);
        }
    }
}
